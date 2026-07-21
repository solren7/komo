use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::domain::context::ToolContext;

/// A tool's result, split into the three views opencode v2 keeps separate: a
/// short human/UI `title`, the `text` the model sees, and `structured` data for
/// programmatic/ledger consumers (unused by the model, so it costs no tokens).
///
/// Most tools only produce text — [`ToolOutput::text`] is the one-liner for
/// that. `title`/`structured` are opt-in via the builders.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub title: Option<String>,
    pub text: String,
    pub structured: Value,
}

impl ToolOutput {
    /// The common case: model-facing text, no title, no structured view.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            title: None,
            text: text.into(),
            structured: Value::Null,
        }
    }

    /// Attach a short human/UI title (shown in logs and, later, the TUI).
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Attach a structured view for programmatic/ledger consumers.
    pub fn with_structured(mut self, structured: Value) -> Self {
        self.structured = structured;
        self
    }
}

/// A tool-call failure, classified so the executor can render it the right way.
///
/// [`InvalidInput`](ToolError::InvalidInput) and [`Denied`](ToolError::Denied)
/// are **recoverable**: the executor turns them into model-facing content (a
/// "rewrite the arguments" nudge, or the denial reason) and never retries. Only
/// [`Failed`](ToolError::Failed) flows through the transient-retry path — it
/// carries the underlying `anyhow::Error` (and any [`TransientError`]) so the
/// existing retry classification is unchanged. `?` on an `anyhow::Error` inside
/// a tool auto-wraps to `Failed` via the `From` impl.
#[derive(Debug)]
pub enum ToolError {
    /// Arguments did not match the tool's schema. Not retried.
    InvalidInput(String),
    /// The action was refused (approval denied / policy block). Not retried.
    Denied(String),
    /// A genuine execution failure — retried if transient (see [`RetryHint`]).
    Failed(anyhow::Error),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::InvalidInput(m) => write!(f, "invalid tool input: {m}"),
            ToolError::Denied(m) => write!(f, "{m}"),
            ToolError::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError::Failed(e)
    }
}

/// Decode a tool's typed arguments from the JSON `Value` the executor parsed,
/// mapping a schema mismatch to the canonical [`ToolError::InvalidInput`] — the
/// one place tool arguments are validated, replacing each tool's hand-rolled
/// `serde_json::from_str` + ad-hoc error string.
pub fn parse_args<T: DeserializeOwned>(input: &Value) -> Result<T, ToolError> {
    serde_json::from_value(input.clone()).map_err(|e| ToolError::InvalidInput(e.to_string()))
}

/// A failure's retry-safety, classified at its source (where the typed cause is
/// still intact — e.g. a `reqwest::Error`, before it is flattened to a string)
/// and carried on the error via [`TransientError`]. The retry layer
/// (`services::tool_execution`) reads this in preference to sniffing the error's
/// Display text. Mirrors the buckets that layer acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryHint {
    /// The request provably never reached the server (connection refused, DNS
    /// failure). Safe to retry for any tool — no side effect can have landed.
    Connection,
    /// Landed-or-not is ambiguous (timeout, 5xx, rate-limit). Retry only an
    /// idempotent tool, so a side effect is never applied twice.
    Ambiguous,
}

/// An error that classifies its own retry-safety via a [`RetryHint`]. A tool
/// builds one at the failure's source (see `tools::http`) so the retry layer
/// decides from a typed signal rather than a heuristic string match; anything
/// that doesn't classify itself falls back to that heuristic.
#[derive(Debug)]
pub struct TransientError {
    pub hint: RetryHint,
    pub message: String,
}

impl TransientError {
    pub fn new(hint: RetryHint, message: impl Into<String>) -> Self {
        Self {
            hint,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TransientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransientError {}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    /// JSON Schema describing this tool's arguments, exposed to the LLM for
    /// function calling. Defaults to "no arguments". Tools that take arguments
    /// override this and parse the matching JSON object from `execute`'s input.
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    /// **Tool trait v2 entry point.** Execute the tool with the parsed JSON
    /// `input` and the explicit per-call [`ToolContext`] (session + run +
    /// approver). Returns a structured [`ToolOutput`]; recoverable problems use
    /// [`ToolError::InvalidInput`] / [`ToolError::Denied`].
    ///
    /// During the v2 migration this has a default that bridges to the legacy
    /// [`execute`](Tool::execute): an unmigrated tool implements `execute` and
    /// inherits this bridge; a migrated tool overrides `call` and leaves
    /// `execute` as its (never-called) default. Once every tool is migrated,
    /// `execute` and this default are removed and `call` becomes required.
    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // Bridge to the legacy string API. `Value::String` carries a raw
        // (non-JSON) arg string the executor wrapped; `Null` is the no-arg call.
        let legacy = match input {
            Value::Null => String::new(),
            Value::String(s) => s,
            other => other.to_string(),
        };
        self.execute(legacy)
            .await
            .map(ToolOutput::text)
            .map_err(ToolError::Failed)
    }

    /// Legacy string-in/string-out execution (tool trait v1). Unmigrated tools
    /// implement this; migrated tools implement [`call`](Tool::call) instead and
    /// leave this default, which is never invoked for them.
    async fn execute(&self, _input: String) -> anyhow::Result<String> {
        unimplemented!("this tool implements `Tool::call`, not the legacy `execute`")
    }

    /// Whether `execute` is safe to retry after a transient failure whose
    /// side-effect status is *ambiguous* — a timeout or 5xx that may already
    /// have landed and applied server-side. Read-only tools (`web_fetch`,
    /// `web_search`) return `true`; any tool that can mutate external state
    /// keeps the default `false`, so a retry can never double-apply an effect
    /// (e.g. fire a Home Assistant service or run a shell command twice).
    ///
    /// Connection-level failures (the request provably never reached the
    /// server — connection refused, DNS failure) are retried regardless of
    /// this flag; see `services::tool_execution`.
    fn idempotent(&self) -> bool {
        false
    }

    /// Sanitize the raw arguments before they are written to the run ledger
    /// (`services::tool_execution`). The ledger stores tool
    /// args verbatim by default (this identity impl); tools carrying sensitive
    /// payloads override it so secrets/large bodies never land in `state.db`.
    /// `shell` scrubs secret-looking substrings, `file` drops write bodies.
    fn redact_args(&self, args: &str) -> String {
        args.to_string()
    }
}
