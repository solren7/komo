use serde::{Deserialize, Serialize};

use super::message::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Immutable workspace identity chosen when the session is first created.
    /// Older sessions predate workspaces and therefore belong to the default.
    #[serde(default = "default_workspace")]
    pub workspace: String,
    pub messages: Vec<Message>,
    pub created_at: i64,
    /// Optional operator-set display name (empty = untitled; clients fall back
    /// to a label derived from the id). Set via `SessionRepository::set_title`.
    #[serde(default)]
    pub title: String,
    /// Lifecycle: `"active"` (default), `"archive"`, or `"deleted"`. A soft
    /// status set via `SessionRepository::set_status`; the session list hides
    /// `deleted`. See [`SESSION_STATUS_ACTIVE`] etc.
    #[serde(default = "default_status")]
    pub status: String,
    /// Per-session model override (empty = the gateway's configured model).
    /// Unlike [`workspace`](Self::workspace) this is *not* creation-locked — a
    /// conversation may switch models mid-thread, and the last choice is what
    /// the next turn (and any other client opening the session) uses. Only
    /// honored for the main agent; aux/reviewer/briefing keep their own model.
    #[serde(default)]
    pub model: String,
    /// Per-session reasoning effort (`low` / `medium` / `high`; empty = the
    /// provider default). Which values a provider actually supports is decided
    /// by the LLM adapter — see `infra::llm::reasoning_params`.
    #[serde(default)]
    pub effort: String,
}

/// Default session status when none is stored (older rows, fresh sessions).
pub const SESSION_STATUS_ACTIVE: &str = "active";
pub const SESSION_STATUS_ARCHIVE: &str = "archive";
pub const SESSION_STATUS_DELETED: &str = "deleted";
pub const DEFAULT_WORKSPACE: &str = "__default__";

fn default_status() -> String {
    SESSION_STATUS_ACTIVE.to_string()
}

fn default_workspace() -> String {
    DEFAULT_WORKSPACE.to_string()
}

impl Session {
    pub fn new(id: impl Into<String>) -> Self {
        Self::with_workspace(id, DEFAULT_WORKSPACE)
    }

    pub fn with_workspace(id: impl Into<String>, workspace: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            workspace: workspace.into(),
            messages: Vec::new(),
            created_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            title: String::new(),
            status: default_status(),
            model: String::new(),
            effort: String::new(),
        }
    }

    /// The session's model override, or `None` when it runs on the gateway
    /// default.
    pub fn model_override(&self) -> Option<&str> {
        Some(self.model.trim()).filter(|m| !m.is_empty())
    }

    /// The session's reasoning effort, or `None` for the provider default.
    pub fn effort_override(&self) -> Option<&str> {
        Some(self.effort.trim()).filter(|e| !e.is_empty())
    }

    pub fn user_turns(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == super::message::Role::User)
            .count()
    }
}
