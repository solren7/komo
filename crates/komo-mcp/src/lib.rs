//! A generic MCP client: connect to a Model Context Protocol server over
//! Streamable HTTP, enumerate its tools, and call them.
//!
//! Its own crate for the reason `komo-provider` is: it references nothing else
//! in komo, so it compiles in parallel and an edit here never rebuilds the
//! agent. Nothing in this crate knows about `Tool`, `ToolContext`, or any
//! particular server — the adapter that turns an [`McpToolDef`] into a komo
//! tool lives in `komo-tools`, and the servers themselves are config.
//!
//! Transport is Streamable HTTP only. stdio servers would drag in child-process
//! lifecycle (spawn, reap, restart-on-crash), which is a separate concern from
//! speaking the protocol; add it when a server actually needs it.

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientInfo, ContentBlock, Implementation,
};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, ServiceExt};

/// How komo identifies itself in the MCP `initialize` handshake.
const CLIENT_NAME: &str = "komo";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ceiling on one HTTP round-trip to an MCP server. The tool layer applies its
/// own per-call timeout on top; this one exists so a server that accepts the
/// connection and then goes silent cannot hold the socket forever.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// A failure talking to an MCP server, split by whether a retry could plausibly
/// succeed *and* is safe.
///
/// The distinction matters because the caller feeds it into komo's retry
/// classifier: a server that answered with a protocol-level error will answer
/// the same way next time, while a dropped connection may not have reached the
/// server at all.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// The transport failed (connection, TLS, timeout, malformed stream). The
    /// request may or may not have landed server-side.
    #[error("mcp transport error talking to `{server}`: {source}")]
    Transport {
        server: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The server answered, and the answer was an error. Retrying re-sends the
    /// same request to a server that already rejected it.
    #[error("mcp server `{server}` rejected the request: {message}")]
    Protocol { server: String, message: String },
}

impl McpError {
    /// Whether a retry could plausibly succeed. `true` only for transport
    /// failures — and even then the caller must treat the side effect as
    /// *ambiguous*, since a tool call that timed out may already have applied.
    pub fn retryable(&self) -> bool {
        matches!(self, McpError::Transport { .. })
    }
}

/// One tool as the server declared it in `tools/list`.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    /// The name on the wire — what `tools/call` expects. Not namespaced; the
    /// caller is responsible for avoiding collisions with its own catalog.
    pub name: String,
    pub description: Option<String>,
    /// The JSON Schema for this tool's arguments, verbatim from the server.
    pub input_schema: serde_json::Value,
}

/// What a `tools/call` produced, flattened into the shape a tool result needs.
#[derive(Debug, Clone, Default)]
pub struct McpCallResult {
    /// Every text block the server returned, joined by blank lines.
    pub text: String,
    /// The server's structured output when it sent one, else the full content
    /// array — so a non-text block is still recoverable from the ledger even
    /// though [`text`](Self::text) cannot carry it.
    pub structured: serde_json::Value,
    /// The server's `isError` flag: a *tool-level* failure (bad arguments, a
    /// remote 404) rather than a protocol failure. The call itself succeeded.
    pub is_error: bool,
    /// Content kinds dropped from `text` because they aren't text, counted by
    /// kind — so the caller can say what was lost instead of silently eliding it.
    pub dropped: BTreeMap<&'static str, usize>,
}

/// A live connection to one MCP server.
///
/// Constructed once at wiring time and shared (`Arc`) by every tool it backs:
/// the underlying service owns a session with the server, and one session per
/// tool would multiply handshakes for no benefit.
pub struct McpClient {
    /// The operator's name for this server (the config table key), used in
    /// errors and to namespace tool names.
    server: String,
    service: RunningService<RoleClient, ClientInfo>,
}

impl McpClient {
    /// Connect to `url` and complete the MCP `initialize` handshake.
    ///
    /// `token`, when present, is sent as `Authorization: Bearer <token>` on
    /// every request — the scheme memos, GitHub, and the other hosted servers
    /// use. Servers needing OAuth are out of scope: rmcp's `auth` feature can
    /// do it, but it wants an interactive redirect komo has nowhere to put.
    ///
    /// The token is passed **bare**: rmcp's reqwest backend calls
    /// `bearer_auth`, which prepends the scheme itself. Passing `"Bearer …"`
    /// here yields `Authorization: Bearer Bearer …` and a 401 that looks like a
    /// bad credential.
    pub async fn connect(server: &str, url: &str, token: Option<&str>) -> Result<Self, McpError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| McpError::Transport {
                server: server.to_string(),
                source: Box::new(e),
            })?;
        let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_string())
            // A long-lived gateway outlives a server's session window; without
            // this, the first call after an expiry fails instead of re-handshaking.
            .reinit_on_expired_session(true);
        if let Some(token) = token {
            config = config.auth_header(token.to_string());
        }
        let transport = StreamableHttpClientTransport::with_client(http, config);
        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new(CLIENT_NAME, CLIENT_VERSION),
        );
        let service = client_info
            .serve(transport)
            .await
            .map_err(|e| McpError::Transport {
                server: server.to_string(),
                source: Box::new(e),
            })?;
        Ok(Self {
            server: server.to_string(),
            service,
        })
    }

    /// The operator's name for this server.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The server's advertised name and version from the `initialize` response,
    /// for the startup log — so an operator can see *what* answered, not just
    /// that something did.
    pub fn peer_description(&self) -> Option<String> {
        // Optional on the wire: a discovery response need not identify itself.
        let info = self.service.peer_info()?.server_info.clone()?;
        Some(format!("{} {}", info.name, info.version))
    }

    /// Enumerate every tool the server offers.
    ///
    /// Paginates to exhaustion: `tools/list` is cursor-based, and a server with
    /// more tools than one page would otherwise be silently truncated — which
    /// looks identical to "that tool doesn't exist".
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, McpError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|e| self.service_error(e))?;
        Ok(tools
            .into_iter()
            .map(|t| McpToolDef {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()),
                input_schema: serde_json::Value::Object((*t.input_schema).clone()),
            })
            .collect())
    }

    /// Invoke `tool` with `args` (a JSON object; anything else is sent as no
    /// arguments, which is what a server sees for a tool taking none).
    pub async fn call(
        &self,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpError> {
        let mut params = CallToolRequestParams::new(tool.to_string());
        if let serde_json::Value::Object(map) = args {
            params = params.with_arguments(map);
        }
        let result = self
            .service
            .call_tool(params)
            .await
            .map_err(|e| self.service_error(e))?;

        let mut texts = Vec::new();
        let mut dropped: BTreeMap<&'static str, usize> = BTreeMap::new();
        for item in &result.content {
            match item {
                ContentBlock::Text(t) => texts.push(t.text.clone()),
                ContentBlock::Image(_) => *dropped.entry("image").or_default() += 1,
                ContentBlock::Audio(_) => *dropped.entry("audio").or_default() += 1,
                ContentBlock::Resource(_) => *dropped.entry("resource").or_default() += 1,
                ContentBlock::ResourceLink(_) => *dropped.entry("resource_link").or_default() += 1,
                // `ContentBlock` is #[non_exhaustive]: a future block kind must
                // be counted as dropped, not silently vanish from the result.
                _ => *dropped.entry("unknown").or_default() += 1,
            }
        }
        let structured = match &result.structured_content {
            Some(value) => value.clone(),
            None => serde_json::to_value(&result.content).unwrap_or(serde_json::Value::Null),
        };
        Ok(McpCallResult {
            text: texts.join("\n\n"),
            structured,
            is_error: result.is_error.unwrap_or(false),
            dropped,
        })
    }

    /// Classify an rmcp service error into [`McpError`].
    ///
    /// `ServiceError::McpError` is the server's own error response — it
    /// answered, so this is terminal. Everything else (transport, timeout,
    /// cancellation, a closed sink) could not establish what the server saw.
    fn service_error(&self, error: rmcp::ServiceError) -> McpError {
        match error {
            rmcp::ServiceError::McpError(e) => McpError::Protocol {
                server: self.server.clone(),
                message: e.to_string(),
            },
            other => McpError::Transport {
                server: self.server.clone(),
                source: Box::new(other),
            },
        }
    }
}

/// Connect to several servers concurrently, keeping the ones that answered.
///
/// A server that is down must not stop komo from booting — the same call komo
/// makes for a missing model key or a token-less HA channel. The failure is
/// logged and that server's tools are simply absent for the process lifetime,
/// which is honest: the catalog is fixed at wiring time anyway, so there is no
/// later moment at which they could appear.
pub async fn connect_all(servers: Vec<(String, String, Option<String>)>) -> Vec<Arc<McpClient>> {
    let attempts = servers.into_iter().map(|(name, url, token)| async move {
        match McpClient::connect(&name, &url, token.as_deref()).await {
            Ok(client) => {
                tracing::info!(
                    server = %name,
                    peer = client.peer_description().unwrap_or_else(|| "unknown".into()),
                    "mcp server connected"
                );
                Some(Arc::new(client))
            }
            Err(error) => {
                tracing::warn!(server = %name, %error, "mcp server unreachable — its tools are unavailable");
                None
            }
        }
    });
    futures_util::future::join_all(attempts)
        .await
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port nothing is listening on: bind one, read it back, then drop the
    /// listener. Racier alternatives (a hardcoded high port) fail on whatever
    /// machine happens to be running something there.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a scratch port");
        listener.local_addr().expect("read scratch port").port()
    }

    #[tokio::test]
    async fn an_unreachable_server_fails_as_a_retryable_transport_error() {
        let url = format!("http://127.0.0.1:{}/mcp", closed_port());
        let error = McpClient::connect("down", &url, None)
            .await
            .err()
            .expect("connecting to a closed port must not succeed");
        assert!(
            matches!(error, McpError::Transport { .. }),
            "a refused connection is a transport failure, not a protocol one: {error}"
        );
        assert!(error.retryable());
        assert!(error.to_string().contains("down"), "{error}");
    }

    #[tokio::test]
    async fn connect_all_drops_the_servers_that_did_not_answer() {
        // The boot-time contract: an unreachable server costs its own tools and
        // nothing else. `connect_all` must never propagate the failure.
        let url = format!("http://127.0.0.1:{}/mcp", closed_port());
        let clients = connect_all(vec![
            ("down".to_string(), url.clone(), None),
            ("also-down".to_string(), url, Some("token".to_string())),
        ])
        .await;
        assert!(clients.is_empty());
    }
}
