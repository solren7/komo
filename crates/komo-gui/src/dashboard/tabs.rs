//! The individual dashboard tabs.
//!
//! Each tab captures the `ConnState` signal at its top (a hook), reads the
//! client with `conn.peek().client()` inside handlers/futures (no hook there),
//! and drives a `use_resource` off a `tick` signal that a mounted-only
//! `use_future` bumps every few seconds — so only the visible tab polls.

use std::time::Duration;

use dioxus::prelude::*;

use crate::ConnState;
use crate::View;

use super::fmt_ts;

/// Poll interval for the active tab.
const POLL: Duration = Duration::from_secs(6);

/// A small toolbar row with a Refresh button; calls `on_refresh` on click.
#[component]
fn RefreshBar(on_refresh: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div { class: "dash-toolbar",
            {children}
            button { class: "small", onclick: move |_| on_refresh.call(()), "刷新" }
        }
    }
}

// ---- Status ----------------------------------------------------------------

#[component]
pub fn StatusTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.status().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(s)) => rsx! {
                    div { class: "status-grid",
                        StatCard { label: "版本", value: s.version.clone() }
                        StatCard { label: "开放任务", value: s.open_tasks.to_string() }
                        StatCard { label: "会话数", value: s.sessions.to_string() }
                        StatCard { label: "Home", value: s.home_chat.clone().unwrap_or_else(|| "—".into()) }
                    }
                    div { class: "channels",
                        span { class: "muted", "渠道：" }
                        if s.channels.is_empty() {
                            span { "无" }
                        } else {
                            for ch in s.channels.iter() {
                                span { class: "chip", "{ch}" }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn StatCard(label: String, value: String) -> Element {
    rsx! {
        div { class: "stat-card",
            div { class: "stat-value", "{value}" }
            div { class: "stat-label", "{label}" }
        }
    }
}

// ---- Tasks -----------------------------------------------------------------

#[component]
pub fn TasksTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.tasks().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(tasks)) if tasks.is_empty() => rsx! { div { class: "empty", "没有开放任务。" } },
                Some(Ok(tasks)) => rsx! {
                    for t in tasks.iter() {
                        div { class: "row",
                            span { class: "tag", "{t.status:?}" }
                            span { class: "row-main", "{t.title}" }
                            if !t.board.is_empty() {
                                span { class: "chip", "#{t.board}" }
                            }
                            if let Some(due) = t.due_at {
                                span { class: "muted", "截止 {fmt_ts(due)}" }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ---- Runs ------------------------------------------------------------------

#[component]
pub fn RunsTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    let mut selected = use_signal(|| None::<String>);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.runs(50).await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(runs)) if runs.is_empty() => rsx! { div { class: "empty", "还没有运行记录。" } },
                Some(Ok(runs)) => rsx! {
                    for r in runs.iter() {
                        {
                            let id = r.id.clone();
                            let id_sel = id.clone();
                            rsx! {
                                div {
                                    class: "row clickable",
                                    onclick: move |_| {
                                        let cur = selected();
                                        selected.set(if cur.as_deref() == Some(id_sel.as_str()) { None } else { Some(id_sel.clone()) });
                                    },
                                    span { class: "tag", "{r.status:?}" }
                                    span { class: "row-main", "{r.input}" }
                                    if r.recoverable {
                                        span { class: "chip warn", title: "可恢复", "⟲" }
                                    }
                                    span { class: "muted", "{fmt_ts(r.started_at)}" }
                                }
                                if selected().as_deref() == Some(id.as_str()) {
                                    RunDetail { id: id.clone() }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn RunDetail(id: String) -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let id_for_fetch = id.clone();
    let detail = use_resource(move || {
        let client = conn.peek().client();
        let id = id_for_fetch.clone();
        async move {
            match client {
                Some(c) => c.run(&id).await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });
    let snapshot = detail.read().clone();
    rsx! {
        div { class: "run-detail",
            match snapshot {
                None => rsx! { div { class: "loading", "加载步骤…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(None)) => rsx! { div { class: "empty", "运行不存在。" } },
                Some(Ok(Some((run, steps)))) => rsx! {
                    if !run.final_output.is_empty() {
                        div { class: "run-output", "{run.final_output}" }
                    }
                    if !run.error.is_empty() {
                        div { class: "err", "{run.error}" }
                    }
                    for s in steps.iter() {
                        div { class: "step",
                            span { class: if s.ok { "tag ok" } else { "tag deny" }, "{s.seq}. {s.tool_name}" }
                            span { class: "step-args", "{s.args}" }
                            if !s.error.is_empty() {
                                span { class: "err", "{s.error}" }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ---- Memories --------------------------------------------------------------

const MEM_FILTERS: &[(&str, &str)] = &[
    ("", "全部"),
    ("candidate", "候选"),
    ("active", "活跃"),
    ("archived", "归档"),
    ("rejected", "拒绝"),
];

#[component]
pub fn MemoriesTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    let mut filter = use_signal(String::new);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let f = filter();
        let client = conn.peek().client();
        async move {
            let status = if f.is_empty() { None } else { Some(f.clone()) };
            match client {
                Some(c) => c
                    .memories(status.as_deref())
                    .await
                    .map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    // A memory governance write, then refresh.
    let act = move |id: String, action: &'static str| {
        let client = conn.peek().client();
        spawn(async move {
            if let Some(c) = client {
                let _ = c.memory_transition(&id, action).await;
            }
            tick += 1;
        });
    };

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1,
            select {
                class: "small",
                value: "{filter}",
                onchange: move |e| filter.set(e.value()),
                for (val, label) in MEM_FILTERS.iter().copied() {
                    option { value: "{val}", "{label}" }
                }
            }
        }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(mems)) if mems.is_empty() => rsx! { div { class: "empty", "没有记忆。" } },
                Some(Ok(mems)) => rsx! {
                    for m in mems.iter() {
                        {
                            let (id_p, id_r, id_pin) = (m.id.clone(), m.id.clone(), m.id.clone());
                            rsx! {
                                div { class: "row mem-row",
                                    div { class: "mem-head",
                                        span { class: "tag", "{m.status:?}" }
                                        span { class: "chip", "{m.kind:?}" }
                                        if m.pinned {
                                            span { class: "chip warn", "📌" }
                                        }
                                        span { class: "muted", "{m.confidence:?}" }
                                    }
                                    div { class: "mem-content", "{m.content}" }
                                    div { class: "mem-actions",
                                        button { class: "btn ok", onclick: move |_| act(id_p.clone(), "promote"), "promote" }
                                        button { class: "btn", onclick: move |_| act(id_pin.clone(), "pin"), "pin" }
                                        button { class: "btn deny", onclick: move |_| act(id_r.clone(), "reject"), "reject" }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ---- Sessions --------------------------------------------------------------

#[component]
pub fn SessionsTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut session = use_context::<Signal<String>>();
    let mut view = use_context::<Signal<View>>();
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.sessions().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(sessions)) if sessions.is_empty() => rsx! { div { class: "empty", "没有会话。" } },
                Some(Ok(sessions)) => rsx! {
                    for s in sessions.iter() {
                        {
                            let id = s.id.clone();
                            rsx! {
                                div { class: "row",
                                    span { class: "row-main mono", "{s.id}" }
                                    span { class: "muted", "{s.user_turns} 轮 · {s.messages} 条 · {fmt_ts(s.created_at)}" }
                                    button {
                                        class: "small",
                                        onclick: move |_| {
                                            session.set(id.clone());
                                            view.set(View::Chat);
                                        },
                                        "在聊天中继续"
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ---- Reminders -------------------------------------------------------------

#[component]
pub fn RemindersTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.reminders().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(rs)) if rs.is_empty() => rsx! { div { class: "empty", "没有提醒。" } },
                Some(Ok(rs)) => rsx! {
                    for r in rs.iter() {
                        div { class: "row",
                            span { class: "tag", "{r.status:?}" }
                            span { class: "row-main", "{r.message}" }
                            if !r.schedule.is_empty() {
                                span { class: "chip", "{r.schedule}" }
                            }
                            span { class: "muted", "{fmt_ts(r.run_at)}" }
                        }
                    }
                },
            }
        }
    }
}

// ---- Skills ----------------------------------------------------------------

#[component]
pub fn SkillsTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.skills().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1 }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(sk)) if sk.is_empty() => rsx! { div { class: "empty", "没有技能。" } },
                Some(Ok(sk)) => rsx! {
                    for s in sk.iter() {
                        div { class: "row skill-row",
                            div { class: "skill-head",
                                span { class: "row-main", "{s.name}" }
                                span { class: "chip", "{s.source}" }
                                if s.protected {
                                    span { class: "chip", "protected" }
                                }
                                if s.disabled {
                                    span { class: "chip deny", "disabled" }
                                }
                            }
                            div { class: "muted", "{s.description}" }
                        }
                    }
                },
            }
        }
    }
}

// ---- Pairings --------------------------------------------------------------

#[component]
pub fn PairingsTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    let mut code = use_signal(String::new);
    let mut notice = use_signal(String::new);
    use_future(move || async move {
        loop {
            tokio::time::sleep(POLL).await;
            tick += 1;
        }
    });
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.pairings().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let approve = move |_| {
        let c = code().trim().to_string();
        if c.is_empty() {
            return;
        }
        let client = conn.peek().client();
        code.set(String::new());
        spawn(async move {
            if let Some(client) = client {
                match client.pair_approve(&c).await {
                    Ok(()) => notice.set("已批准。".to_string()),
                    Err(e) => notice.set(format!("批准失败：{e}")),
                }
            }
            tick += 1;
        });
    };

    let revoke = move |id: String| {
        let client = conn.peek().client();
        spawn(async move {
            if let Some(client) = client {
                let _ = client.pair_revoke(&id).await;
            }
            tick += 1;
        });
    };

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1,
            input {
                class: "small",
                placeholder: "配对码…",
                value: "{code}",
                oninput: move |e| code.set(e.value()),
            }
            button { class: "small", onclick: approve, "批准配对码" }
        }
        if !notice().is_empty() {
            div { class: "notice", "{notice}" }
        }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok(ps)) if ps.is_empty() => rsx! { div { class: "empty", "没有配对。" } },
                Some(Ok(ps)) => rsx! {
                    for p in ps.iter() {
                        {
                            let id = p.id.clone();
                            rsx! {
                                div { class: "row",
                                    span { class: "tag", "{p.status}" }
                                    span { class: "row-main mono", "{p.id}" }
                                    span { class: "muted", "{fmt_ts(p.created_at)}" }
                                    button { class: "small", onclick: move |_| revoke(id.clone()), "解除" }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ---- Dream -----------------------------------------------------------------

#[component]
pub fn DreamTab() -> Element {
    let conn = use_context::<Signal<ConnState>>();
    let mut tick = use_signal(|| 0u64);
    let mut notice = use_signal(String::new);
    let data = use_resource(move || {
        let _ = tick();
        let client = conn.peek().client();
        async move {
            match client {
                Some(c) => c.dream_preview().await.map_err(|e| e.to_string()),
                None => Err("未连接".to_string()),
            }
        }
    });

    let apply = move |_| {
        let client = conn.peek().client();
        spawn(async move {
            if let Some(c) = client {
                match c.dream_apply().await {
                    Ok((p, a)) => notice.set(format!("已应用：promote {p} · archive {a}")),
                    Err(e) => notice.set(format!("失败：{e}")),
                }
            }
            tick += 1;
        });
    };

    let snapshot = data.read().clone();
    rsx! {
        RefreshBar { on_refresh: move |_| tick += 1,
            button { class: "small", onclick: apply, "应用一次 Dream" }
        }
        if !notice().is_empty() {
            div { class: "notice", "{notice}" }
        }
        div { class: "panel",
            match snapshot {
                None => rsx! { div { class: "loading", "加载中…" } },
                Some(Err(e)) => rsx! { div { class: "err", "{e}" } },
                Some(Ok((promote, archive))) if promote.is_empty() && archive.is_empty() => {
                    rsx! { div { class: "empty", "没有待整理的候选记忆。" } }
                }
                Some(Ok((promote, archive))) => rsx! {
                    if !promote.is_empty() {
                        div { class: "dream-group",
                            div { class: "dream-title ok", "将提升（{promote.len()}）" }
                            for d in promote.iter() {
                                div { class: "row",
                                    span { class: "muted", "recalls={d.recall_count} queries={d.unique_queries}" }
                                    span { class: "row-main", "{d.content}" }
                                }
                            }
                        }
                    }
                    if !archive.is_empty() {
                        div { class: "dream-group",
                            div { class: "dream-title deny", "将归档（{archive.len()}）" }
                            for d in archive.iter() {
                                div { class: "row",
                                    span { class: "muted", "recalls={d.recall_count}" }
                                    span { class: "row-main", "{d.content}" }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
