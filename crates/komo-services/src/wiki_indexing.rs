//! Index a note vault into a [`WikiIndex`].
//!
//! Lives here rather than in the CLI because two callers need it and must not
//! drift: `komo wiki index` when no gateway is running, and the gateway's own
//! operator action when one is. That is the same rule the operator commands
//! follow — one implementation, two transports.
//!
//! Depends only on the [`WikiIndex`] and [`EmbeddingClient`] traits, so it stays
//! inside `komo-services`' rule of never reaching into `komo-infra`: the caller
//! supplies the concrete backend and embedder.
//!
//! Indexing is **incremental by mtime**. A note whose file has not changed since
//! it was indexed is never read, chunked, or embedded again — embedding is the
//! entire cost of a run, so skipping unchanged files skips essentially all of it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use komo_core::domain::embedding::EmbeddingClient;
use komo_core::domain::wiki::{WikiChunk, WikiIndex};

use crate::wiki_chunking::{ChunkSpec, chunk_markdown};

/// How many chunks are embedded per request. A batch is far faster than the
/// same chunks one at a time, but an unbounded one risks the backend's own
/// request limits on a large note.
const EMBED_BATCH: usize = 32;

/// What one indexing run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexOutcome {
    pub files_seen: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    pub chunks_written: usize,
    /// Chunks in the index when the run finished.
    pub chunks_total: usize,
    /// Notes that could not be read, with the reason. One unreadable file must
    /// not abort a whole run, but it must not vanish silently either.
    pub skipped: Vec<String>,
}

/// Progress callback payload. Emitted per embedded batch, which is the only
/// point where a long run makes observable progress.
#[derive(Debug, Clone, Copy)]
pub struct IndexProgress {
    pub chunks_written: usize,
    pub files_changed: usize,
}

/// Every `.md` under `root`, skipping dot-directories (`.obsidian`, `.trash`).
pub fn walk_vault(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // A vanished or unreadable subdirectory must not abort the whole walk.
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn mtime_of(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Embed one batch and write it.
async fn flush(
    index: &dyn WikiIndex,
    embedder: &dyn EmbeddingClient,
    mut batch: Vec<WikiChunk>,
) -> anyhow::Result<usize> {
    let texts: Vec<String> = batch
        .iter()
        // The heading trail is embedded with the body: a chunk under
        // "checkout policy > 状态机" should match a query naming either, and the
        // body alone often names neither.
        .map(|c| format!("{}\n{}", c.heading_path, c.text))
        .collect();
    let vectors = embedder.embed(&texts).await?;
    if vectors.len() != batch.len() {
        anyhow::bail!(
            "embedding backend returned {} vectors for {} chunks",
            vectors.len(),
            batch.len()
        );
    }
    for (chunk, vector) in batch.iter_mut().zip(vectors) {
        chunk.embedding = vector;
    }
    let n = batch.len();
    index.upsert(&batch).await?;
    Ok(n)
}

/// Index `vault` into `index`, embedding with `embedder`.
///
/// `rebuild` drops the store first. That is not the same as deleting every
/// point: vector width is fixed when the index is created, so changing the
/// embedding model is only possible this way.
///
/// `on_progress` is called after each embedded batch. A run over a large vault
/// takes minutes, and this is the only signal a caller can surface.
pub async fn index_vault(
    index: &dyn WikiIndex,
    embedder: &dyn EmbeddingClient,
    vault: &Path,
    embedding_model: &str,
    rebuild: bool,
    mut on_progress: impl FnMut(IndexProgress),
) -> anyhow::Result<IndexOutcome> {
    if !vault.is_dir() {
        anyhow::bail!("vault not found: {}", vault.display());
    }
    let files = walk_vault(vault);
    let indexed = if rebuild {
        index.reset().await?;
        HashMap::new()
    } else {
        index.indexed().await?
    };

    // The index stores vault-relative paths, so the whole diff happens in that
    // space — absolute paths would break the moment the vault directory moves.
    let mut on_disk: HashMap<String, (PathBuf, i64)> = HashMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(vault)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        on_disk.insert(rel, (path.clone(), mtime_of(path)));
    }

    let mut changed: Vec<String> = on_disk
        .iter()
        .filter(|(rel, (_, mtime))| indexed.get(*rel).is_none_or(|had| had.mtime != *mtime))
        .map(|(rel, _)| rel.clone())
        .collect();
    changed.sort();
    let live: HashSet<&str> = on_disk.keys().map(String::as_str).collect();
    let removed: Vec<String> = indexed
        .keys()
        .filter(|rel| !live.contains(rel.as_str()))
        .cloned()
        .collect();

    let mut outcome = IndexOutcome {
        files_seen: files.len(),
        files_changed: changed.len(),
        files_removed: removed.len(),
        ..Default::default()
    };

    if !removed.is_empty() {
        index.delete_paths(&removed).await?;
    }
    if changed.is_empty() && removed.is_empty() {
        outcome.chunks_total = index.count().await?;
        return Ok(outcome);
    }
    // A changed file's old chunks must go before its new ones land, or a note
    // that got shorter keeps orphaned tail chunks forever. After `rebuild` the
    // store is already empty.
    if !rebuild {
        index.delete_paths(&changed).await?;
    }

    let spec = ChunkSpec::default();
    let mut pending: Vec<WikiChunk> = Vec::new();

    for rel in &changed {
        let (path, mtime) = &on_disk[rel];
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                outcome.skipped.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let title = path.file_stem().unwrap_or_default().to_string_lossy();
        for raw in chunk_markdown(&title, &content, &spec) {
            pending.push(WikiChunk {
                id: WikiChunk::make_id(rel, raw.ordinal),
                path: rel.clone(),
                heading_path: raw.heading_path,
                ordinal: raw.ordinal,
                text: raw.text,
                mtime: *mtime,
                embedding: Vec::new(),
                embedding_model: embedding_model.to_string(),
            });
        }
        while pending.len() >= EMBED_BATCH {
            let batch: Vec<WikiChunk> = pending.drain(..EMBED_BATCH).collect();
            outcome.chunks_written += flush(index, embedder, batch).await?;
            on_progress(IndexProgress {
                chunks_written: outcome.chunks_written,
                files_changed: outcome.files_changed,
            });
        }
    }
    if !pending.is_empty() {
        outcome.chunks_written += flush(index, embedder, pending).await?;
        on_progress(IndexProgress {
            chunks_written: outcome.chunks_written,
            files_changed: outcome.files_changed,
        });
    }

    outcome.chunks_total = index.count().await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use komo_core::domain::wiki::{IndexedFile, WikiHit};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeIndex {
        chunks: Mutex<Vec<WikiChunk>>,
        resets: Mutex<usize>,
    }

    #[async_trait]
    impl WikiIndex for FakeIndex {
        async fn upsert(&self, chunks: &[WikiChunk]) -> anyhow::Result<()> {
            let mut held = self.chunks.lock().unwrap();
            for chunk in chunks {
                held.retain(|c| c.id != chunk.id);
                held.push(chunk.clone());
            }
            Ok(())
        }
        async fn search(
            &self,
            _: &[f32],
            _: &str,
            _: usize,
            _: f32,
        ) -> anyhow::Result<Vec<WikiHit>> {
            Ok(Vec::new())
        }
        async fn indexed(&self) -> anyhow::Result<HashMap<String, IndexedFile>> {
            let mut out: HashMap<String, IndexedFile> = HashMap::new();
            for chunk in self.chunks.lock().unwrap().iter() {
                let entry = out.entry(chunk.path.clone()).or_insert(IndexedFile {
                    mtime: chunk.mtime,
                    chunks: 0,
                });
                entry.chunks += 1;
                entry.mtime = entry.mtime.min(chunk.mtime);
            }
            Ok(out)
        }
        async fn delete_paths(&self, paths: &[String]) -> anyhow::Result<()> {
            self.chunks
                .lock()
                .unwrap()
                .retain(|c| !paths.contains(&c.path));
            Ok(())
        }
        async fn count(&self) -> anyhow::Result<usize> {
            Ok(self.chunks.lock().unwrap().len())
        }
        async fn reset(&self) -> anyhow::Result<()> {
            self.chunks.lock().unwrap().clear();
            *self.resets.lock().unwrap() += 1;
            Ok(())
        }
        async fn vector_spec(&self) -> anyhow::Result<Option<(usize, String)>> {
            Ok(None)
        }
    }

    struct FakeEmbedder;

    #[async_trait]
    impl EmbeddingClient for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
        fn model_id(&self) -> &str {
            "fake"
        }
    }

    fn vault_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    async fn run(index: &FakeIndex, vault: &Path, rebuild: bool) -> IndexOutcome {
        index_vault(index, &FakeEmbedder, vault, "fake", rebuild, |_| {})
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn first_run_indexes_everything() {
        let vault = vault_with(&[("a.md", "甲的正文内容"), ("b.md", "乙的正文内容")]);
        let index = FakeIndex::default();
        let out = run(&index, vault.path(), false).await;
        assert_eq!(out.files_seen, 2);
        assert_eq!(out.files_changed, 2);
        assert!(out.chunks_written >= 2);
        assert_eq!(out.chunks_total, out.chunks_written);
    }

    /// The whole point of the mtime diff: a second run must embed nothing.
    #[tokio::test]
    async fn second_run_skips_unchanged_files() {
        let vault = vault_with(&[("a.md", "甲的正文内容")]);
        let index = FakeIndex::default();
        run(&index, vault.path(), false).await;

        let out = run(&index, vault.path(), false).await;
        assert_eq!(out.files_changed, 0);
        assert_eq!(out.chunks_written, 0);
        assert!(out.chunks_total > 0, "index must still hold the chunks");
    }

    /// A note deleted from the vault must lose its chunks.
    #[tokio::test]
    async fn removed_files_are_deleted_from_the_index() {
        let vault = vault_with(&[("a.md", "甲的正文内容"), ("b.md", "乙的正文内容")]);
        let index = FakeIndex::default();
        run(&index, vault.path(), false).await;

        std::fs::remove_file(vault.path().join("b.md")).unwrap();
        let out = run(&index, vault.path(), false).await;
        assert_eq!(out.files_removed, 1);
        let indexed = index.indexed().await.unwrap();
        assert!(!indexed.contains_key("b.md"), "{indexed:?}");
    }

    /// A shortened note must not keep the tail chunks of its longer version.
    #[tokio::test]
    async fn a_shortened_note_drops_its_orphaned_chunks() {
        let long = "很长的一段内容。".repeat(300);
        let vault = vault_with(&[("a.md", long.as_str())]);
        let index = FakeIndex::default();
        let first = run(&index, vault.path(), false).await;
        assert!(first.chunks_written > 1);

        std::fs::write(vault.path().join("a.md"), "短".repeat(20)).unwrap();
        // mtime resolution is one second, so a rewrite within the same second
        // would look unchanged; stamp an explicit time instead of sleeping.
        let file = std::fs::File::options()
            .write(true)
            .open(vault.path().join("a.md"))
            .unwrap();
        file.set_times(
            std::fs::FileTimes::new().set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
            ),
        )
        .unwrap();
        let second = run(&index, vault.path(), false).await;
        assert_eq!(second.chunks_total, second.chunks_written);
        assert!(second.chunks_total < first.chunks_written);
    }

    #[tokio::test]
    async fn rebuild_resets_the_store() {
        let vault = vault_with(&[("a.md", "甲的正文内容")]);
        let index = FakeIndex::default();
        run(&index, vault.path(), false).await;
        run(&index, vault.path(), true).await;
        assert_eq!(*index.resets.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn a_missing_vault_is_an_error() {
        let index = FakeIndex::default();
        let err = index_vault(
            &index,
            &FakeEmbedder,
            Path::new("/definitely/not/here"),
            "fake",
            false,
            |_| {},
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("vault not found"), "{err}");
    }

    #[tokio::test]
    async fn dot_directories_are_skipped() {
        let vault = vault_with(&[
            ("a.md", "甲的正文内容"),
            (".obsidian/workspace.md", "不该被索引"),
        ]);
        let index = FakeIndex::default();
        let out = run(&index, vault.path(), false).await;
        assert_eq!(out.files_seen, 1);
    }
}
