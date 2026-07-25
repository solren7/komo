//! Cooperative turn cancellation.
//!
//! A turn started over the api channel runs on a spawned task, so a client
//! hanging up doesn't stop it — the agent keeps going and its reply lands in the
//! transcript with nobody watching. This is the signal that lets the caller stop
//! it for real.
//!
//! **Cooperative**, in two senses worth being precise about:
//!
//!   - The agent loop stops between rounds and mid-await (the model round-trip
//!     and the tool round are raced against the signal), so cancelling lands
//!     within one await, not after the whole turn.
//!   - A tool call *already executing* still runs to completion — the executor
//!     spawns each call, and [`Tool`](crate::domain::tool::Tool) has no abort
//!     hook. So cancelling means "no further rounds, no further tool calls", not
//!     "kill whatever is running".
//!
//! Like [`ToolEventSink`](crate::domain::events::ToolEventSink), this is a trait
//! so the domain stays runtime-agnostic; the `watch`-channel implementation
//! lives with the api channel's interaction state.

use async_trait::async_trait;

/// What a cancelled turn answers with. Persisted as the assistant message so the
/// transcript stays user/assistant-alternating and re-reading the session shows
/// why it stopped, rather than a question with no reply.
pub const CANCELLED_REPLY: &str = "（已中断）";

/// Ledger `error` for a run the user cancelled. Distinct from
/// [`INTERRUPTED_ERROR`](crate::domain::run::INTERRUPTED_ERROR), which marks
/// crash residue and is resumable — a deliberate cancel is not.
pub const CANCELLED_ERROR: &str = "cancelled by user";

/// The error a cancelled turn fails with, so every layer can tell a cancel from
/// a genuine failure by downcasting.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(CANCELLED_ERROR)
    }
}

impl std::error::Error for Cancelled {}

/// Is this error a turn cancellation?
pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Cancelled>().is_some()
}

/// One turn's cancellation signal.
#[async_trait]
pub trait CancelSignal: Send + Sync {
    /// Cheap check, for the point between rounds.
    fn is_cancelled(&self) -> bool;
    /// Resolves once cancelled. Must never resolve otherwise — it is raced
    /// against real work in a `select!`, so a spurious wake would abort a
    /// perfectly good turn.
    async fn cancelled(&self);
}
