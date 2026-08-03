//! Typed provider failures.
//!
//! komo used to sit behind `rig`, which collapsed every provider failure into a
//! string, so the two decisions that matter most had to be made by sniffing that
//! string: *may I retry this?* and *did the request overflow the context
//! window?* Both are now properties of the error itself, decided once at the
//! HTTP boundary where the status code, the response headers, and the provider's
//! own error `code` are all still intact.
//!
//! Two rules, taken from how codex and grok-build do it:
//!
//! 1. **Retryability is an exhaustive `match`** ([`LlmError::is_retryable`]), so
//!    a new failure kind cannot be added without deciding what retrying it
//!    means — the compiler asks.
//! 2. **The server's own delay wins.** A `Retry-After` is parsed into
//!    [`LlmError::retry_after`] at the boundary and the retry loop prefers it
//!    over any local backoff table: a provider that tells us when the limit
//!    clears is more accurate than a guess, always.

use std::fmt;
use std::time::Duration;

/// The upper bound on a server-supplied `Retry-After`. A provider that asks for
/// an hour is either wrong or telling us the turn is dead; either way waiting
/// that long inside a chat turn is worse than failing and letting the user
/// decide.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// What went wrong on a model round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmErrorKind {
    /// Bad or missing credentials. Never retried — it will fail identically
    /// forever, and the user needs to see the message.
    Auth,
    /// The provider rejected the request as malformed (bad schema, unknown
    /// model, unsupported parameter). Not retryable: the same bytes will be
    /// rejected the same way.
    InvalidRequest,
    /// The request did not fit the model's context window. Not *transient*, but
    /// recoverable by sending less — which is why it gets its own kind rather
    /// than folding into `InvalidRequest`: the turn driver degrades its history
    /// and re-issues the round.
    ContextOverflow,
    /// Rate limited (429). Retryable, and usually carries a `retry_after`.
    RateLimited,
    /// The provider is overloaded or erroring server-side (5xx). Retryable.
    Overloaded,
    /// The request never got a response: connection refused, DNS failure, TLS
    /// error, or a client-side timeout. Retryable — a completion has no side
    /// effect that could double-apply.
    Transport,
    /// The stream ended without a terminal event, so the answer is incomplete.
    /// Retryable, and the anchor of the whole streaming contract: anything short
    /// of an explicit completion frame is a failed round, not a short answer.
    Stream,
    /// komo's *own* per-round deadline elapsed. Not retryable, and deliberately
    /// distinct from [`Self::Transport`]: the deadline bounds every attempt of a
    /// round together, so retrying inside it would be spending a budget that has
    /// already run out.
    Timeout,
}

/// A provider failure, with everything the retry layer needs already extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmError {
    pub kind: LlmErrorKind,
    /// How long the *server* asked us to wait, when it said. Preferred over any
    /// local backoff.
    pub retry_after: Option<Duration>,
    /// The HTTP status, when the failure was an HTTP response.
    pub status: Option<u16>,
    /// The provider's own error `code`, when it sent one (`rate_limit_exceeded`,
    /// `context_length_exceeded`, …). Kept for diagnostics; the classification
    /// decision it drove is already in `kind`.
    pub code: Option<String>,
    pub message: String,
}

impl LlmError {
    pub fn new(kind: LlmErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            retry_after: None,
            status: None,
            code: None,
            message: message.into(),
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(LlmErrorKind::Transport, message)
    }

    pub fn stream(message: impl Into<String>) -> Self {
        Self::new(LlmErrorKind::Stream, message)
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_retry_after(mut self, after: Option<Duration>) -> Self {
        self.retry_after = after.map(|d| d.min(MAX_RETRY_AFTER));
        self
    }

    pub fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }

    /// Whether re-sending the identical request could succeed.
    ///
    /// Exhaustive on purpose: adding a kind forces a decision here rather than
    /// inheriting a default that silently retries (burning a turn's latency) or
    /// silently doesn't (throwing away a recoverable turn).
    pub fn is_retryable(&self) -> bool {
        match self.kind {
            LlmErrorKind::Auth
            | LlmErrorKind::InvalidRequest
            | LlmErrorKind::ContextOverflow
            | LlmErrorKind::Timeout => false,
            LlmErrorKind::RateLimited
            | LlmErrorKind::Overloaded
            | LlmErrorKind::Transport
            | LlmErrorKind::Stream => true,
        }
    }

    pub fn is_context_overflow(&self) -> bool {
        self.kind == LlmErrorKind::ContextOverflow
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) => write!(f, "HTTP {status}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for LlmError {}

/// Classify an HTTP failure into a [`LlmErrorKind`].
///
/// `code` is the provider's own error code from the response body, which is the
/// precise signal where it exists — a 400 carrying `context_length_exceeded` is
/// an overflow, not a malformed request, and only the code distinguishes them.
pub fn classify_status(status: u16, code: Option<&str>, message: &str) -> LlmErrorKind {
    // The provider named the failure: trust that over the status code.
    match code {
        Some("context_length_exceeded") => return LlmErrorKind::ContextOverflow,
        Some("rate_limit_exceeded" | "slow_down") => return LlmErrorKind::RateLimited,
        Some("server_is_overloaded" | "overloaded_error") => return LlmErrorKind::Overloaded,
        Some("invalid_api_key" | "authentication_error") => return LlmErrorKind::Auth,
        _ => {}
    }
    match status {
        401 | 403 => LlmErrorKind::Auth,
        429 => LlmErrorKind::RateLimited,
        // Includes the non-standard overload codes providers reach for under
        // load (Cloudflare's 520, Anthropic's 529) — same meaning, same handling.
        500..=599 => LlmErrorKind::Overloaded,
        // A 413 is literally "your payload is too large", which for a
        // completion request means the context window.
        413 => LlmErrorKind::ContextOverflow,
        _ if looks_like_context_overflow(message) => LlmErrorKind::ContextOverflow,
        _ => LlmErrorKind::InvalidRequest,
    }
}

/// Phrasings that mean "the request did not fit in the model's context window".
///
/// A provider that sends a machine-readable `code` is handled by
/// [`classify_status`] before this is ever consulted; this is the fallback for
/// the ones that only say it in prose, and every entry here is a phrasing some
/// provider actually emits. Deliberately broad: a false positive costs one
/// degraded retry, a false negative costs the whole turn.
const OVERFLOW_NEEDLES: &[&str] = &[
    "context length",
    "context_length_exceeded",
    "maximum context",
    "context window",
    "prompt is too long",
    "too many tokens",
    "reduce the length of the messages",
    "input length and `max_tokens` exceed",
    "string too long",
    "exceeds the maximum length",
    "request too large",
];

/// Phrasings that contain an overflow needle but mean something else. Without
/// these a rate-limit message that happens to quote a token budget ("request
/// too large for gpt-4 in organization … tokens per min") would be mistaken for
/// an overflow and the turn would throw away context it still needed.
const NOT_OVERFLOW_NEEDLES: &[&str] = &[
    "rate limit",
    "tokens per min",
    "tokens per day",
    "quota",
    "too many requests",
];

/// Whether `message` reads as a context-window overflow.
pub fn looks_like_context_overflow(message: &str) -> bool {
    let text = message.to_lowercase();
    if NOT_OVERFLOW_NEEDLES.iter().any(|n| text.contains(n)) {
        return false;
    }
    OVERFLOW_NEEDLES.iter().any(|n| text.contains(n))
}

/// Read a server-supplied retry delay out of response headers.
///
/// Three spellings, most precise first: `retry-after-ms` (milliseconds, what
/// OpenAI and Anthropic actually send under load), `retry-after` as integer
/// seconds, and `retry-after` as an HTTP date. The HTTP-date form is converted
/// to a delay from *now*, and a date already in the past yields zero rather than
/// an error — the limit has cleared.
pub fn retry_after_from_headers(header: impl Fn(&str) -> Option<String>) -> Option<Duration> {
    if let Some(ms) = header("retry-after-ms").and_then(|v| v.trim().parse::<f64>().ok())
        && ms.is_finite()
        && ms >= 0.0
    {
        return Some(Duration::from_millis(ms as u64));
    }
    let raw = header("retry-after")?;
    let raw = raw.trim();
    if let Ok(secs) = raw.parse::<f64>()
        && secs.is_finite()
        && secs >= 0.0
    {
        return Some(Duration::from_secs_f64(secs));
    }
    // HTTP-date form (RFC 7231 §7.1.3).
    let when = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let delta = when.timestamp() - chrono::Utc::now().timestamp();
    Some(Duration::from_secs(delta.max(0) as u64))
}

/// Read a retry delay out of a provider's error *message* — the "Please try
/// again in 1.5s" that OpenAI puts in the body of a rate-limit error without
/// setting a `Retry-After` header.
///
/// Hand-parsed rather than a regex: komo carries no regex dependency for this,
/// and the pattern is one number and one unit.
pub fn retry_after_from_message(message: &str) -> Option<Duration> {
    let text = message.to_lowercase();
    let at = text.find("try again in")? + "try again in".len();
    let rest = text[at..].trim_start();
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = digits.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let unit = rest[digits.len()..].trim_start();
    if unit.starts_with("ms") {
        Some(Duration::from_millis(value as u64))
    } else if unit.starts_with('s') || unit.starts_with("second") {
        Some(Duration::from_secs_f64(value))
    } else if unit.starts_with('m') || unit.starts_with("minute") {
        Some(Duration::from_secs_f64(value * 60.0))
    } else {
        None
    }
}

/// Classify a `reqwest` transport failure. Every one of these means the request
/// did not complete, so they are all retryable — the distinction between
/// "never left" and "may have landed" that `tools::http` draws does not matter
/// here, because a completion has no side effect to double-apply.
pub fn transport_error(e: &reqwest::Error, context: &str) -> LlmError {
    LlmError::transport(format!("{context}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_providers_error_code_beats_its_status_code() {
        // The case that motivates reading the code at all: a 400 that is really
        // an overflow, which must degrade-and-retry rather than fail the turn.
        assert_eq!(
            classify_status(400, Some("context_length_exceeded"), "whatever"),
            LlmErrorKind::ContextOverflow
        );
        assert_eq!(
            classify_status(200, Some("rate_limit_exceeded"), ""),
            LlmErrorKind::RateLimited
        );
    }

    #[test]
    fn statuses_map_to_kinds() {
        for (status, kind) in [
            (401, LlmErrorKind::Auth),
            (403, LlmErrorKind::Auth),
            (429, LlmErrorKind::RateLimited),
            (500, LlmErrorKind::Overloaded),
            (503, LlmErrorKind::Overloaded),
            (529, LlmErrorKind::Overloaded),
            (413, LlmErrorKind::ContextOverflow),
            (404, LlmErrorKind::InvalidRequest),
        ] {
            assert_eq!(classify_status(status, None, ""), kind, "status {status}");
        }
    }

    #[test]
    fn only_recoverable_kinds_are_retried() {
        let retry = |kind| LlmError::new(kind, "x").is_retryable();
        assert!(retry(LlmErrorKind::RateLimited));
        assert!(retry(LlmErrorKind::Overloaded));
        assert!(retry(LlmErrorKind::Transport));
        assert!(retry(LlmErrorKind::Stream));
        // An overflow is recoverable, but not by re-sending the same request —
        // the driver has to shrink the history first, so the retry layer must
        // leave it alone.
        assert!(!retry(LlmErrorKind::ContextOverflow));
        assert!(!retry(LlmErrorKind::Auth));
        assert!(!retry(LlmErrorKind::InvalidRequest));
    }

    #[test]
    fn context_overflow_is_recognised_across_provider_phrasings() {
        for message in [
            "This model's maximum context length is 128000 tokens",
            "error code: context_length_exceeded",
            "prompt is too long: 210000 tokens > 200000 maximum",
            "Please reduce the length of the messages",
            "input length and `max_tokens` exceed context limit",
        ] {
            assert!(
                looks_like_context_overflow(message),
                "should read as overflow: {message}"
            );
        }
        for message in [
            "invalid api key",
            "HTTP 429 Too Many Requests",
            "timeout",
            // The trap the exclusion list exists for: a rate-limit message that
            // quotes a token budget.
            "Request too large for gpt-4: limit 10000 tokens per min",
        ] {
            assert!(
                !looks_like_context_overflow(message),
                "should not read as overflow: {message}"
            );
        }
    }

    #[test]
    fn retry_after_prefers_the_millisecond_header() {
        let headers = |name: &str| match name {
            "retry-after-ms" => Some("1500".to_string()),
            "retry-after" => Some("60".to_string()),
            _ => None,
        };
        assert_eq!(
            retry_after_from_headers(headers),
            Some(Duration::from_millis(1500)),
            "the precise header wins over the coarse one"
        );
    }

    #[test]
    fn retry_after_reads_seconds_and_http_dates() {
        let secs = |name: &str| (name == "retry-after").then(|| "30".to_string());
        assert_eq!(
            retry_after_from_headers(secs),
            Some(Duration::from_secs(30))
        );

        // A date in the past means the limit already cleared: zero, not an error.
        let past = |name: &str| {
            (name == "retry-after").then(|| "Wed, 21 Oct 2015 07:28:00 GMT".to_string())
        };
        assert_eq!(retry_after_from_headers(past), Some(Duration::ZERO));

        let none = |_: &str| None;
        assert_eq!(retry_after_from_headers(none), None);
    }

    #[test]
    fn retry_after_falls_back_to_the_error_message() {
        assert_eq!(
            retry_after_from_message("Rate limit reached. Please try again in 1.5s"),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            retry_after_from_message("please try again in 200ms"),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            retry_after_from_message("try again in 2 minutes"),
            Some(Duration::from_secs(120))
        );
        assert_eq!(retry_after_from_message("rate limited, sorry"), None);
    }

    #[test]
    fn an_absurd_retry_after_is_capped() {
        let error = LlmError::new(LlmErrorKind::RateLimited, "x")
            .with_retry_after(Some(Duration::from_secs(3600)));
        assert_eq!(error.retry_after, Some(MAX_RETRY_AFTER));
    }
}
