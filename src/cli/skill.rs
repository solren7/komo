//! Operator governance over the skill store (`~/.shion/skills`) — roadmap §9.
//!
//! All of these are pure file operations on the governed store, so they work
//! whether or not the gateway is running (no db lock involved). The runtime
//! `SkillRegistry` loads at startup, so changes that affect the agent's
//! catalog (promote / enable / disable) take effect on the next
//! `shion gateway restart`.

use crate::infra::skills::FsSkillStore;

fn store() -> FsSkillStore {
    FsSkillStore::new(FsSkillStore::default_root())
}

const RESTART_HINT: &str = "Takes effect for the agent after `shion gateway restart`.";

/// Accept a reviewer candidate: move it into the active store (overwriting the
/// active skill of the same name, i.e. accepting an update proposal).
pub fn promote(name: &str) -> anyhow::Result<()> {
    let skill = store().promote(name)?;
    println!("Promoted `{}` to active. {RESTART_HINT}", skill.name);
    Ok(())
}

/// Discard a reviewer candidate.
pub fn reject(name: &str) -> anyhow::Result<()> {
    store().reject(name)?;
    println!("Rejected candidate `{name}` (deleted).");
    Ok(())
}

/// Set or clear `protected`: a protected skill is operator-edit-only — the
/// reviewer stops writing even candidate proposals for it.
pub fn protect(name: &str, on: bool) -> anyhow::Result<()> {
    let skill = store().set_protected(name, on)?;
    if skill.protected {
        println!("Protected `{}` — the reviewer will no longer propose changes to it.", skill.name);
    } else {
        println!("Unprotected `{}`.", skill.name);
    }
    Ok(())
}

/// Enable or disable an active skill without deleting it: disabled skills stay
/// on disk and inspectable but are hidden from the model's catalog.
pub fn set_enabled(name: &str, enabled: bool) -> anyhow::Result<()> {
    let skill = store().set_disabled(name, !enabled)?;
    if skill.disabled {
        println!("Disabled `{}` — kept on disk, hidden from the agent. {RESTART_HINT}", skill.name);
    } else {
        println!("Enabled `{}`. {RESTART_HINT}", skill.name);
    }
    Ok(())
}
