//! File-mutation primitives shared by `write` (and next, `edit` /
//! `apply_patch`).
//!
//! The one non-obvious guarantee here is **stale protection**. A write goes:
//! read the current bytes → ask the user → write. That middle step can take
//! minutes (a chat approval), and the file can change under it — an editor save,
//! a `git checkout`, another agent turn. Writing anyway silently discards
//! whatever landed in between. So the write re-reads and compares against the
//! snapshot taken before the prompt, and refuses when it moved. Borrowed from
//! opencode v2's `FileMutation.writeIfUnchanged`.
//!
//! Scope is deliberately narrow: this closes the *approval window*, not a
//! cross-turn one. There is no "the model must have read the file first" rule.

use std::path::Path;

/// The file's state at snapshot time — `None` when it did not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot(Option<Vec<u8>>);

impl Snapshot {
    pub fn existed(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the snapshot starts with a UTF-8 BOM.
    fn had_bom(&self) -> bool {
        self.0
            .as_deref()
            .is_some_and(|b| b.starts_with(&[0xef, 0xbb, 0xbf]))
    }
}

/// The file changed between the snapshot and the write; nothing was written.
#[derive(Debug)]
pub struct StaleContent {
    pub path: String,
}

impl std::fmt::Display for StaleContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} changed after the approval prompt; nothing was written. \
             Read it again before writing.",
            self.path
        )
    }
}

impl std::error::Error for StaleContent {}

/// Capture the file's current bytes (absent is not an error — a `write` may be
/// creating it).
pub async fn snapshot(path: &Path) -> anyhow::Result<Snapshot> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Snapshot(Some(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Snapshot(None)),
        Err(e) => Err(anyhow::anyhow!("failed to read {}: {e}", path.display())),
    }
}

/// Write `content` only if the file still matches `expected`.
///
/// A UTF-8 BOM present in the snapshot is re-applied when the new content lacks
/// one: the model never sends a BOM, and silently stripping it would rewrite
/// every line ending of a Windows-authored file's first line in git.
pub async fn write_if_unchanged(
    path: &Path,
    expected: &Snapshot,
    content: &str,
) -> anyhow::Result<()> {
    let current = snapshot(path).await?;
    if &current != expected {
        return Err(StaleContent {
            path: path.display().to_string(),
        }
        .into());
    }

    let payload = if expected.had_bom() && !content.starts_with('\u{feff}') {
        format!("\u{feff}{content}")
    } else {
        content.to_string()
    };

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", parent.display()))?;
    }
    tokio::fs::write(path, payload.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("komo_filemut_{tag}"));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[tokio::test]
    async fn writes_when_the_file_is_unchanged() {
        let p = temp("unchanged");
        std::fs::write(&p, "old").unwrap();
        let snap = snapshot(&p).await.unwrap();
        write_if_unchanged(&p, &snap, "new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new");
    }

    #[tokio::test]
    async fn refuses_when_the_file_moved_under_us() {
        let p = temp("moved");
        std::fs::write(&p, "old").unwrap();
        let snap = snapshot(&p).await.unwrap();
        // Someone else saves the file while the approval prompt is up.
        std::fs::write(&p, "theirs").unwrap();

        let err = write_if_unchanged(&p, &snap, "mine").await.unwrap_err();
        assert!(err.to_string().contains("changed after the approval"));
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "theirs",
            "their write must survive"
        );
    }

    #[tokio::test]
    async fn creating_a_file_expects_it_absent() {
        let p = temp("create");
        let snap = snapshot(&p).await.unwrap();
        assert!(!snap.existed());
        write_if_unchanged(&p, &snap, "fresh").await.unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "fresh");
        let _ = std::fs::remove_file(&p);
    }

    /// A file appearing between snapshot and write is stale too — otherwise a
    /// "create" would silently clobber whatever just landed there.
    #[tokio::test]
    async fn a_file_appearing_after_the_snapshot_is_stale() {
        let p = temp("appeared");
        let snap = snapshot(&p).await.unwrap();
        std::fs::write(&p, "someone else got here first").unwrap();
        assert!(write_if_unchanged(&p, &snap, "mine").await.is_err());
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "someone else got here first"
        );
    }

    #[tokio::test]
    async fn a_bom_survives_a_rewrite() {
        let p = temp("bom");
        std::fs::write(&p, "\u{feff}old").unwrap();
        let snap = snapshot(&p).await.unwrap();
        write_if_unchanged(&p, &snap, "new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "\u{feff}new");
    }
}
