//! The HTTP boundary: send a completion request, classify what comes back, and
//! hand the caller a stream of SSE frames.
//!
//! This is the layer that exists so [`super::error::LlmError`] can be built
//! while the status code, the response headers, and the provider's error body
//! are all still in hand. Above it, no code sniffs strings to decide whether to
//! retry.
//!
//! Every request is streamed. Not a preference: the Codex backend rejects
//! non-streamed requests outright, and a single transport means the idle-timeout
//! and terminal-event rules below hold for every provider instead of only the
//! one that forced them.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;

use super::error::{
    LlmError, LlmErrorKind, classify_status, retry_after_from_headers, retry_after_from_message,
    transport_error,
};

/// How long a stream may produce nothing before it is declared stalled.
///
/// A provider that stops mid-stream without closing the connection would
/// otherwise hold the turn open until the whole-round timeout — and report
/// nothing about why. This turns that into a retryable [`LlmErrorKind::Stream`]
/// quickly enough that the retry has time to succeed inside the round budget.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Resolves a bearer token per request.
///
/// Exists for Codex, whose OAuth access token rotates hourly: a long-running
/// gateway must authenticate with a freshly resolved token on every request
/// rather than one captured at construction. Static-key providers use
/// [`Auth::ApiKey`] / [`Auth::Bearer`] and never touch this.
#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn token(&self) -> anyhow::Result<String>;
}

/// How a request authenticates.
#[derive(Clone)]
pub enum Auth {
    /// `Authorization: Bearer <key>` — OpenAI, DeepSeek, OpenRouter.
    Bearer(String),
    /// `x-api-key: <key>` — Anthropic.
    ApiKey(String),
    /// Resolved fresh per request (Codex).
    Dynamic(Arc<dyn TokenSource>),
}

/// Everything about *where* and *how* to reach one provider, resolved once at
/// startup. Pure data: a provider komo has never heard of is a base URL, an auth
/// mode, and a header map — not new code.
pub struct Endpoint {
    /// Full URL of the completion endpoint.
    pub url: String,
    pub auth: Auth,
    /// Static headers sent on every request (Anthropic's `anthropic-version`,
    /// Codex's Cloudflare-appeasing `originator`, …).
    pub headers: Vec<(String, String)>,
    pub client: reqwest::Client,
}

impl Endpoint {
    /// Apply auth + static headers to a request builder.
    async fn authorize(
        &self,
        mut req: reqwest::RequestBuilder,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        req = match &self.auth {
            Auth::Bearer(key) => req.bearer_auth(key),
            Auth::ApiKey(key) => req.header("x-api-key", key),
            Auth::Dynamic(source) => req.bearer_auth(source.token().await?),
        };
        for (name, value) in &self.headers {
            req = req.header(name.as_str(), value.as_str());
        }
        Ok(req)
    }

    /// POST `body` and return the SSE frames the provider streams back.
    ///
    /// A non-2xx response is consumed here and turned into a classified
    /// [`LlmError`]: the status, the body's error `code`/`message`, and any
    /// `Retry-After` all feed the classification, so callers above never see an
    /// HTTP status again.
    pub async fn stream(&self, body: &Value) -> Result<SseStream, LlmError> {
        let req = self
            .client
            .post(&self.url)
            .header("accept", "text/event-stream")
            .json(body);
        let req = self
            .authorize(req)
            .await
            // A credential that cannot be resolved is an auth failure, not a
            // transport one — retrying an expired refresh token is pointless.
            .map_err(|e| LlmError::new(LlmErrorKind::Auth, format!("{e:#}")))?;

        let response = req
            .send()
            .await
            .map_err(|e| transport_error(&e, "LLM request failed"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(self.error_from_response(response).await);
        }
        Ok(SseStream::new(response))
    }

    /// Turn a non-2xx response into a classified error, reading the body for the
    /// provider's own `code` and `message`.
    async fn error_from_response(&self, response: reqwest::Response) -> LlmError {
        let status = response.status().as_u16();
        // Headers must be read before the body consumes the response.
        let headers = response.headers().clone();
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let body = response.text().await.unwrap_or_default();
        let (code, message) = error_fields(&body);
        let message = if message.is_empty() {
            // Nothing parseable (an HTML gateway page, an empty body): the raw
            // text is still the most informative thing we have.
            truncate(&body, 500)
        } else {
            message
        };
        let retry_after =
            retry_after_from_headers(header).or_else(|| retry_after_from_message(&message));
        LlmError::new(classify_status(status, code.as_deref(), &message), message)
            .with_status(status)
            .with_code(code)
            .with_retry_after(retry_after)
    }
}

/// Pull `error.code` and `error.message` out of a provider error body.
///
/// Shapes seen in the wild: `{"error":{"code":…,"message":…}}` (OpenAI,
/// DeepSeek), `{"error":{"type":…,"message":…}}` (Anthropic), and a bare
/// `{"message":…}`. Anything else yields empties and the caller falls back to
/// the raw body.
fn error_fields(body: &str) -> (Option<String>, String) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return (None, String::new());
    };
    let error = value.get("error").unwrap_or(&value);
    let code = error
        .get("code")
        .or_else(|| error.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (code, message)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// A server-sent-event stream, decoded into the JSON payload of each frame.
///
/// SSE framing only: what the frames *mean* is the wire codec's business (see
/// `super::responses`). Frames without a `data:` line, comments, and the
/// `[DONE]` sentinel are dropped here so codecs only ever see real payloads.
pub struct SseStream {
    inner: Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin>,
    /// Bytes received but not yet split into complete lines.
    buffer: String,
    /// `data:` lines of the frame currently being accumulated.
    data: Vec<String>,
    /// Frames already decoded from the current chunk, oldest first.
    ready: std::collections::VecDeque<Value>,
    done: bool,
}

impl SseStream {
    fn new(response: reqwest::Response) -> Self {
        Self {
            inner: Box::new(response.bytes_stream()),
            buffer: String::new(),
            data: Vec::new(),
            ready: std::collections::VecDeque::new(),
            done: false,
        }
    }

    /// A stream over a canned SSE body, for codec tests: what a codec folds out
    /// of a sequence of frames (which one ends the round, which id it keeps) is
    /// only observable through a real stream.
    #[cfg(test)]
    pub(super) fn from_body(body: &str) -> Self {
        Self {
            inner: Box::new(futures_util::stream::iter(vec![Ok(bytes::Bytes::from(
                body.to_string(),
            ))])),
            buffer: String::new(),
            data: Vec::new(),
            ready: std::collections::VecDeque::new(),
            done: false,
        }
    }

    /// The next frame's payload, or `None` once the stream ends.
    ///
    /// Ending is *not* success on its own: a stream that stops before its
    /// codec saw a terminal event is a failed round, which the codec enforces —
    /// see `super::responses::collect`.
    pub async fn next(&mut self) -> Result<Option<Value>, LlmError> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Ok(Some(frame));
            }
            if self.done {
                return Ok(None);
            }
            let chunk = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, self.inner.next()).await {
                Ok(Some(Ok(chunk))) => chunk,
                Ok(Some(Err(e))) => return Err(transport_error(&e, "LLM stream failed")),
                Ok(None) => {
                    self.done = true;
                    // A frame left mid-accumulation (no trailing blank line)
                    // is still a frame.
                    self.flush_frame();
                    continue;
                }
                Err(_) => {
                    return Err(LlmError::stream(format!(
                        "LLM stream stalled for {}s with no data",
                        STREAM_IDLE_TIMEOUT.as_secs()
                    )));
                }
            };
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
            self.decode_buffered_lines();
        }
    }

    /// Split whatever complete lines the buffer holds into frames.
    fn decode_buffered_lines(&mut self) {
        while let Some(at) = self.buffer.find('\n') {
            let line = self.buffer[..at].trim_end_matches('\r').to_string();
            self.buffer.drain(..=at);
            if line.is_empty() {
                // Blank line terminates a frame.
                self.flush_frame();
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data.push(rest.trim_start().to_string());
            }
            // `event:` / `id:` / `retry:` / comments are ignored: the Responses
            // and Messages payloads both name their own type inside `data`.
        }
    }

    /// Parse the accumulated `data:` lines as one frame and queue it.
    fn flush_frame(&mut self) {
        if self.data.is_empty() {
            return;
        }
        let payload = self.data.join("\n");
        self.data.clear();
        if payload == "[DONE]" {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&payload) {
            self.ready.push_back(value);
        } else {
            tracing::debug!(payload = %truncate(&payload, 200), "unparseable SSE frame ignored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_fields_read_every_shape_providers_send() {
        // OpenAI / DeepSeek
        let (code, message) = error_fields(
            r#"{"error":{"code":"rate_limit_exceeded","message":"slow down please"}}"#,
        );
        assert_eq!(code.as_deref(), Some("rate_limit_exceeded"));
        assert_eq!(message, "slow down please");

        // Anthropic names the code `type`.
        let (code, message) =
            error_fields(r#"{"error":{"type":"overloaded_error","message":"Overloaded"}}"#);
        assert_eq!(code.as_deref(), Some("overloaded_error"));
        assert_eq!(message, "Overloaded");

        // A bare object, and a non-JSON body (an HTML gateway page).
        let (_, message) = error_fields(r#"{"message":"nope"}"#);
        assert_eq!(message, "nope");
        let (code, message) = error_fields("<html>502 Bad Gateway</html>");
        assert!(code.is_none() && message.is_empty());
    }

    /// The framing rules that matter: `data:` accumulates, a blank line ends a
    /// frame, `[DONE]` and comments are not frames, and a frame split across
    /// two network chunks still arrives whole.
    #[test]
    fn sse_framing_handles_split_chunks_and_sentinels() {
        let mut stream = SseStream {
            inner: Box::new(futures_util::stream::empty()),
            buffer: String::new(),
            data: Vec::new(),
            ready: Default::default(),
            done: true,
        };

        stream.buffer.push_str("event: response.created\ndata: {\"type\":\"a\"}\n\n: comment\ndata: [DONE]\n\ndata: {\"ty");
        stream.decode_buffered_lines();
        assert_eq!(
            stream.ready.len(),
            1,
            "only the complete non-sentinel frame is ready"
        );
        assert_eq!(stream.ready[0]["type"], "a");

        // The rest of the split frame arrives.
        stream.buffer.push_str("pe\":\"b\"}\n\n");
        stream.decode_buffered_lines();
        assert_eq!(stream.ready.len(), 2);
        assert_eq!(stream.ready[1]["type"], "b");
    }

    #[test]
    fn a_frame_without_a_trailing_blank_line_still_counts() {
        // Providers do close the connection right after the last frame.
        let mut stream = SseStream {
            inner: Box::new(futures_util::stream::empty()),
            buffer: String::new(),
            data: Vec::new(),
            ready: Default::default(),
            done: true,
        };
        stream.buffer.push_str("data: {\"type\":\"last\"}\n");
        stream.decode_buffered_lines();
        assert!(stream.ready.is_empty(), "no blank line yet");
        stream.flush_frame();
        assert_eq!(stream.ready.len(), 1);
    }

    #[test]
    fn truncate_cuts_on_char_boundaries() {
        let text = "前".repeat(100);
        let cut = truncate(&text, 10);
        assert!(cut.starts_with('前') && cut.ends_with('…'));
        assert!(cut.len() <= 14);
    }
}
