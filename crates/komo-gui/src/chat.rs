//! Chat view: message list, composer, interactive tool-approval modal, and
//! inline clarify answering — the GUI analog of the chat TUI.
//!
//! A turn runs on a spawned task (the UI thread only flips signals, so a 300s+
//! turn never blocks rendering). While it's in flight a second task polls
//! `interactions(session)` ~1s; a pending approval raises the modal, a pending
//! clarify question re-enables the composer as an answer box. Both resolve
//! out-of-band over the api channel — the GUI's equivalent of the TUI's y/s/n
//! modal and mid-turn answer, since HTTP has no reply sink to prompt on.

use std::time::Duration;

use dioxus::prelude::*;

use komo_core::domain::message::Role;

use crate::api::{ChatMode, PendingApproval};
use crate::md;
use crate::{ConnState, new_session_id};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MsgKind {
    User,
    Assistant,
    Error,
}

#[derive(Clone, PartialEq)]
struct ChatMsg {
    kind: MsgKind,
    content: String,
}

/// The `X-Komo-Session-Id` header for a full session id: the server prepends
/// `api:` when resolving, so a GUI session `api:gui-x` is continued by sending
/// `gui-x`. A non-api id (a telegram/feishu session opened from the dashboard)
/// has no such round-trip, so it is sent whole — the server forks a fresh
/// `api:` session rather than truly resuming it (a known v1 limitation).
fn header_for(full: &str) -> String {
    full.strip_prefix("api:").unwrap_or(full).to_string()
}

#[component]
pub fn ChatView() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut session = use_context::<Signal<String>>();

    let mut messages = use_signal(Vec::<ChatMsg>::new);
    let mut input = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut mode = use_signal(|| ChatMode::Interactive);
    let mut pending_approval = use_signal(|| None::<PendingApproval>);
    let mut pending_question = use_signal(|| None::<String>);

    // Load the transcript whenever the active session changes (a fresh gui
    // session simply comes back empty). `peek` on conn so this doesn't re-run on
    // every health poll — only a session switch reloads history.
    use_effect(move || {
        let sess = session();
        let client = conn.peek().client();
        spawn(async move {
            let Some(client) = client else { return };
            if let Ok(msgs) = client.session_messages(&sess).await {
                let mapped = msgs
                    .into_iter()
                    .filter(|m| matches!(m.role, Role::User | Role::Assistant))
                    .map(|m| ChatMsg {
                        kind: match m.role {
                            Role::User => MsgKind::User,
                            _ => MsgKind::Assistant,
                        },
                        content: m.content,
                    })
                    .collect::<Vec<_>>();
                messages.set(mapped);
            }
        });
    });

    // Start a normal turn: optimistically show the user message, then run the
    // turn + an interactions poll concurrently.
    let mut start_turn = move |text: String| {
        messages.write().push(ChatMsg {
            kind: MsgKind::User,
            content: text.clone(),
        });
        sending.set(true);
        pending_approval.set(None);
        pending_question.set(None);
        let Some(client) = conn.peek().client() else {
            messages.write().push(ChatMsg {
                kind: MsgKind::Error,
                content: "未连接到 gateway。".to_string(),
            });
            sending.set(false);
            return;
        };
        let sess = session();
        let header = header_for(&sess);
        let turn_mode = mode();

        // Turn task.
        let turn_client = client.clone();
        let turn_header = header.clone();
        spawn(async move {
            let result = turn_client.chat(&turn_header, &text, turn_mode).await;
            match result {
                Ok(reply) => messages.write().push(ChatMsg {
                    kind: MsgKind::Assistant,
                    content: reply,
                }),
                Err(error) => messages.write().push(ChatMsg {
                    kind: MsgKind::Error,
                    content: format!("请求失败：{error}"),
                }),
            }
            sending.set(false);
        });

        // Interactions poll task: mirrors pending approval / clarify into signals
        // while the turn is in flight, and clears them when it ends.
        spawn(async move {
            while sending() {
                if let Ok(ix) = client.interactions(&sess).await {
                    pending_approval.set(ix.approval);
                    pending_question.set(ix.question);
                }
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
            pending_approval.set(None);
            pending_question.set(None);
        });
    };

    // Composer submit: answers a pending clarify question if one is waiting,
    // else starts a new turn.
    let mut submit = move || {
        let text = input().trim().to_string();
        if text.is_empty() {
            return;
        }
        if pending_question().is_some() {
            input.set(String::new());
            messages.write().push(ChatMsg {
                kind: MsgKind::User,
                content: text.clone(),
            });
            pending_question.set(None);
            if let Some(client) = conn.peek().client() {
                let sess = session();
                spawn(async move {
                    let _ = client.answer_question(&sess, &text).await;
                });
            }
            return;
        }
        if sending() {
            return;
        }
        input.set(String::new());
        start_turn(text);
    };

    // Composer is disabled mid-turn EXCEPT while a clarify question waits (then
    // the box is the answer field).
    let awaiting_answer = pending_question().is_some();
    let composer_disabled = sending() && !awaiting_answer;

    let decide = move |decision: &'static str| {
        pending_approval.set(None);
        if let Some(client) = conn.peek().client() {
            let sess = session();
            spawn(async move {
                let _ = client.resolve_approval(&sess, decision).await;
            });
        }
    };

    rsx! {
        div { class: "chat",
            // Session toolbar.
            div { class: "chat-toolbar",
                button {
                    class: "small",
                    onclick: move |_| {
                        messages.set(Vec::new());
                        session.set(new_session_id());
                    },
                    "新会话"
                }
                div { class: "mode",
                    label {
                        input {
                            r#type: "checkbox",
                            checked: mode() == ChatMode::Trusted,
                            onchange: move |e| {
                                mode.set(if e.checked() { ChatMode::Trusted } else { ChatMode::Interactive });
                            },
                        }
                        span {
                            title: "开启后副作用工具自动批准（等同 komo chat）；关闭则弹出审批",
                            " 信任模式（自动批准）"
                        }
                    }
                }
            }

            // Transcript.
            div { class: "messages",
                for (i, m) in messages().iter().enumerate() {
                    MessageBubble { key: "{i}", kind: m.kind, content: m.content.clone() }
                }
                if sending() && !awaiting_answer {
                    div { class: "typing", "komo 正在思考…" }
                }
            }

            // Clarify prompt (inline, above the composer).
            if let Some(q) = pending_question() {
                div { class: "clarify",
                    div { class: "clarify-q", "❓ {q}" }
                    div { class: "clarify-hint", "在下面输入你的回答并发送" }
                }
            }

            // Composer.
            div { class: "composer",
                textarea {
                    value: "{input}",
                    disabled: composer_disabled,
                    placeholder: if awaiting_answer { "输入你的回答…" } else { "给 komo 发消息…（Enter 发送，Shift+Enter 换行）" },
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            submit();
                        }
                    },
                }
                button {
                    class: "send",
                    disabled: composer_disabled,
                    onclick: move |_| submit(),
                    if awaiting_answer { "回答" } else { "发送" }
                }
            }
        }

        // Approval modal (interactive mode only reaches here).
        if let Some(req) = pending_approval() {
            ApprovalModal { req, decide }
        }
    }
}

#[component]
fn MessageBubble(kind: MsgKind, content: String) -> Element {
    match kind {
        MsgKind::User => rsx! {
            div { class: "bubble user", "{content}" }
        },
        MsgKind::Assistant => {
            let html = md::to_html(&content);
            rsx! {
                div { class: "bubble assistant md", dangerous_inner_html: "{html}" }
            }
        }
        MsgKind::Error => rsx! {
            div { class: "bubble error", "{content}" }
        },
    }
}

#[component]
fn ApprovalModal(req: PendingApproval, decide: EventHandler<&'static str>) -> Element {
    let dangerous = req.risk == "dangerous";
    let heading = if dangerous {
        "🛑 需要审批（危险操作）"
    } else {
        "⚠️ 需要审批"
    };
    rsx! {
        div { class: "modal-backdrop",
            div { class: if dangerous { "modal danger" } else { "modal" },
                div { class: "modal-title", "{heading}" }
                div { class: "modal-summary", "{req.summary}" }
                if let Some(detail) = req.detail.clone() {
                    div { class: "modal-detail", "{detail}" }
                }
                div { class: "modal-actions",
                    button { class: "btn ok", onclick: move |_| decide.call("once"), "批准本次" }
                    button { class: "btn", onclick: move |_| decide.call("session"), "批准本会话" }
                    button { class: "btn deny", onclick: move |_| decide.call("deny"), "拒绝" }
                }
            }
        }
    }
}
