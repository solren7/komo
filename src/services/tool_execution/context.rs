//! Per-turn execution context glue.
//!
//! The context **value types** ([`SessionContext`], [`RunContext`],
//! [`ToolContext`]) now live in `domain::context` — they are pure values over
//! domain traits. This module re-exports them for path stability and adds two
//! service-layer concerns: the per-turn [`ToolTurnContext`] bundle the runtime
//! hands the executor, and the ambient-session task-local.
//!
//! The `SESSION` task-local survives only as an internal compatibility seam:
//! the approvers (`ChatApprover`, `PolicyApprover`) resolve a prompt against the
//! current conversation without threading a context parameter through the
//! `Approver` trait, so the executor installs the turn's session around each
//! spawned tool task and they read [`current_session`]. Migrated tools read
//! `ctx.session` instead. The run context is purely explicit — no ambient state
//! decides whether a turn is ledgered.

pub use crate::domain::context::{RunContext, SessionContext, ToolContext};

/// Everything the executor needs to know about the turn a round of tool calls
/// belongs to. Built once per turn by `AgentRuntime::run_agent_loop`.
#[derive(Clone)]
pub struct ToolTurnContext {
    pub session: SessionContext,
    /// `Some` when the turn is recorded in the run ledger (the main agent);
    /// `None` for callers without a ledger (rig's fallback path).
    pub run: Option<RunContext>,
}

tokio::task_local! {
    pub(super) static SESSION: SessionContext;
}

/// Run `future` with `ctx` as the ambient session context. Called by the turn
/// entry points (the gateway dispatcher, the api channel, `handle_input`); the
/// executor re-installs the context around each spawned tool task.
pub async fn with_session<F: std::future::Future>(ctx: SessionContext, future: F) -> F::Output {
    SESSION.scope(ctx, future).await
}

/// The ambient session context, if the current task is running inside one.
/// `None` for aux sub-agents and maintenance sweeps.
pub fn current_session() -> Option<SessionContext> {
    SESSION.try_with(|c| c.clone()).ok()
}
