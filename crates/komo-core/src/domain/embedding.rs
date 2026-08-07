//! Dense vector embeddings for L3 recall.
//!
//! Lexical recall ([`super::memory::recall_score`]) can only match terms that
//! are spelled the same way, so it structurally fails across scripts: a Chinese
//! question and an English memory tokenize into disjoint term sets and can never
//! overlap, no matter how close their meaning. Embeddings are the layer that
//! bridges them — a multilingual model maps "用户用中文沟通" and "User
//! communicates in Chinese." to nearby vectors.
//!
//! The trait is deliberately tiny: recall needs one batch of vectors and a
//! model identifier, nothing else. Embeddings are an *optional* enrichment —
//! every caller must degrade to lexical-only when no backend is configured or a
//! call fails, so recall can never get worse than it was before this layer
//! existed.

use async_trait::async_trait;

/// A backend that turns text into dense vectors.
///
/// Implementations **must return L2-normalized vectors**, so that cosine
/// similarity is a plain dot product ([`cosine`]) and stored vectors stay
/// directly comparable to freshly-embedded queries without re-normalizing on
/// every turn.
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    /// Embed a batch, returning one vector per input **in the same order**.
    /// An empty input slice yields an empty result without a round trip.
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Identifier for the model producing these vectors, stored alongside each
    /// memory's embedding. Vectors from different models are not comparable, so
    /// a change here invalidates stored embeddings rather than silently mixing
    /// two vector spaces — see `Memory::embedding_model`.
    fn model_id(&self) -> &str;
}

/// Cosine similarity of two L2-normalized vectors: their dot product.
///
/// Returns 0.0 for a dimension mismatch or an empty vector rather than
/// panicking — a memory embedded by a different model must simply not match,
/// not take down the turn.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Scale a vector to unit length, in place. No-op for an all-zero vector (which
/// has no direction to preserve). Called by backends so the invariant above
/// holds for everything that reaches the store.
pub fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in vector.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_of_identical_unit_vectors_is_one() {
        let mut a = vec![3.0, 4.0];
        normalize(&mut a);
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_of_orthogonal_vectors_is_zero() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    /// A memory embedded by another model (different dimensionality) must score
    /// as "no match", never panic on the zip.
    #[test]
    fn cosine_of_mismatched_dimensions_is_zero() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn normalize_leaves_zero_vector_alone() {
        let mut v = vec![0.0, 0.0];
        normalize(&mut v);
        assert_eq!(v, vec![0.0, 0.0]);
    }
}
