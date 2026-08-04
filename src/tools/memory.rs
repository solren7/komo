use std::sync::Arc;

use async_trait::async_trait;
use komo_services::memory_enrichment::pinned_budget_usage;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    context::ToolContext,
    memory::{
        Memory, MemoryConfidence, MemoryContext, MemoryKind, MemoryQuery, MemoryRepository,
        MemoryStatus, ScoredMemory, parse_memory_kind, parse_memory_status,
    },
    tool::{Tool, ToolError, ToolOutput, parse_args},
};

/// Default cap on search results.
const SEARCH_LIMIT: usize = 10;

#[derive(Deserialize)]
struct MemoryArgs {
    action: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    query: Option<String>,
    /// Target memory id (action=update/promote/reject/archive).
    #[serde(default)]
    id: Option<String>,
    /// New status (action=update).
    #[serde(default)]
    status: Option<String>,
    /// Pin/unpin (action=update). Pinning is the only path into L1 injection.
    #[serde(default)]
    pinned: Option<bool>,
    /// New ranking weight 0–100 (action=update).
    #[serde(default)]
    importance: Option<i32>,
    /// Optional TTL in days (action=save).
    #[serde(default)]
    expiry_days: Option<i64>,
}

impl MemoryArgs {
    /// Some models fill every optional schema field with a placeholder instead
    /// of omitting it (`"id": ""`, `"status": ""`). An empty string is never a
    /// meaningful value for any of these, so normalize it to absent — otherwise
    /// `parse_memory_status("")` silently becomes an `active` filter and a
    /// `list` over an all-candidate store returns nothing.
    fn normalized(mut self) -> Self {
        for field in [
            &mut self.text,
            &mut self.kind,
            &mut self.query,
            &mut self.id,
            &mut self.status,
        ] {
            if field.as_deref().is_some_and(|s| s.trim().is_empty()) {
                *field = None;
            }
        }
        self
    }
}

/// Long-term, cross-session memory with governance. The model `save`s facts,
/// `search`es them (scoped to the current chat/session), and curates the
/// library: `promote` a candidate to active, `reject`/`archive` it, or `update`
/// fields (including `pinned`, which gates L1 per-turn injection). Storage lives
/// behind [`MemoryRepository`] — the same store the reviewer writes to.
pub struct MemoryTool {
    memories: Arc<dyn MemoryRepository>,
}

impl MemoryTool {
    pub fn new(memories: Arc<dyn MemoryRepository>) -> Self {
        Self { memories }
    }

    /// A Hermes-style usage line for the L1 pinned profile — the one memory
    /// surface with a real, finite budget (it is injected verbatim every turn).
    /// Surfacing "how full is it" nudges the model to keep pinned compact and
    /// curate before adding. Returns `None` when nothing is pinned (no pressure
    /// to report). Best-effort: a load failure just omits the line.
    async fn pinned_usage_line(&self, scope: &MemoryContext) -> Option<String> {
        let pinned = self.memories.pinned(scope).await.ok()?;
        let (used, budget) = pinned_budget_usage(&pinned);
        if used == 0 {
            return None;
        }
        let pct = (used * 100) / budget;
        Some(format!(
            "L1 pinned profile: {used}/{budget} chars ({pct}%) used."
        ))
    }

    /// Load a memory by id or return a helpful error.
    /// Look up the memory an action names. A missing / unknown id is the model's
    /// mistake to fix, so both map to [`ToolError::InvalidInput`] rather than a
    /// retryable failure.
    async fn require(&self, id: &Option<String>) -> Result<Memory, ToolError> {
        let id = id.as_deref().ok_or_else(|| {
            ToolError::InvalidInput("`id` is required for this action".to_string())
        })?;
        self.memories
            .get(id)
            .await?
            .ok_or_else(|| ToolError::InvalidInput(format!("no memory with id `{id}`")))
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn description(&self) -> &'static str {
        "Persistent long-term memory across sessions, with governance. \
         action=\"save\" stores a fact (optional kind: profile | preference | feedback | \
         project | person | fact | decision | reference); action=\"search\" returns facts \
         matching a query (scoped to this chat); action=\"list\" returns stored facts; \
         action=\"update\" changes a memory by id (status / pinned / importance / kind / \
         content); action=\"promote\" marks a candidate active; action=\"reject\" / \
         \"archive\" retire one. Pin a memory (update pinned=true) only when the user \
         confirms it as durable profile context. \
         Write each memory as a declarative fact, not an instruction (\"User prefers \
         concise replies\" ✓, \"Always reply concisely\" ✗), and prioritize what reduces \
         future steering. Do not save anything that will be stale within a week — task \
         progress, completed-work logs, PR/issue numbers, or commit SHAs do not belong here."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["save", "search", "list", "update", "promote", "reject", "archive"],
                    "description": "The memory operation to perform."
                },
                "text": { "type": "string", "description": "Fact to store (action=save) or new content (action=update)." },
                "kind": {
                    "type": "string",
                    "enum": ["profile", "preference", "feedback", "project", "person", "fact", "decision", "reference"],
                    "description": "Category (action=save, default profile; or action=update)."
                },
                "query": { "type": "string", "description": "Search term (action=search)." },
                "id": { "type": "string", "description": "Target memory id (action=update/promote/reject/archive)." },
                "status": { "type": "string", "enum": ["candidate", "active", "archived", "rejected"], "description": "New status (action=update)." },
                "pinned": { "type": "boolean", "description": "Pin/unpin for L1 injection (action=update). Only pin user-confirmed durable facts." },
                "importance": { "type": "integer", "description": "Ranking weight 0–100 (action=update)." },
                "expiry_days": { "type": "integer", "description": "Optional TTL in days (action=save); omit for permanent." }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, input: Value, tool_ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: MemoryArgs = parse_args::<MemoryArgs>(&input)?.normalized();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        // Scope comes from the *explicit* per-call context (tool trait v2), not
        // the ambient task-local: `memory` was the last tool reading that seam.
        let scope = memory_context(&tool_ctx.session.session_id);

        match args.action.as_str() {
            "save" => {
                let text = args.text.ok_or_else(|| {
                    ToolError::InvalidInput("`text` is required for action=save".to_string())
                })?;
                let kind = args
                    .kind
                    .as_deref()
                    .map(parse_memory_kind)
                    .unwrap_or(MemoryKind::Profile);
                let mut memory = Memory::new(kind, text);
                // An explicit user save is the highest trust tier.
                memory.confidence = MemoryConfidence::UserWritten;
                // Scope to the current chat so a channel fact does not leak elsewhere.
                memory.scope = scope.write_scope();
                if let Some(days) = args.expiry_days.filter(|d| *d > 0) {
                    memory.expires_at = Some(now + days * 86_400);
                }
                self.memories.save(&memory).await?;
                let mut out = format!("Saved memory {}.", memory.id);
                if let Some(usage) = self.pinned_usage_line(&scope).await {
                    out.push('\n');
                    out.push_str(&usage);
                }
                Ok(ToolOutput::text(out).with_structured(json!({ "id": memory.id })))
            }
            "list" => {
                let mut memories = self.memories.list().await?;
                let total = memories.len();
                let breakdown = status_breakdown(&memories);
                if let Some(status) = args.status.as_deref().map(parse_memory_status) {
                    memories.retain(|m| m.status == status);
                }
                let mut out = if memories.is_empty() && total > 0 {
                    // A status filter that matched nothing must not read as "the
                    // store is empty" — say where the memories actually are so
                    // the model can re-list instead of concluding there are none.
                    format!(
                        "No memories with that status, but {total} exist: {breakdown}. \
                         Call list without `status` to see them."
                    )
                } else {
                    render(&memories)
                };
                if let Some(usage) = self.pinned_usage_line(&scope).await {
                    out.push_str("\n\n");
                    out.push_str(&usage);
                }
                Ok(ToolOutput::text(out).with_title(format!("{} memories", memories.len())))
            }
            "search" => {
                let text = args.query.ok_or_else(|| {
                    ToolError::InvalidInput("`query` is required for action=search".to_string())
                })?;
                let query = MemoryQuery {
                    text,
                    allowed_scopes: scope.allowed_scopes.clone(),
                    kinds: Vec::new(),
                    statuses: vec![MemoryStatus::Active],
                    limit: SEARCH_LIMIT,
                };
                let hits = self.memories.search(query).await?;
                Ok(ToolOutput::text(render_scored(&hits))
                    .with_title(format!("{} matches", hits.len())))
            }
            "update" => {
                let mut memory = self.require(&args.id).await?;
                if let Some(text) = args.text {
                    memory.content = text;
                }
                if let Some(kind) = args.kind.as_deref() {
                    memory.kind = parse_memory_kind(kind);
                }
                if let Some(status) = args.status.as_deref() {
                    memory.status = parse_memory_status(status);
                }
                if let Some(pinned) = args.pinned {
                    memory.pinned = pinned;
                    // Pinning requires high confidence to actually surface in L1.
                    if pinned && memory.confidence == MemoryConfidence::Extracted {
                        memory.confidence = MemoryConfidence::Confirmed;
                    }
                }
                if let Some(importance) = args.importance {
                    memory.importance = importance.clamp(0, 100);
                }
                memory.updated_at = now;
                self.memories.save(&memory).await?;
                Ok(ToolOutput::text(format!("Updated memory {}.", memory.id)))
            }
            "promote" => {
                let mut memory = self.require(&args.id).await?;
                memory.promote(now);
                self.memories.save(&memory).await?;
                Ok(ToolOutput::text(format!(
                    "Promoted memory {} to active.",
                    memory.id
                )))
            }
            "reject" => set_status(self, &args.id, MemoryStatus::Rejected, now).await,
            "archive" => set_status(self, &args.id, MemoryStatus::Archived, now).await,
            other => Err(ToolError::InvalidInput(format!(
                "unknown action `{other}` (expected save/search/list/update/promote/reject/archive)"
            ))),
        }
    }
}

/// The memory context for this call, derived from the turn's session id (see
/// [`MemoryContext::from_session`]: a chat session also gets channel scope, a
/// CLI one does not). An empty id yields the global-only context.
fn memory_context(session_id: &str) -> MemoryContext {
    MemoryContext::from_session(session_id)
}

async fn set_status(
    tool: &MemoryTool,
    id: &Option<String>,
    status: MemoryStatus,
    now: i64,
) -> Result<ToolOutput, ToolError> {
    let mut memory = tool.require(id).await?;
    memory.status = status;
    memory.updated_at = now;
    tool.memories.save(&memory).await?;
    Ok(ToolOutput::text(format!(
        "Set memory {} to {}.",
        memory.id,
        status.as_str()
    )))
}

/// Count memories per status, e.g. `candidate=24, archived=2`.
fn status_breakdown(memories: &[Memory]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for m in memories {
        let name = m.status.as_str();
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, c)) => *c += 1,
            None => counts.push((name, 1)),
        }
    }
    counts
        .iter()
        .map(|(n, c)| format!("{n}={c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render(memories: &[Memory]) -> String {
    if memories.is_empty() {
        return "(no memories)".to_string();
    }
    memories
        .iter()
        .map(render_one)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_one(m: &Memory) -> String {
    let pin = if m.pinned { " 📌" } else { "" };
    let mut line = format!(
        "[{}/{}/{}{}] {}: {}",
        m.kind.as_str(),
        m.status.as_str(),
        m.scope.type_str(),
        pin,
        m.id,
        m.content
    );
    if !m.source.is_empty() {
        line.push_str(&format!(" (from {})", m.source));
    }
    line
}

fn render_scored(hits: &[ScoredMemory]) -> String {
    if hits.is_empty() {
        return "(no matches)".to_string();
    }
    hits.iter()
        .map(|h| render_one(&h.memory))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use komo_infra::memory::md_memory::MdMemoryStore;

    fn temp_tool(name: &str) -> MemoryTool {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        MemoryTool::new(Arc::new(MdMemoryStore::new(dir)))
    }

    /// A CLI-shaped session: global + session scope, no channel scope.
    fn ctx() -> ToolContext {
        crate::tools::test_support::detached_ctx("cli:test")
    }

    #[tokio::test]
    async fn save_list_search_roundtrip() {
        let tool = temp_tool("komo_mem_tool_test");

        tool.call(json!({ "action": "save", "text": "用户喜欢蓝色" }), &ctx())
            .await
            .unwrap();
        tool.call(
            json!({ "action": "save", "text": "项目用 Rust 写", "kind": "project" }),
            &ctx(),
        )
        .await
        .unwrap();

        let list = tool
            .call(json!({ "action": "list" }), &ctx())
            .await
            .unwrap()
            .text;
        assert!(list.contains("蓝色"));
        assert!(list.contains("Rust"));
        assert!(list.contains("[project/"));

        let hit = tool
            .call(json!({ "action": "search", "query": "rust" }), &ctx())
            .await
            .unwrap()
            .text;
        assert!(hit.contains("Rust"));
        assert!(!hit.contains("蓝色"));
    }

    /// The exact call shape observed from a model that fills every optional
    /// field with a placeholder (run-019fc562): `status: "active"` over an
    /// all-candidate store must not read as "the store is empty".
    #[tokio::test]
    async fn list_filtered_to_nothing_reports_where_memories_are() {
        let tool = temp_tool("komo_mem_tool_filler");
        let mut cand = Memory::new(MemoryKind::Fact, "user prefers rebase before push");
        cand.status = MemoryStatus::Candidate;
        tool.memories.save(&cand).await.unwrap();

        let out = tool
            .call(
                json!({
                    "action": "list", "status": "active", "kind": "fact",
                    "id": "", "query": "", "text": "",
                    "importance": 0, "pinned": false, "expiry_days": 0
                }),
                &ctx(),
            )
            .await
            .unwrap()
            .text;
        assert!(!out.contains("(no memories)"));
        assert!(out.contains("candidate=1"));
    }

    /// An empty-string `status` is a placeholder, not an `active` filter
    /// (`parse_memory_status("")` would otherwise default to Active).
    #[tokio::test]
    async fn empty_string_args_are_treated_as_absent() {
        let tool = temp_tool("komo_mem_tool_empty_args");
        let mut cand = Memory::new(MemoryKind::Fact, "protoc lives in /opt/homebrew/bin");
        cand.status = MemoryStatus::Candidate;
        tool.memories.save(&cand).await.unwrap();

        let out = tool
            .call(json!({ "action": "list", "status": "", "id": "" }), &ctx())
            .await
            .unwrap()
            .text;
        assert!(out.contains("protoc"));
    }

    #[tokio::test]
    async fn promote_then_pin_via_update() {
        let tool = temp_tool("komo_mem_tool_promote");
        // A candidate (simulating a reviewer extraction).
        let mut cand = Memory::new(MemoryKind::Preference, "prefers concise answers");
        cand.status = MemoryStatus::Candidate;
        cand.confidence = MemoryConfidence::Extracted;
        tool.memories.save(&cand).await.unwrap();

        tool.call(json!({ "action": "promote", "id": cand.id }), &ctx())
            .await
            .unwrap();
        let after = tool.memories.get(&cand.id).await.unwrap().unwrap();
        assert_eq!(after.status, MemoryStatus::Active);
        assert_eq!(after.confidence, MemoryConfidence::Confirmed);

        tool.call(
            json!({ "action": "update", "id": cand.id, "pinned": true }),
            &ctx(),
        )
        .await
        .unwrap();
        let pinned = tool.memories.get(&cand.id).await.unwrap().unwrap();
        assert!(pinned.pinned);
    }

    #[tokio::test]
    async fn reject_and_archive_set_status() {
        let tool = temp_tool("komo_mem_tool_reject");
        let m = Memory::new(MemoryKind::Fact, "ephemeral");
        tool.memories.save(&m).await.unwrap();

        tool.call(json!({ "action": "reject", "id": m.id }), &ctx())
            .await
            .unwrap();
        assert_eq!(
            tool.memories.get(&m.id).await.unwrap().unwrap().status,
            MemoryStatus::Rejected
        );
    }

    #[tokio::test]
    async fn update_unknown_id_errors() {
        let tool = temp_tool("komo_mem_tool_unknown");
        let err = tool
            .call(json!({ "action": "promote", "id": "nope" }), &ctx())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no memory with id"));
    }
}
