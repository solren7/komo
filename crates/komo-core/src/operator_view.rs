//! Operator view DTOs — the serialized shapes the gateway's HTTP endpoints emit
//! and the CLI / GUI clients deserialize.
//!
//! These carry no domain dependency (plain rows over `String`/`i64`/`f64`), so
//! they live in `komo-core` where any HTTP client can reuse them as the single
//! source of truth. The richer operator request/reply enums that *do* wrap
//! domain types stay in `komo::services::operator_control::request`, which
//! re-exports these for path stability.

use serde::{Deserialize, Serialize};

/// A session list row (full transcripts are never dumped in a list view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    /// Immutable workspace id selected when the session was created.
    #[serde(default = "default_workspace")]
    pub workspace: String,
    pub created_at: i64,
    pub messages: usize,
    pub user_turns: usize,
    /// Operator-set display name (empty = untitled). `default` so a payload from
    /// an older gateway still parses.
    #[serde(default)]
    pub title: String,
    /// Lifecycle status: `active` / `archive` (`deleted` sessions are omitted
    /// from the list). `default` for older-gateway compatibility.
    #[serde(default)]
    pub status: String,
    /// Per-session model override (empty = the gateway default). Switchable
    /// mid-conversation, unlike `workspace`.
    #[serde(default)]
    pub model: String,
    /// Per-session reasoning effort (empty = the provider default).
    #[serde(default)]
    pub effort: String,
}

fn default_workspace() -> String {
    "__default__".to_string()
}

/// A pairing row without the salted code hash / salt (never leaves the host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingView {
    pub id: String,
    /// `pending` | `approved` | `expired`.
    pub status: String,
    pub created_at: i64,
}

/// One `skill view` step from the run ledger (backs `komo skills audit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInvocation {
    pub run_id: String,
    pub seq: i64,
    pub started_at: i64,
    pub ok: bool,
}

/// The result of resuming an interrupted run, consumed by `komo run resume`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeOutcome {
    pub run_id: String,
    pub session_id: String,
    /// How many completed steps the priming digest handed to the model.
    pub steps: usize,
    pub reply: String,
}

/// One candidate in the dreaming preview, with the score that drove its verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamItem {
    pub id: String,
    pub recall_count: i64,
    /// Distinct recall-query fingerprints (the diversity half of the promote
    /// gate). `default` so a payload from an older gateway still parses.
    #[serde(default)]
    pub unique_queries: usize,
    pub score: f64,
    pub content: String,
}

/// The dreaming dry-run classification: which candidates would promote
/// (strongest case first), which would archive, and how many remain under
/// observation this cycle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DreamReport {
    pub promote: Vec<DreamItem>,
    pub archive: Vec<DreamItem>,
    /// Every candidate considered by the sweep, including ones that remain in
    /// the observation window. A missing field from an older gateway means the
    /// CLI can still render the actionable buckets safely.
    #[serde(default)]
    pub candidate_count: usize,
}

impl DreamReport {
    /// Whether this cycle has any state transition to apply.
    pub fn has_actions(&self) -> bool {
        !self.promote.is_empty() || !self.archive.is_empty()
    }

    /// Candidates that are neither ready to promote nor old and cold enough to
    /// archive yet.
    pub fn observing_count(&self) -> usize {
        self.candidate_count
            .saturating_sub(self.promote.len() + self.archive.len())
    }

    /// Backwards-compatible spelling for callers that mean “no state changes”,
    /// not “there are no candidate memories”.
    pub fn is_empty(&self) -> bool {
        !self.has_actions()
    }
}
