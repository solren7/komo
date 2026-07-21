//! komo desktop client (Dioxus).
//!
//! A pure HTTP front end over the gateway's api channel — the GUI analog of the
//! chat TUI and the CLI's `GatewayClient`. It auto-discovers a running gateway
//! via `~/.komo/gateway.json`, then offers chat (with interactive tool approval
//! and clarify) plus a read/write dashboard over the same `/api/*` endpoints.

mod api;
mod chat;
mod dashboard;
mod md;

use std::time::Duration;

use dioxus::prelude::*;

use api::ApiClient;

/// Inlined so the app is a single `cargo run` with no `dx` asset pipeline.
const CSS: &str = include_str!("assets/style.css");

/// The gateway connection, shared through context. Rebuilt by the lifecycle
/// task whenever the gateway comes or goes.
#[derive(Clone)]
pub enum ConnState {
    Connecting,
    Online(ApiClient),
    Offline(String),
}

impl ConnState {
    /// The live client, if connected. Views read this to issue requests.
    pub fn client(&self) -> Option<ApiClient> {
        match self {
            ConnState::Online(c) => Some(c.clone()),
            _ => None,
        }
    }
}

/// Which top-level view is showing (navigation is a plain signal switch — two
/// views don't warrant a router).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Chat,
    Dashboard,
}

/// A fresh chat session id. The GUI drives the api channel, which stores the
/// session under `api:{header}`, so the full id it lists/loads by is
/// `api:gui-{uuid}`; the chat send strips the `api:` prefix back off for the
/// `X-Komo-Session-Id` header (see `chat::header_for`).
pub fn new_session_id() -> String {
    format!("api:gui-{}", uuid::Uuid::now_v7())
}

fn main() {
    // Desktop routes tracing to stderr; a webview log line can't corrupt it the
    // way it would the TUI's alternate screen.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("KOMO_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let conn = use_signal(|| ConnState::Connecting);
    use_context_provider(|| conn);
    let view = use_signal(|| View::Chat);
    use_context_provider(|| view);
    // The active chat session (full id). Shared so the dashboard's Sessions tab
    // can hand a past session to the chat view ("continue in chat").
    let session = use_signal(new_session_id);
    use_context_provider(|| session);

    // Connection lifecycle: discover + probe, then poll health while online and
    // retry while offline, so the GUI attaches when the gateway starts and
    // detaches when it stops — no restart needed either way.
    use_future(move || async move {
        let mut conn = conn;
        loop {
            let snapshot = conn.read().clone();
            match snapshot {
                ConnState::Online(client) => {
                    if !client.is_healthy().await {
                        conn.set(ConnState::Offline("gateway 无响应，正在重连…".to_string()));
                        continue;
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                _ => match ApiClient::connect().await {
                    Some(client) => conn.set(ConnState::Online(client)),
                    None => {
                        conn.set(ConnState::Offline(
                            "未发现运行中的 komo gateway（启动 `komo gateway` 后自动连接）"
                                .to_string(),
                        ));
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                },
            }
        }
    });

    rsx! {
        style { dangerous_inner_html: CSS }
        div { class: "app",
            NavBar {}
            ConnectionBanner {}
            main { class: "content",
                match view() {
                    View::Chat => rsx! { chat::ChatView {} },
                    View::Dashboard => rsx! { dashboard::Dashboard {} },
                }
            }
        }
    }
}

#[component]
fn NavBar() -> Element {
    let mut view = use_context::<Signal<View>>();
    let conn = use_context::<Signal<ConnState>>();
    let online = matches!(&*conn.read(), ConnState::Online(_));
    let dot = if online { "dot online" } else { "dot offline" };

    rsx! {
        nav { class: "navbar",
            span { class: "brand", "komo" }
            div { class: "tabs",
                button {
                    class: if view() == View::Chat { "tab active" } else { "tab" },
                    onclick: move |_| view.set(View::Chat),
                    "聊天"
                }
                button {
                    class: if view() == View::Dashboard { "tab active" } else { "tab" },
                    onclick: move |_| view.set(View::Dashboard),
                    "仪表盘"
                }
            }
            span { class: "{dot}", title: if online { "已连接" } else { "未连接" } }
        }
    }
}

/// A thin strip shown only when the gateway is not reachable.
#[component]
fn ConnectionBanner() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let message = match &*conn.read() {
        ConnState::Online(_) => None,
        ConnState::Connecting => Some("正在连接 komo gateway…".to_string()),
        ConnState::Offline(msg) => Some(msg.clone()),
    };
    match message {
        Some(msg) => rsx! { div { class: "banner", "{msg}" } },
        None => rsx! {},
    }
}
