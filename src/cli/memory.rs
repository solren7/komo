//! `shion memory` — operator-facing governance over the memory library.
//!
//! Unlike the in-chat `memory` tool (scoped to the current chat), the CLI is a
//! host-side operator view: it lists and searches across *all* scopes, so you
//! can triage candidates the reviewer captured and promote/pin the durable ones.

use crate::domain::memory::{Memory, MemoryConfidence, MemoryRepository, MemoryStatus};
use crate::infra::memory_db::MemoryDb;

async fn store(url: &str) -> anyhow::Result<MemoryDb> {
    MemoryDb::connect(url).await
}

/// List stored memories, optionally filtered by status.
pub async fn list(url: &str, status: Option<String>) -> anyhow::Result<()> {
    let db = store(url).await?;
    let filter = status
        .as_deref()
        .map(crate::domain::memory::parse_memory_status);
    let mut memories = db.list().await?;
    if let Some(status) = filter {
        memories.retain(|m| m.status == status);
    }
    if memories.is_empty() {
        println!("(no memories)");
        return Ok(());
    }
    // Group by status so candidates needing triage stand out.
    memories.sort_by(|a, b| {
        a.status
            .as_str()
            .cmp(b.status.as_str())
            .then(b.updated_at.cmp(&a.updated_at))
    });
    for m in &memories {
        println!("{}", line(m));
    }
    Ok(())
}

/// Substring search across all scopes (operator view — no scope enforcement).
pub async fn search(url: &str, query: &str) -> anyhow::Result<()> {
    let db = store(url).await?;
    let needle = query.to_lowercase();
    let hits: Vec<Memory> = db
        .list()
        .await?
        .into_iter()
        .filter(|m| m.content.to_lowercase().contains(&needle))
        .collect();
    if hits.is_empty() {
        println!("(no matches)");
        return Ok(());
    }
    for m in &hits {
        println!("{}", line(m));
    }
    Ok(())
}

/// Promote a candidate to an active, confirmed memory.
pub async fn promote(url: &str, id: &str) -> anyhow::Result<()> {
    mutate(url, id, |m| {
        m.status = MemoryStatus::Active;
        m.confidence = MemoryConfidence::Confirmed;
    })
    .await?;
    println!("Promoted {id} to active.");
    Ok(())
}

/// Reject a candidate (won't surface in recall or injection).
pub async fn reject(url: &str, id: &str) -> anyhow::Result<()> {
    mutate(url, id, |m| m.status = MemoryStatus::Rejected).await?;
    println!("Rejected {id}.");
    Ok(())
}

/// Pin a memory into the L1 per-turn profile (the manual, explicit path —
/// automated extraction never pins). Raises confidence so it actually surfaces.
pub async fn pin(url: &str, id: &str) -> anyhow::Result<()> {
    mutate(url, id, |m| {
        m.pinned = true;
        m.status = MemoryStatus::Active;
        if m.confidence == MemoryConfidence::Extracted {
            m.confidence = MemoryConfidence::Confirmed;
        }
    })
    .await?;
    println!("Pinned {id} into the L1 profile.");
    Ok(())
}

async fn mutate(url: &str, id: &str, apply: impl FnOnce(&mut Memory)) -> anyhow::Result<()> {
    let db = store(url).await?;
    let mut memory = db
        .get(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no memory with id `{id}`"))?;
    apply(&mut memory);
    memory.updated_at = time::OffsetDateTime::now_utc().unix_timestamp();
    db.save(&memory).await?;
    Ok(())
}

fn line(m: &Memory) -> String {
    let pin = if m.pinned { " 📌" } else { "" };
    let mut s = format!(
        "{}  [{}/{}/{}{}]  {}",
        m.id,
        m.status.as_str(),
        m.kind.as_str(),
        m.scope.type_str(),
        pin,
        m.content
    );
    if !m.source.is_empty() {
        s.push_str(&format!("  (from {})", m.source));
    }
    s
}
