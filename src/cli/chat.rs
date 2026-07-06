use std::sync::Arc;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::{
    cli::{approver::CliApprover, gateway_client::GatewayClient, wiring},
    domain::{approval::Approver, repository::SessionRepository, session::Session},
    infra::persistence::{db::Db, kanban::KanbanDb},
};

/// Start the chat REPL. If a gateway is already running it holds the exclusive
/// db lock, so we can't open the db here — route the conversation to it over the
/// loopback api channel instead (trusted: side-effecting tools auto-approve).
/// Otherwise run the agent in-process against the db, exactly as before.
pub async fn run(db_url: &str, kanban_url: &str) -> anyhow::Result<()> {
    if let Some(gw) = GatewayClient::try_connect().await {
        return run_remote(gw, new_session_id(), false).await;
    }
    run_local(db_url, kanban_url, new_session_id(), false).await
}

/// Continue an existing session (`shion session resume <id>`): reopen the REPL
/// bound to that session id so its history is loaded and the conversation picks
/// up where it left off. Errors if no such session exists — this never creates
/// one (that is `shion chat`'s job).
pub async fn resume(db_url: &str, kanban_url: &str, session_id: &str) -> anyhow::Result<()> {
    if let Some(gw) = GatewayClient::try_connect().await {
        // No db here — confirm the session exists server-side before reopening it.
        let known = gw.sessions().await?.iter().any(|s| s.id == session_id);
        if !known {
            anyhow::bail!("no session with id `{session_id}` (see `shion session list`)");
        }
        return run_remote(gw, session_id.to_string(), true).await;
    }
    run_local(db_url, kanban_url, session_id.to_string(), true).await
}

/// REPL backed by a running gateway over HTTP. No db is opened here; session
/// history lives server-side keyed by the session id we send each turn.
/// `resuming` only changes the greeting — an existing session's transcript is
/// threaded automatically once we send turns under its id.
async fn run_remote(
    gw: GatewayClient,
    mut current_session: String,
    resuming: bool,
) -> anyhow::Result<()> {
    println!(
        "Shion v0.1 — connected to the running gateway ({} `{}`, trusted). Type /new to start a fresh session, Ctrl-C or Ctrl-D to quit.\n",
        if resuming {
            "resumed session"
        } else {
            "session"
        },
        current_session
    );
    let mut editor = DefaultEditor::new()?;
    loop {
        let (line, returned_editor) = tokio::task::spawn_blocking(move || {
            let line = editor.readline("->");
            (line, editor)
        })
        .await?;
        editor = returned_editor;

        let input = match line {
            Ok(line) => line.trim().to_string(),
            Err(ReadlineError::Eof) => break,
            Err(ReadlineError::Interrupted) => break,
            Err(e) => return Err(e.into()),
        };
        if input.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(&input);

        // `/new` rotates the session id client-side; the gateway simply starts a
        // fresh transcript under the new id. (Other `/`-commands aren't routed —
        // use a chat channel for `/sethome` / `/pair`; approval is automatic here.)
        if input == "/new" || input == "/clear" {
            current_session = new_session_id();
            println!("Started new session `{}`.\n", current_session);
            continue;
        }

        match gw.chat(&current_session, &input).await {
            Ok(reply) => println!("Agent: {}\n", reply),
            Err(e) => eprintln!("Error: {e:#}\n"),
        }
    }
    Ok(())
}

/// REPL backed by an in-process agent over the local db (no gateway running).
/// When `resuming`, the session id names an existing transcript that must
/// already exist; otherwise it's a fresh, program-managed session we create.
async fn run_local(
    db_url: &str,
    kanban_url: &str,
    mut current_session: String,
    resuming: bool,
) -> anyhow::Result<()> {
    let db = Arc::new(Db::connect(db_url).await?);
    let kanban = Arc::new(KanbanDb::connect(kanban_url).await?);

    // Resume requires the session to exist already — never create it here.
    if resuming
        && SessionRepository::find(&*db, &current_session)
            .await?
            .is_none()
    {
        anyhow::bail!("no session with id `{current_session}` (see `shion session list`)");
    }

    // Interactive approval at the TTY for side-effecting tools.
    let approver: Arc<dyn Approver> = Arc::new(CliApprover::new());
    let runtime = wiring::build(db.clone(), kanban, approver).await?.runtime;

    ensure_session(&db, &current_session).await?;
    println!(
        "Shion v0.1 — {} `{}`. Type /new (or /clear) to start a fresh session, Ctrl-C or Ctrl-D to quit.\n",
        if resuming {
            "resumed session"
        } else {
            "session"
        },
        current_session
    );

    // `rustyline` runs the terminal in raw mode for the duration of each
    // `readline` call only, decoding UTF-8 and tracking display width itself —
    // so backspace deletes whole multi-byte (CJK) characters instead of
    // corrupting them as the kernel's cooked-mode line discipline does. The
    // editor releases the terminal the moment it returns, so a tool's approval
    // gate (`CliApprover`) can still read stdin while a turn is in flight.
    let mut editor = DefaultEditor::new()?;

    loop {
        // `readline` blocks until the user submits a line; run it on tokio's
        // blocking thread pool so it never pins an async worker thread. The
        // editor moves into the closure and back out each iteration.
        let (line, returned_editor) = tokio::task::spawn_blocking(move || {
            let line = editor.readline("->");
            (line, editor)
        })
        .await?;
        editor = returned_editor;

        let input = match line {
            Ok(line) => line.trim().to_string(),
            Err(ReadlineError::Eof) => break,         // Ctrl-D
            Err(ReadlineError::Interrupted) => break, // Ctrl-C
            Err(e) => return Err(e.into()),
        };
        if input.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(&input);

        // `/new` and `/clear` are equivalent: both start a fresh, program-managed
        // session. There are no user-supplied session ids.
        if input == "/new" || input == "/clear" {
            current_session = new_session_id();
            ensure_session(&db, &current_session).await?;
            println!("Started new session `{}`.\n", current_session);
            continue;
        }

        // No need to echo the input — `rustyline` already left it on the prompt
        // line, so re-printing it would double every message. A failed turn
        // (tool panic, network error, …) is reported and the loop continues;
        // only readline/session errors above end the REPL.
        match runtime.handle_input(&current_session, input).await {
            Ok(reply) => println!("Agent: {}\n", reply),
            Err(e) => eprintln!("Error: {e:#}\n"),
        }
    }

    Ok(())
}

async fn ensure_session(db: &Db, session_id: &str) -> anyhow::Result<()> {
    if SessionRepository::find(db, session_id).await?.is_none() {
        SessionRepository::save(db, &Session::new(session_id)).await?;
    }
    Ok(())
}

fn new_session_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
