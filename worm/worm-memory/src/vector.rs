use std::collections::HashMap;

use crate::error::MemoryError;

/// A single result from a vector similarity search.
#[derive(Debug, Clone)]
pub struct VectorResult {
    pub id: String,
    pub similarity: f32,
}

/// Port for vector storage and similarity search.
#[async_trait::async_trait]
pub trait VectorStorer: Send + Sync {
    async fn upsert(
        &self,
        id: &str,
        content: &str,
        meta: &HashMap<String, String>,
    ) -> Result<(), MemoryError>;
    async fn delete(&self, id: &str) -> Result<(), MemoryError>;
    async fn query(
        &self,
        query_text: &str,
        n_results: usize,
    ) -> Result<Vec<VectorResult>, MemoryError>;
    fn count(&self) -> usize;
}

/// Port for text embedding (e.g. via an LLM provider).
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, MemoryError>;
}

/// Cosine similarity between two unit vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical() {
        let v = vec![0.6, 0.8];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.01);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.01);
    }
}
