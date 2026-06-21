use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tracing::{Instrument, info, info_span, warn};

use crate::domain::{
    gateway::ReplySink,
    run::{RunRepository, RunStep, STEP_FIELD_CAP, truncate},
    tool::Tool,
};

/// Ambient context for the turn a tool is executing within: which session it
/// belongs to and how to talk back to that conversation. Set by the gateway
/// dispatcher around a turn (`agent::interaction`) and read by a chat-channel
/// approver when a tool needs mid-execution approval.
///
/// It rides a task-local rather than the tool's argument string because rig's
/// `ToolDyn::call` signature is fixed — we can't thread it through the LLM
/// tool-call path. `execute_isolated` re-establishes it across its `spawn`.
#[derive(Clone)]
pub struct SessionContext {
    pub session_id: String,
    pub sink: Arc<dyn ReplySink>,
}

impl SessionContext {
    /// A context that knows the session but cannot talk back mid-turn (its sink
    /// is a no-op). Used by the REPL and any caller that has a session id but no
    /// channel to prompt on — enough for session-scoped tools like `todo`, while
    /// a mid-turn approval prompt simply goes nowhere (the REPL gates approvals
    /// at the TTY, not through this sink).
    pub fn detached(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            sink: Arc::new(NoopSink),
        }
    }
}

/// A [`ReplySink`] that drops everything — see [`SessionContext::detached`].
struct NoopSink;

#[async_trait::async_trait]
impl ReplySink for NoopSink {
    async fn send(&self, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

tokio::task_local! {
    static SESSION: SessionContext;
}

/// Run `future` with `ctx` as the ambient session context.
pub async fn with_session<F: std::future::Future>(ctx: SessionContext, future: F) -> F::Output {
    SESSION.scope(ctx, future).await
}

/// The ambient session context, if the current task is running inside one.
/// `None` for the REPL, aux sub-agents, and maintenance sweeps.
pub fn current_session() -> Option<SessionContext> {
    SESSION.try_with(|c| c.clone()).ok()
}

/// Ambient run-ledger context for the turn (`domain/run.rs`, roadmap §7). Set by
/// `AgentRuntime::run_turn` around the turn body so `execute_isolated` — the one
/// choke point every tool call funnels through (both the LLM function-calling
/// path and the keyword-routed path) — can record each tool invocation as a
/// `RunStep`. Absent for aux sub-agents and maintenance sweeps, so their tool
/// use never pollutes the ledger.
#[derive(Clone)]
pub struct RunContext {
    pub run_id: String,
    pub repo: Arc<dyn RunRepository>,
    /// Monotonic step counter, shared across clones so steps within a run get a
    /// stable order even when the context is cloned across tasks.
    seq: Arc<AtomicI64>,
}

impl RunContext {
    pub fn new(run_id: String, repo: Arc<dyn RunRepository>) -> Self {
        Self {
            run_id,
            repo,
            seq: Arc::new(AtomicI64::new(0)),
        }
    }

    fn next_seq(&self) -> i64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// How many tool steps have been claimed so far (the post-turn count).
    pub fn steps_count(&self) -> i64 {
        self.seq.load(Ordering::Relaxed)
    }
}

tokio::task_local! {
    static RUN: RunContext;
}

/// Run `future` with `ctx` as the ambient run-ledger context.
pub async fn with_run<F: std::future::Future>(ctx: RunContext, future: F) -> F::Output {
    RUN.scope(ctx, future).await
}

/// The ambient run-ledger context, if the current turn is being recorded.
pub fn current_run() -> Option<RunContext> {
    RUN.try_with(|c| c.clone()).ok()
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// All registered tools, shared via `Arc` (handed to the LLM agent in
    /// `build_llm`). The registry is now purely this catalog — the LLM owns
    /// tool dispatch via rig, so there is no keyword-routed execute path.
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
    }
}

/// Process-wide cap on the size of a tool result handed back to the LLM. A
/// single tool returning tens of KB (a big file read, a full `/api/states`
/// dump, a long web page) floods the context window *every subsequent turn*,
/// since the result stays in history. Sized **above** the per-tool self-caps
/// (`web_fetch` / `homeassistant` cap themselves at 8 KB) so it never fights a
/// tool that already trims sensibly — it only catches the ones that don't.
///
/// Resolved once at startup from `max_tool_result_bytes`
/// (`SHION_MAX_TOOL_RESULT_BYTES` env > config.toml > `DEFAULT_MAX_TOOL_RESULT_BYTES`)
/// via [`set_tool_result_cap`]. A `OnceLock` rather than a threaded parameter
/// because rig's `ToolDyn::call` signature is fixed — same reason the session /
/// run contexts are ambient. Unset (tests, aux paths) → the built-in default.
static TOOL_RESULT_CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Set the process-wide tool-result byte cap. Called once during wiring
/// (`cli/wiring.rs`); a second call is ignored (first wins).
pub fn set_tool_result_cap(bytes: usize) {
    let _ = TOOL_RESULT_CAP.set(bytes);
}

fn tool_result_cap() -> usize {
    TOOL_RESULT_CAP
        .get()
        .copied()
        .unwrap_or(crate::config::DEFAULT_MAX_TOOL_RESULT_BYTES)
}

/// Truncate an over-long tool result at a UTF-8 char boundary, appending a
/// marker that nudges the model to re-query more narrowly. Short results pass
/// through untouched. Applied uniformly to every tool at the choke point below,
/// so no individual tool has to implement its own ceiling.
fn cap_tool_result(mut out: String) -> String {
    let cap = tool_result_cap();
    if out.len() <= cap {
        return out;
    }
    let mut end = cap;
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out.push_str(&format!(
        "\n\n…[truncated: result exceeded the {} KB tool-result limit. Re-run with \
         a narrower query — a filter, a specific id, or a smaller range — to see the rest.]",
        cap / 1024
    ));
    out
}

/// Runs a tool on its own tokio task, isolated from the caller. This keeps
/// tool work off the chat task's thread and — because `JoinHandle` catches
/// panics — turns a panicking tool into an error reply instead of a process
/// exit. Used by both invocation paths: the keyword-routed registry above and
/// the LLM function-calling adapter (`infra::rig_tool::RigTool`).
pub async fn execute_isolated(tool: Arc<dyn Tool>, input: String) -> anyhow::Result<String> {
    let name = tool.name();

    // Run-ledger bookkeeping (only when this turn is being recorded). Capture
    // the redacted args and seq up front: the raw `input` is moved into the
    // spawned task below, and the seq must be claimed before the tool runs so
    // the span and the persisted step agree.
    let run = current_run();
    let ledger = run.as_ref().map(|r| (r.clone(), r.next_seq()));
    let redacted_args = ledger.as_ref().map(|_| tool.redact_args(&input));
    let started_at = now();

    // Span so the tool's own logs carry the run's `seq`/`name`. Spans don't
    // cross `tokio::spawn` on their own — instrument the spawned future.
    let seq_field = ledger.as_ref().map(|(_, s)| *s).unwrap_or(-1);
    let span = info_span!("tool", name, seq = seq_field);

    // Carry the turn's session context into the spawned task; `tokio::spawn`
    // starts a fresh task that wouldn't otherwise inherit the task-local.
    let join = match current_session() {
        Some(ctx) => tokio::spawn(
            SESSION
                .scope(ctx, async move { tool.execute(input).await })
                .instrument(span),
        ),
        None => tokio::spawn(async move { tool.execute(input).await }.instrument(span)),
    };
    let result = match join.await {
        Ok(result) => result,
        Err(join_err) if join_err.is_panic() => {
            let panic = join_err.into_panic();
            let msg = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(anyhow::anyhow!("tool `{name}` panicked: {msg}"))
        }
        Err(join_err) => Err(anyhow::anyhow!("tool `{name}` was cancelled: {join_err}")),
    };

    // Record the step — best-effort, never affecting the tool's own result.
    if let (Some((run, seq)), Some(args)) = (ledger, redacted_args) {
        let ended_at = now();
        let (ok, result_s, error_s) = match &result {
            Ok(out) => (true, truncate(out, STEP_FIELD_CAP), String::new()),
            Err(e) => (
                false,
                String::new(),
                truncate(&format!("{e:#}"), STEP_FIELD_CAP),
            ),
        };
        if ok {
            info!(
                tool = name,
                seq,
                elapsed_ms = (ended_at - started_at) * 1000,
                "tool ok"
            );
        } else {
            warn!(tool = name, seq, error = %error_s, "tool failed");
        }
        let step = RunStep {
            run_id: run.run_id.clone(),
            seq,
            tool_name: name.to_string(),
            args: truncate(&args, STEP_FIELD_CAP),
            result: result_s,
            error: error_s,
            ok,
            started_at,
            ended_at,
        };
        if let Err(error) = run.repo.append_step(&step).await {
            warn!(%error, tool = name, "failed to record run step (non-fatal)");
        }
    }

    // Cap the LLM-facing result *after* the ledger records the original, so the
    // audit trail stays faithful while the model's context stays bounded.
    result.map(cap_tool_result)
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::run::{Run, RunStep};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Captures appended steps; everything else is inert.
    struct RecordingRuns {
        steps: Mutex<Vec<RunStep>>,
    }

    #[async_trait]
    impl RunRepository for RecordingRuns {
        async fn start(&self, _run: &Run) -> anyhow::Result<()> {
            Ok(())
        }
        async fn append_step(&self, step: &RunStep) -> anyhow::Result<()> {
            self.steps.lock().unwrap().push(step.clone());
            Ok(())
        }
        async fn finish(&self, _run: &Run) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list(&self, _limit: usize) -> anyhow::Result<Vec<Run>> {
            Ok(Vec::new())
        }
        async fn get(&self, _id: &str) -> anyhow::Result<Option<Run>> {
            Ok(None)
        }
        async fn steps(&self, _run_id: &str) -> anyhow::Result<Vec<RunStep>> {
            Ok(Vec::new())
        }
    }

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "echoes its input"
        }
        async fn execute(&self, input: String) -> anyhow::Result<String> {
            Ok(format!("echoed: {input}"))
        }
    }

    #[tokio::test]
    async fn run_context_records_a_step_per_tool_call() {
        let repo = Arc::new(RecordingRuns {
            steps: Mutex::new(Vec::new()),
        });
        let ctx = RunContext::new("run-1".into(), repo.clone());
        with_run(ctx, async {
            execute_isolated(Arc::new(EchoTool), "hi".into())
                .await
                .unwrap();
        })
        .await;

        let steps = repo.steps.lock().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].run_id, "run-1");
        assert_eq!(steps[0].seq, 0);
        assert_eq!(steps[0].tool_name, "echo");
        assert!(steps[0].ok);
        assert!(steps[0].result.contains("echoed: hi"));
        assert!(steps[0].error.is_empty());
    }

    #[tokio::test]
    async fn failed_tool_records_an_error_step() {
        let repo = Arc::new(RecordingRuns {
            steps: Mutex::new(Vec::new()),
        });
        let ctx = RunContext::new("run-2".into(), repo.clone());
        with_run(ctx, async {
            let _ = execute_isolated(Arc::new(PanickingTool), String::new()).await;
        })
        .await;

        let steps = repo.steps.lock().unwrap();
        assert_eq!(steps.len(), 1);
        assert!(!steps[0].ok);
        assert!(steps[0].error.contains("panicked"));
        assert!(steps[0].result.is_empty());
    }

    struct BigTool;
    #[async_trait]
    impl Tool for BigTool {
        fn name(&self) -> &'static str {
            "big"
        }
        fn description(&self) -> &'static str {
            "returns a large result"
        }
        async fn execute(&self, _input: String) -> anyhow::Result<String> {
            Ok("x".repeat(crate::config::DEFAULT_MAX_TOOL_RESULT_BYTES + 5000))
        }
    }

    #[tokio::test]
    async fn oversized_result_is_capped_with_marker() {
        let out = execute_isolated(Arc::new(BigTool), String::new())
            .await
            .unwrap();
        assert!(
            out.len() <= crate::config::DEFAULT_MAX_TOOL_RESULT_BYTES + 200,
            "should be capped"
        );
        assert!(
            out.contains("truncated"),
            "should carry the truncation marker"
        );
    }

    #[tokio::test]
    async fn small_result_passes_through_uncapped() {
        let out = execute_isolated(Arc::new(EchoTool), "hi".into())
            .await
            .unwrap();
        assert_eq!(out, "echoed: hi");
    }

    #[test]
    fn cap_preserves_multibyte_boundaries() {
        // A run of 3-byte CJK chars whose total exceeds the cap: the cut must
        // land on a char boundary, not mid-codepoint (would panic otherwise).
        let big = "界".repeat(crate::config::DEFAULT_MAX_TOOL_RESULT_BYTES); // 3 bytes each
        let capped = cap_tool_result(big);
        assert!(capped.contains("truncated"));
    }

    #[tokio::test]
    async fn no_run_context_records_nothing() {
        // No `with_run` wrapper: execute_isolated must still work and record nada.
        let out = execute_isolated(Arc::new(EchoTool), "x".into())
            .await
            .unwrap();
        assert!(out.contains("echoed: x"));
    }

    struct PanickingTool;

    #[async_trait]
    impl Tool for PanickingTool {
        fn name(&self) -> &'static str {
            "boom"
        }
        fn description(&self) -> &'static str {
            "always panics"
        }
        async fn execute(&self, _input: String) -> anyhow::Result<String> {
            panic!("kaboom");
        }
    }

    #[tokio::test]
    async fn panicking_tool_returns_error_instead_of_crashing() {
        let err = execute_isolated(Arc::new(PanickingTool), String::new())
            .await
            .expect_err("panic should surface as an error");
        let msg = err.to_string();
        assert!(msg.contains("panicked"), "unexpected error: {msg}");
        assert!(msg.contains("kaboom"), "unexpected error: {msg}");
    }
}
