//! komo's own provider layer: the wire protocols, the HTTP boundary, and the
//! typed errors that come out of it.
//!
//! # Why this is not a library
//!
//! komo owns its tool loop (`agent::runtime::run_agent_loop`), so what it needs
//! from a provider client is exactly one thing — *one completion per call* — plus
//! honest errors. Every LLM crate it tried instead owned the loop and collapsed
//! provider failures into strings, which cost komo the two decisions it most
//! needs to make well: whether to retry, and whether to shrink the history and
//! try again.
//!
//! # Shape
//!
//! One **wire format** per module, not one per provider ([`Wire`]). This is the
//! structural bet, and it is what keeps the layer small: five providers collapse
//! into two codecs, and a provider komo has never heard of is a base URL plus an
//! auth mode ([`transport::Endpoint`]) rather than new code.
//!
//! - [`responses`] — the OpenAI Responses API: OpenAI, Codex, DeepSeek,
//!   OpenRouter.
//! - [`messages`] — the Anthropic Messages API, which has no Responses surface.
//!
//! [`types`] holds komo's own conversation model, which both codecs translate
//! from, and [`error`] the classified failures every layer above reads.

pub mod error;
pub mod messages;
pub mod responses;
pub mod transport;
pub mod types;

use serde_json::Value;

pub use error::{LlmError, LlmErrorKind};
pub use transport::{Auth, Endpoint, TokenSource};
pub use types::{AssistantBlock, Completion, ToolSchema, Turn, UserBlock};

/// A chunk of output, handed to the caller as it streams in.
///
/// The provider layer deliberately does not know about `TurnEvent` or session
/// contexts — it reports what arrived and lets `infra::llm` decide where that
/// goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta<'a> {
    /// Part of the assistant's visible answer.
    Text(&'a str),
    /// Part of the model's reasoning summary.
    Reasoning(&'a str),
}

/// Callback invoked per streamed chunk. Synchronous and non-blocking: it runs
/// inside the stream loop, so anything slow here stalls the round.
pub type OnDelta<'a> = &'a (dyn Fn(Delta<'_>) + Send + Sync);

/// Which wire protocol a provider speaks.
///
/// Deliberately not a trait: a wire format is a pair of pure functions (build a
/// request, read the events back), and an enum keeps both codecs' full shape
/// visible at the one call site that dispatches on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// `POST /v1/responses` — the system prompt is a top-level field and
    /// reasoning round-trips.
    Responses,
    /// `POST /v1/messages` — Anthropic's native API, with explicit
    /// `cache_control` breakpoints.
    Messages,
}

/// One provider, reachable. Built once at startup and shared across turns.
pub struct ProviderClient {
    pub endpoint: Endpoint,
    pub wire: Wire,
}

impl ProviderClient {
    /// Run one completion: build the request for this wire, stream it, and fold
    /// the events into a single assistant message.
    ///
    /// This is the whole surface the agent uses. Everything that makes a turn a
    /// *turn* — the loop, the budget, the history window, the degrade — lives
    /// above it.
    pub async fn complete(
        &self,
        model: &str,
        instructions: &str,
        history: &[Turn],
        tools: &[ToolSchema],
        extra: Option<&Value>,
        on_delta: Option<OnDelta<'_>>,
    ) -> Result<Completion, LlmError> {
        let body = match self.wire {
            Wire::Responses => responses::request(model, instructions, history, tools, extra),
            Wire::Messages => messages::request(model, instructions, history, tools, extra),
        };
        let mut stream = self.endpoint.stream(&body).await?;
        match self.wire {
            Wire::Responses => responses::collect(&mut stream, on_delta).await,
            Wire::Messages => messages::collect(&mut stream, on_delta).await,
        }
    }
}
