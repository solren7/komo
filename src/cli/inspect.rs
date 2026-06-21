//! Operator subcommands (`shion cron list`, `shion session list/clean`).
//!
//! These query the database directly and print to stdout — no LLM, no agent
//! runtime. They are the operator's view into what the gateway will act on.

use crate::{
    domain::{
        reminder::ReminderRepository,
        repository::SessionRepository,
        run::RunRepository,
        task::{TaskRepository, TaskStatus},
    },
    infra::{db::Db, kanban::KanbanDb},
};

fn local_time(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_else(|| unix.to_string())
}

/// List pending reminders with their schedules (recurring ones show the cron
/// expression, one-shots are marked as such).
pub async fn cron_list(db_url: &str) -> anyhow::Result<()> {
    let db = Db::connect(db_url).await?;
    let mut pending = ReminderRepository::list_pending(&db).await?;
    pending.sort_by_key(|r| r.run_at);

    if pending.is_empty() {
        println!("No pending reminders.");
        return Ok(());
    }
    for r in pending {
        if r.is_recurring() {
            println!(
                "{}  [{}]  next {}  {}",
                r.id,
                r.schedule,
                local_time(r.run_at),
                r.message
            );
        } else {
            println!(
                "{}  [one-shot]  due {}  {}",
                r.id,
                local_time(r.run_at),
                r.message
            );
        }
    }
    Ok(())
}

/// List open tasks grouped by status (inbox first — it needs triage).
pub async fn task_list(kanban_url: &str) -> anyhow::Result<()> {
    let db = KanbanDb::connect(kanban_url).await?;
    let open = TaskRepository::list_open(&db).await?;

    if open.is_empty() {
        println!("No open tasks.");
        return Ok(());
    }
    for status in [TaskStatus::Inbox, TaskStatus::Todo, TaskStatus::Waiting] {
        let group: Vec<_> = open.iter().filter(|t| t.status == status).collect();
        if group.is_empty() {
            continue;
        }
        println!("{}:", status.as_str());
        for t in group {
            let mut line = format!("  {}  {}", t.id, t.title);
            if !t.board.is_empty() {
                line.push_str(&format!("  #{}", t.board));
            }
            if !t.waiting_on.is_empty() {
                line.push_str(&format!("  (waiting on: {})", t.waiting_on));
            }
            if let Some(due) = t.due_at {
                line.push_str(&format!("  due {}", local_time(due)));
            }
            println!("{line}");
        }
    }
    Ok(())
}

/// Truncate a string to `n` chars for a single-line summary, collapsing newlines.
fn oneline(s: &str, n: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= n {
        flat
    } else {
        let mut out: String = flat.chars().take(n).collect();
        out.push('…');
        out
    }
}

/// List recent runs (most recent first), one per line: id, status, time, plan,
/// and a snippet of the input. The run ledger (roadmap §7).
pub async fn run_list(db_url: &str, limit: usize) -> anyhow::Result<()> {
    let db = Db::connect(db_url).await?;
    let runs = RunRepository::list(&db, limit).await?;

    if runs.is_empty() {
        println!("No runs recorded.");
        return Ok(());
    }
    for r in runs {
        println!(
            "{}  [{}]  {}  {}  {}",
            r.id,
            r.status.as_str(),
            local_time(r.started_at),
            if r.plan.is_empty() { "-" } else { &r.plan },
            oneline(&r.input, 60),
        );
    }
    Ok(())
}

/// Show one run in full: its input, plan, outcome, and every tool step in order.
pub async fn run_inspect(db_url: &str, id: &str) -> anyhow::Result<()> {
    let db = Db::connect(db_url).await?;
    let Some(run) = RunRepository::get(&db, id).await? else {
        println!("No run with id `{id}`.");
        return Ok(());
    };

    println!("run     {}", run.id);
    println!("session {}", run.session_id);
    println!("status  {}", run.status.as_str());
    println!("started {}", local_time(run.started_at));
    if let Some(ended) = run.ended_at {
        println!("ended   {}", local_time(ended));
    }
    if !run.plan.is_empty() {
        println!("plan    {}", run.plan);
    }
    println!("input   {}", oneline(&run.input, 200));
    if !run.error.is_empty() {
        println!("error   {}", run.error);
    }
    if !run.final_output.is_empty() {
        println!("output  {}", oneline(&run.final_output, 200));
    }

    let steps = RunRepository::steps(&db, &run.id).await?;
    if steps.is_empty() {
        println!("\n(no tool steps)");
        return Ok(());
    }
    println!("\nsteps:");
    for s in steps {
        let mark = if s.ok { "ok " } else { "ERR" };
        println!("  #{}  {}  {}", s.seq, mark, s.tool_name);
        println!("      args   {}", oneline(&s.args, 120));
        if s.ok {
            println!("      result {}", oneline(&s.result, 120));
        } else {
            println!("      error  {}", oneline(&s.error, 120));
        }
    }
    Ok(())
}

/// List stored sessions with creation time and message counts.
pub async fn session_list(db_url: &str) -> anyhow::Result<()> {
    let db = Db::connect(db_url).await?;
    let sessions = SessionRepository::list(&db).await?;

    if sessions.is_empty() {
        println!("No sessions.");
        return Ok(());
    }
    for s in sessions {
        println!(
            "{}  created {}  {} messages ({} user turns)",
            s.id,
            local_time(s.created_at),
            s.messages.len(),
            s.user_turns()
        );
    }
    Ok(())
}

/// Delete every session with zero messages. An operator action — run it by
/// hand or from an external scheduler (launchd/cron), e.g. daily at 4am.
pub async fn session_clean(db_url: &str) -> anyhow::Result<()> {
    let db = Db::connect(db_url).await?;
    let removed = SessionRepository::delete_empty_sessions(&db).await?;
    println!("Removed {removed} empty session(s).");
    Ok(())
}
