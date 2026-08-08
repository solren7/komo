//! Embedded backend: `qdrant-edge` running in-process.
//!
//! Two things shape this file.
//!
//! **The API is synchronous.** `EdgeShard` does blocking file I/O, so every call
//! is wrapped in `spawn_blocking`; calling it directly from the async context
//! would stall the executor for the duration of a search.
//!
//! **The shard is created lazily.** A shard's config fixes the vector width at
//! creation, and the width is only known once the first embedded chunk arrives.
//! So an index with no data yet holds `None`, and the first `upsert` creates the
//! shard with that batch's dimensionality. Every read against an
//! uncreated shard answers "empty" rather than failing — asking a
//! never-indexed vault a question is a legitimate state, not an error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use komo_core::domain::wiki::{IndexedFile, WikiChunk, WikiHit, WikiIndex};
use qdrant_edge::{
    Distance, EdgeConfig, EdgeShard, EdgeVectorParams, NamedQuery, Payload, PointId,
    PointInsertOperations, PointOperations, QueryEnum, ScrollRequest, SearchRequestBuilder,
    UpdateOperation, VectorInternal, VectorStructInternal, VectorStructPersisted, WithPayloadInterface,
    WithVector,
};

use crate::payload::{self, F_MODEL, F_MTIME, F_PATH, VECTOR_NAME, point_id, to_payload};

/// Page size for full scans. The request default is 10, which would turn one
/// `indexed()` call into hundreds of round trips.
const SCROLL_PAGE: usize = 1024;

pub struct EdgeIndex {
    path: PathBuf,
    /// `None` until the first upsert establishes the vector width — see the
    /// module docs.
    shard: Arc<RwLock<Option<Arc<EdgeShard>>>>,
}

impl EdgeIndex {
    /// Open the index under `data_dir/collection`, loading an existing shard if
    /// one is there.
    pub fn open(data_dir: &Path, collection: &str) -> anyhow::Result<Self> {
        let path = data_dir.join(collection);
        let existing = if path.join("segments").exists() || path.join("wal").exists() {
            Some(Arc::new(EdgeShard::load(&path, None).map_err(|e| {
                anyhow!("loading wiki index at {}: {e}", path.display())
            })?))
        } else {
            None
        };
        Ok(Self {
            path,
            shard: Arc::new(RwLock::new(existing)),
        })
    }

    fn snapshot(&self) -> Option<Arc<EdgeShard>> {
        self.shard.read().ok()?.clone()
    }

    /// Create the shard sized for `dim`, or return the existing one.
    ///
    /// An existing shard of a *different* width is a hard error, not a silent
    /// mismatch: vector width is fixed at creation, so this is what a change of
    /// embedding model looks like from here, and the message has to say what
    /// fixes it.
    fn ensure(&self, dim: usize) -> anyhow::Result<Arc<EdgeShard>> {
        if let Some(shard) = self.snapshot() {
            let existing = shard
                .config()
                .vectors
                .get(VECTOR_NAME)
                .map(|v| v.size)
                .unwrap_or(dim);
            if existing != dim {
                anyhow::bail!(
                    "index was built for {existing}-dim vectors but the embedding \
                     model produces {dim}-dim. Vector width is fixed when the index \
                     is created — run `komo wiki index --rebuild` to rebuild it."
                );
            }
            return Ok(shard);
        }
        let mut guard = self
            .shard
            .write()
            .map_err(|_| anyhow!("wiki index lock poisoned"))?;
        // Another writer may have created it between the read and the write.
        if let Some(shard) = guard.as_ref() {
            return Ok(shard.clone());
        }
        std::fs::create_dir_all(&self.path)
            .with_context(|| format!("creating {}", self.path.display()))?;
        let config = EdgeConfig {
            on_disk_payload: Some(false),
            vectors: HashMap::from([(
                VECTOR_NAME.to_string(),
                EdgeVectorParams {
                    size: dim,
                    // Vectors reach us L2-normalized (the `EmbeddingClient`
                    // contract), so a dot product *is* cosine similarity, and
                    // scores come back directly comparable to a caller's
                    // `min_score` with no conversion.
                    distance: Distance::Dot,
                    quantization_config: None,
                    multivector_config: None,
                    datatype: None,
                    on_disk: None,
                    hnsw_config: None,
                },
            )]),
            sparse_vectors: HashMap::new(),
            hnsw_config: Default::default(),
            quantization_config: None,
            optimizers: Default::default(),
            wal_options: None,
            max_search_threads: None,
            search_pool_core: None,
        };
        let shard = Arc::new(
            EdgeShard::new(&self.path, config)
                .map_err(|e| anyhow!("creating wiki index at {}: {e}", self.path.display()))?,
        );
        *guard = Some(shard.clone());
        Ok(shard)
    }

    /// Read every point's payload. Both `indexed` and `delete_paths` need the
    /// full set, and at vault scale (a few thousand points) one scan is cheaper
    /// than maintaining a secondary index.
    fn scan(shard: &EdgeShard) -> anyhow::Result<Vec<(PointId, serde_json::Value)>> {
        let mut out = Vec::new();
        let mut offset = None;
        loop {
            let request = ScrollRequest {
                offset,
                limit: Some(SCROLL_PAGE),
                filter: None,
                with_payload: Some(WithPayloadInterface::Bool(true)),
                with_vector: WithVector::Bool(false),
                order_by: None,
            };
            let (records, next) = shard
                .scroll(request)
                .map_err(|e| anyhow!("scanning wiki index: {e}"))?;
            for record in records {
                let value = record
                    .payload
                    .as_ref()
                    .and_then(|p| serde_json::to_value(p).ok())
                    .unwrap_or(serde_json::Value::Null);
                out.push((record.id, value));
            }
            match next {
                Some(next) => offset = Some(next),
                None => break,
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl WikiIndex for EdgeIndex {
    async fn upsert(&self, chunks: &[WikiChunk]) -> anyhow::Result<()> {
        let Some(dim) = chunks.iter().map(|c| c.embedding.len()).find(|n| *n > 0) else {
            return Ok(());
        };
        if let Some(bad) = chunks
            .iter()
            .find(|c| !c.embedding.is_empty() && c.embedding.len() != dim)
        {
            anyhow::bail!(
                "mixed vector widths in one batch ({} vs {} for {}) — the index stores one width",
                dim,
                bad.embedding.len(),
                bad.path
            );
        }

        let points: Vec<_> = chunks
            .iter()
            .filter(|c| !c.embedding.is_empty())
            .map(|c| {
                let vector = VectorStructPersisted::from(VectorStructInternal::Named(
                    HashMap::from([(
                        VECTOR_NAME.to_string(),
                        VectorInternal::from(c.embedding.clone()),
                    )]),
                ));
                let payload = match to_payload(c) {
                    serde_json::Value::Object(map) => Payload(map.into_iter().collect()),
                    _ => unreachable!("to_payload always builds an object"),
                };
                qdrant_edge::PointStructPersisted {
                    id: PointId::Uuid(point_id(&c.id)),
                    vector,
                    payload: Some(payload),
                }
            })
            .collect();

        let shard = self.ensure(dim)?;
        tokio::task::spawn_blocking(move || {
            shard.update(UpdateOperation::PointOperation(
                PointOperations::UpsertPoints(PointInsertOperations::PointsList(points)),
            ))
        })
        .await?
        .map_err(|e| anyhow!("writing to wiki index: {e}"))?;
        Ok(())
    }

    async fn search(
        &self,
        query: &[f32],
        limit: usize,
        min_score: f32,
    ) -> anyhow::Result<Vec<WikiHit>> {
        let Some(shard) = self.snapshot() else {
            return Ok(Vec::new());
        };
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let query = query.to_vec();
        let hits = tokio::task::spawn_blocking(move || {
            let request = SearchRequestBuilder::new(
                QueryEnum::Nearest(NamedQuery {
                    query: VectorInternal::from(query),
                    using: Some(VECTOR_NAME.into()),
                }),
                limit,
            )
            .with_payload(WithPayloadInterface::Bool(true))
            .score_threshold(min_score)
            .build();
            shard.search(request)
        })
        .await?
        .map_err(|e| anyhow!("searching wiki index: {e}"))?;

        Ok(hits
            .into_iter()
            .filter_map(|hit| {
                let value = serde_json::to_value(hit.payload.as_ref()?).ok()?;
                Some(WikiHit {
                    chunk: payload::from_payload(&value)?,
                    score: hit.score,
                })
            })
            .collect())
    }

    async fn indexed(&self) -> anyhow::Result<HashMap<String, IndexedFile>> {
        let Some(shard) = self.snapshot() else {
            return Ok(HashMap::new());
        };
        let rows = tokio::task::spawn_blocking(move || Self::scan(&shard)).await??;
        let mut out: HashMap<String, IndexedFile> = HashMap::new();
        for (_, value) in rows {
            let (Some(path), Some(mtime)) = (
                value.get(F_PATH).and_then(|v| v.as_str()),
                value.get(F_MTIME).and_then(|v| v.as_i64()),
            ) else {
                continue;
            };
            let entry = out.entry(path.to_string()).or_insert(IndexedFile {
                mtime,
                chunks: 0,
            });
            entry.chunks += 1;
            // A file's chunks all carry the same mtime; if a partial re-index
            // left a mix, the oldest is the honest answer — it forces a
            // re-index rather than skipping a half-updated file.
            entry.mtime = entry.mtime.min(mtime);
        }
        Ok(out)
    }

    async fn delete_paths(&self, paths: &[String]) -> anyhow::Result<()> {
        let Some(shard) = self.snapshot() else {
            return Ok(());
        };
        if paths.is_empty() {
            return Ok(());
        }
        let wanted: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();
        let rows = {
            let shard = shard.clone();
            tokio::task::spawn_blocking(move || Self::scan(&shard)).await??
        };
        let ids: Vec<PointId> = rows
            .into_iter()
            .filter(|(_, v)| {
                v.get(F_PATH)
                    .and_then(|p| p.as_str())
                    .is_some_and(|p| wanted.contains(p))
            })
            .map(|(id, _)| id)
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        tokio::task::spawn_blocking(move || {
            shard.update(UpdateOperation::PointOperation(
                PointOperations::DeletePoints { ids },
            ))
        })
        .await?
        .map_err(|e| anyhow!("deleting from wiki index: {e}"))?;
        Ok(())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        let Some(shard) = self.snapshot() else {
            return Ok(0);
        };
        let rows = tokio::task::spawn_blocking(move || Self::scan(&shard)).await??;
        Ok(rows.len())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        // Drop the handle before touching the files: the shard holds a WAL and
        // mmapped segments, and removing those out from under a live handle is
        // how a half-deleted index survives to confuse the next run.
        {
            let mut guard = self
                .shard
                .write()
                .map_err(|_| anyhow!("wiki index lock poisoned"))?;
            *guard = None;
        }
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            // Already gone is the desired state, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        })
        .await?
        .with_context(|| format!("removing wiki index at {}", self.path.display()))?;
        Ok(())
    }

    async fn vector_spec(&self) -> anyhow::Result<Option<(usize, String)>> {
        let Some(shard) = self.snapshot() else {
            return Ok(None);
        };
        let dim = {
            let config = shard.config();
            config.vectors.get(VECTOR_NAME).map(|v| v.size as usize)
        };
        let Some(dim) = dim else { return Ok(None) };
        let rows = tokio::task::spawn_blocking(move || Self::scan(&shard)).await??;
        let model = rows
            .iter()
            .find_map(|(_, v)| v.get(F_MODEL).and_then(|m| m.as_str()))
            .unwrap_or_default()
            .to_string();
        Ok(Some((dim, model)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit vectors, as the `EmbeddingClient` contract guarantees — the shard is
    /// configured for dot-product distance on that basis.
    fn chunk(path: &str, ordinal: usize, embedding: Vec<f32>) -> WikiChunk {
        WikiChunk {
            id: WikiChunk::make_id(path, ordinal),
            path: path.to_string(),
            heading_path: format!("{path} > 节"),
            ordinal,
            text: format!("{path} 第{ordinal}段的正文"),
            mtime: 1780000000 + ordinal as i64,
            embedding,
            embedding_model: "test-model".into(),
        }
    }

    fn index(dir: &tempfile::TempDir) -> EdgeIndex {
        EdgeIndex::open(dir.path(), "wiki").unwrap()
    }

    /// A vault that was never indexed must answer "empty", not error — see the
    /// module docs on lazy creation.
    #[tokio::test]
    async fn uncreated_index_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        assert_eq!(index.count().await.unwrap(), 0);
        assert!(index.search(&[1.0, 0.0], 5, 0.0).await.unwrap().is_empty());
        assert!(index.indexed().await.unwrap().is_empty());
        assert!(index.vector_spec().await.unwrap().is_none());
        index.delete_paths(&["a.md".into()]).await.unwrap();
    }

    #[tokio::test]
    async fn upsert_then_search_returns_the_nearest_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        index
            .upsert(&[
                chunk("a.md", 0, vec![1.0, 0.0]),
                chunk("b.md", 0, vec![0.0, 1.0]),
            ])
            .await
            .unwrap();

        assert_eq!(index.count().await.unwrap(), 2);
        assert_eq!(index.vector_spec().await.unwrap().unwrap().0, 2);

        let hits = index.search(&[1.0, 0.0], 1, 0.0).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk.path, "a.md");
        assert!(hits[0].score > 0.9, "score was {}", hits[0].score);
        // Payload survived the round trip; vectors deliberately did not.
        assert_eq!(hits[0].chunk.heading_path, "a.md > 节");
        assert!(hits[0].chunk.embedding.is_empty());
    }

    /// `min_score` must drop weak neighbours — an unrelated query always has a
    /// nearest point, and returning it is how a search tool invents relevance.
    #[tokio::test]
    async fn min_score_filters_weak_hits() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        index
            .upsert(&[chunk("a.md", 0, vec![1.0, 0.0])])
            .await
            .unwrap();
        // Orthogonal query: dot product 0.
        assert!(index.search(&[0.0, 1.0], 5, 0.5).await.unwrap().is_empty());
        assert_eq!(index.search(&[0.0, 1.0], 5, -1.0).await.unwrap().len(), 1);
    }

    /// Re-indexing an unchanged file must not duplicate its points — this is
    /// what the deterministic UUIDv5 point id buys.
    #[tokio::test]
    async fn upsert_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        let chunks = [chunk("a.md", 0, vec![1.0, 0.0]), chunk("a.md", 1, vec![0.0, 1.0])];
        index.upsert(&chunks).await.unwrap();
        index.upsert(&chunks).await.unwrap();
        assert_eq!(index.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn indexed_groups_by_path_and_delete_removes_a_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        index
            .upsert(&[
                chunk("a.md", 0, vec![1.0, 0.0]),
                chunk("a.md", 1, vec![0.0, 1.0]),
                chunk("b.md", 0, vec![0.0, 1.0]),
            ])
            .await
            .unwrap();

        let indexed = index.indexed().await.unwrap();
        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed["a.md"].chunks, 2);
        assert_eq!(indexed["b.md"].chunks, 1);
        // Oldest mtime wins, so a half-updated file re-indexes.
        assert_eq!(indexed["a.md"].mtime, 1780000000);

        index.delete_paths(&["a.md".into()]).await.unwrap();
        assert_eq!(index.count().await.unwrap(), 1);
        assert!(index.indexed().await.unwrap().contains_key("b.md"));
    }

    /// Changing embedding model changes vector width, and the store cannot take
    /// the new one. This must say so, and say what fixes it.
    #[tokio::test]
    async fn a_different_vector_width_is_rejected_with_a_fix() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        index
            .upsert(&[chunk("a.md", 0, vec![1.0, 0.0])])
            .await
            .unwrap();

        let err = index
            .upsert(&[chunk("b.md", 0, vec![1.0, 0.0, 0.0])])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("2-dim") && err.contains("3-dim"), "{err}");
        assert!(err.contains("--rebuild"), "must name the fix: {err}");
    }

    /// `reset` is what makes a model change possible: after it, an index built
    /// for one width accepts another.
    #[tokio::test]
    async fn reset_allows_a_new_vector_width() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        index
            .upsert(&[chunk("a.md", 0, vec![1.0, 0.0])])
            .await
            .unwrap();
        assert_eq!(index.count().await.unwrap(), 1);

        index.reset().await.unwrap();
        assert_eq!(index.count().await.unwrap(), 0);

        index
            .upsert(&[chunk("a.md", 0, vec![0.0, 1.0, 0.0])])
            .await
            .unwrap();
        assert_eq!(index.vector_spec().await.unwrap().unwrap().0, 3);
        assert_eq!(index.count().await.unwrap(), 1);
    }

    /// Resetting a never-created index is the desired state, not an error.
    #[tokio::test]
    async fn reset_on_an_empty_index_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        index(&dir).reset().await.unwrap();
    }

    /// One index stores one vector width; a mixed batch is a bug upstream and
    /// must be rejected loudly rather than half-written.
    #[tokio::test]
    async fn mixed_vector_widths_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(&dir);
        let err = index
            .upsert(&[
                chunk("a.md", 0, vec![1.0, 0.0]),
                chunk("b.md", 0, vec![1.0, 0.0, 0.0]),
            ])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("mixed vector widths"), "{err}");
    }
}
