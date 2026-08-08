//! Vector search over the operator's note vault (Obsidian).
//!
//! This is a *derived* index, not a store of record: every chunk here is
//! reproducible from the markdown on disk, which is why the backing file is
//! disposable (delete it, run `komo wiki index`, get it back). That distinction
//! is what separates this from [`super::memory`] — memories are durable personal
//! data with no source to rebuild from.
//!
//! Recall injects memories automatically; wiki chunks are **pulled on demand**
//! by the `wiki_search` tool instead. A vault is orders of magnitude larger than
//! the memory store, so injecting from it every turn would spend context on
//! notes the turn never asked about.

use std::collections::HashMap;

use async_trait::async_trait;

/// One indexed slice of a note.
///
/// `id` is derived from `path` + `ordinal` rather than random, so re-indexing an
/// unchanged file produces the same ids and upsert stays idempotent — a random
/// id would duplicate every chunk on every run.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiChunk {
    pub id: String,
    /// Vault-relative path, e.g. `02-projects/checkout policy.md`. Relative so
    /// the index survives moving or renaming the vault directory itself.
    pub path: String,
    /// Markdown heading trail this chunk sits under (`设计 > 状态机`), empty for
    /// content before the first heading. Carried into the tool's output so a hit
    /// cites *where* in a 120 KB note it came from, not just which file.
    pub heading_path: String,
    /// Position within the file, 0-based. Part of `id`, and what orders chunks
    /// when several from one note are returned together.
    pub ordinal: usize,
    pub text: String,
    /// Source file's mtime at index time. The whole incremental story: a file
    /// whose mtime matches what is indexed is skipped without being read or
    /// embedded.
    pub mtime: i64,
    /// L2-normalized, per [`super::embedding::EmbeddingClient`]'s contract, so
    /// cosine similarity is a plain dot product.
    pub embedding: Vec<f32>,
    /// Model that produced `embedding`. Vectors from different models are not
    /// comparable, and the backing store fixes vector width at table creation,
    /// so a model change means rebuilding rather than mixing.
    pub embedding_model: String,
}

impl WikiChunk {
    /// Stable id for a chunk: `<path>#<ordinal>`. Readable on purpose — it shows
    /// up in logs and `komo wiki` output, where a hash would say nothing.
    pub fn make_id(path: &str, ordinal: usize) -> String {
        format!("{path}#{ordinal}")
    }
}

/// A scored search hit.
#[derive(Debug, Clone)]
pub struct WikiHit {
    pub chunk: WikiChunk,
    /// Cosine similarity against the query vector, in `[-1, 1]`.
    pub score: f32,
}

/// What is currently indexed for one note: its mtime, and how many chunks it
/// produced. The indexer diffs this against the filesystem to decide what to
/// re-embed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedFile {
    pub mtime: i64,
    pub chunks: usize,
}

/// Vector index over the vault.
///
/// Deliberately takes an already-embedded query vector rather than text: the
/// embedding backend lives a layer up (`EmbeddingClient`), so this trait stays
/// implementable by anything that can store and compare vectors, and a caller
/// that already has a vector never embeds twice.
#[async_trait]
pub trait WikiIndex: Send + Sync {
    /// Insert or replace `chunks` by id.
    async fn upsert(&self, chunks: &[WikiChunk]) -> anyhow::Result<()>;

    /// Top `limit` chunks by cosine similarity against `query`.
    ///
    /// `min_score` drops weak hits before they reach the model — an unrelated
    /// query against a vault always has a nearest neighbour, and returning it
    /// anyway is how a search tool starts fabricating relevance.
    async fn search(
        &self,
        query: &[f32],
        limit: usize,
        min_score: f32,
    ) -> anyhow::Result<Vec<WikiHit>>;

    /// Everything indexed, keyed by vault-relative path.
    async fn indexed(&self) -> anyhow::Result<HashMap<String, IndexedFile>>;

    /// Drop every chunk belonging to `paths`. Used both for notes deleted from
    /// the vault and, before re-indexing a changed note, to clear chunks a
    /// shorter version no longer produces.
    async fn delete_paths(&self, paths: &[String]) -> anyhow::Result<()>;

    /// Total chunk count, for `komo wiki status`.
    async fn count(&self) -> anyhow::Result<usize>;

    /// Drop the index entirely, so the next `upsert` builds it fresh.
    ///
    /// Required to change embedding model: vector width is fixed when the index
    /// is created, and a 1024-dim store cannot accept 2560-dim vectors. Deleting
    /// every point is not enough — the *store* has to go. Safe by construction,
    /// since the index is derived data that `komo wiki index` rebuilds.
    async fn reset(&self) -> anyhow::Result<()>;

    /// Vector width the index was created with, and the model that set it.
    /// `None` for an empty index, which adopts whatever the first upsert brings.
    async fn vector_spec(&self) -> anyhow::Result<Option<(usize, String)>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_id_is_stable_for_the_same_position() {
        assert_eq!(
            WikiChunk::make_id("03-areas/oncall.md", 4),
            WikiChunk::make_id("03-areas/oncall.md", 4)
        );
        assert_ne!(
            WikiChunk::make_id("03-areas/oncall.md", 4),
            WikiChunk::make_id("03-areas/oncall.md", 5)
        );
    }
}
