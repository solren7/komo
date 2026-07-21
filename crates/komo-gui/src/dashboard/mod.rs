//! Dashboard: read views + operator writes over the gateway's `/api/*` routes.
//!
//! Each tab owns its own `use_resource` (fetch on mount) plus a `use_future`
//! poll loop that only runs while the tab is mounted — switching tabs drops the
//! loop, so just the active view polls (every fetch is a gateway db read). A
//! manual Refresh and every write bump the same per-tab tick.

use dioxus::prelude::*;

use crate::ConnState;

mod tabs;
use tabs::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DashTab {
    Status,
    Tasks,
    Runs,
    Memories,
    Sessions,
    Reminders,
    Skills,
    Pairings,
    Dream,
}

const TABS: &[(DashTab, &str)] = &[
    (DashTab::Status, "状态"),
    (DashTab::Tasks, "任务"),
    (DashTab::Runs, "运行"),
    (DashTab::Memories, "记忆"),
    (DashTab::Sessions, "会话"),
    (DashTab::Reminders, "提醒"),
    (DashTab::Skills, "技能"),
    (DashTab::Pairings, "配对"),
    (DashTab::Dream, "Dream"),
];

#[component]
pub fn Dashboard() -> Element {
    let mut tab = use_signal(|| DashTab::Status);
    let conn = use_context::<Signal<ConnState>>();
    let connected = matches!(&*conn.read(), ConnState::Online(_));

    rsx! {
        div { class: "dashboard",
            div { class: "dash-tabs",
                for (t, label) in TABS.iter().copied() {
                    button {
                        class: if tab() == t { "dash-tab active" } else { "dash-tab" },
                        onclick: move |_| tab.set(t),
                        "{label}"
                    }
                }
            }
            div { class: "dash-body",
                if !connected {
                    div { class: "placeholder", "未连接到 gateway。" }
                } else {
                    match tab() {
                        DashTab::Status => rsx! { StatusTab {} },
                        DashTab::Tasks => rsx! { TasksTab {} },
                        DashTab::Runs => rsx! { RunsTab {} },
                        DashTab::Memories => rsx! { MemoriesTab {} },
                        DashTab::Sessions => rsx! { SessionsTab {} },
                        DashTab::Reminders => rsx! { RemindersTab {} },
                        DashTab::Skills => rsx! { SkillsTab {} },
                        DashTab::Pairings => rsx! { PairingsTab {} },
                        DashTab::Dream => rsx! { DreamTab {} },
                    }
                }
            }
        }
    }
}

/// Format a unix-seconds timestamp as UTC `MM-DD HH:MM` (compact, list-friendly).
pub(crate) fn fmt_ts(ts: i64) -> String {
    match time::OffsetDateTime::from_unix_timestamp(ts) {
        Ok(dt) => format!(
            "{:02}-{:02} {:02}:{:02}",
            u8::from(dt.month()),
            dt.day(),
            dt.hour(),
            dt.minute()
        ),
        Err(_) => ts.to_string(),
    }
}
