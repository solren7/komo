//! Live turn events for clients that want to watch the agent work in real time
//! (the desktop GUI's tool-call activity feed).
//!
//! komo's rig tool loop has no token-level streaming, so this streams the
//! *tool-call process* — each tool starting and finishing — not the assistant
//! text token-by-token. Mirrors the [`ReplySink`](crate::domain::gateway::ReplySink)
//!
//! The payload deliberately mirrors what the run ledger records for the same
//! call ([`RunStep`](crate::domain::run::RunStep)) — same truncation cap, same
//! measured duration — because a client renders the live feed and the reloaded
//! transcript with the same component. Anything the stream says less precisely
//! than the ledger becomes a visible jump when the page reloads.
//! pattern: a domain trait with no I/O and no tokio, so komo-core stays
//! dependency-light. The infra layer (the api channel) provides an mpsc-backed
//! impl; every non-streaming caller leaves the sink absent.

use serde::{Deserialize, Serialize};

/// One event emitted during a turn. Serialized to JSON for the SSE stream
/// (`{"type":"tool_started", ...}`) and deserialized back by the gateway
/// client that consumes that stream (the chat TUI's live tool feed).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    /// A tool call is about to run.
    ToolStarted {
        /// The turn's ledger sequence for this call (`-1` when un-ledgered).
        seq: i64,
        name: String,
        /// Redacted arguments (secrets scrubbed, same as the ledger stores).
        args: String,
        /// Wall clock at the start, in unix **milliseconds**. A watcher renders
        /// a live duration off this, so whole seconds (what the ledger's
        /// `started_at` carries) are too coarse.
        ///
        /// `default` for the same reason as [`RunStep::elapsed_ms`](crate::domain::run::RunStep):
        /// the chat TUI deserializes these frames off a possibly-older gateway,
        /// and an unknown field must degrade to "no timing", not to a frame that
        /// fails to parse and takes the whole live feed down with it.
        #[serde(default)]
        started_at_ms: i64,
    },
    /// A tool call finished (after any transient-error retries collapse).
    ToolFinished {
        seq: i64,
        name: String,
        ok: bool,
        /// Result (on success) or error message (on failure), truncated to the
        /// same cap the ledger stores — so a watcher's live rendering of a call
        /// and its re-hydrated rendering from the ledger say the same thing.
        summary: String,
        /// Measured duration, from a monotonic clock (so the retry collapse and
        /// sub-second calls are both faithful). `default` as above.
        #[serde(default)]
        elapsed_ms: i64,
    },
}

/// Sink for [`TurnEvent`]s. Sync + fire-and-forget so it can be called from deep
/// inside the tool executor (including spawned per-tool tasks) without an
/// `async` hop. Absent (`None` on the session context) for every turn that has
/// no live watcher.
pub trait ToolEventSink: Send + Sync {
    fn emit(&self, event: TurnEvent);
}
