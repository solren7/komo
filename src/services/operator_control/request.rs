//! Typed operator requests and replies.
//!
//! These are the view types the operator surface exchanges — serialized by the
//! gateway's HTTP endpoints, deserialized by the CLI's gateway adapter, and
//! produced directly by the in-process adapter — so they live here as the
//! single source of truth, not in either transport.

use serde::{Deserialize, Serialize};

/// A session list row (full transcripts are never dumped in a list view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: i64,
    pub messages: usize,
    pub user_turns: usize,
}

/// A pairing row without the salted code hash / salt (never leaves the host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingView {
    pub id: String,
    /// `pending` | `approved` | `expired`.
    pub status: String,
    pub created_at: i64,
}

/// One `skill view` step from the run ledger (backs `shion skill audit`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInvocation {
    pub run_id: String,
    pub seq: i64,
    pub started_at: i64,
    pub ok: bool,
}

/// The result of resuming an interrupted run, consumed by `shion run resume`.
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
/// (strongest case first) and which would archive.
#[derive(Debug, Clone, Default)]
pub struct DreamReport {
    pub promote: Vec<DreamItem>,
    pub archive: Vec<DreamItem>,
}

impl DreamReport {
    pub fn is_empty(&self) -> bool {
        self.promote.is_empty() && self.archive.is_empty()
    }
}
