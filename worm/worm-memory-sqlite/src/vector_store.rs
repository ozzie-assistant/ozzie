use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use worm_memory::{cosine_similarity, Embedder, MemoryError, VectorResult, VectorStorer};

/// Stores embeddings as BLOBs in SQLite and performs brute-force cosine similarity.
pub struct SqliteVectorStore {
    conn: Mutex<Connection>,
    embedder: Box<dyn Embedder>,
    #[allow(dead_code)]
    dims: usize,
}

impl SqliteVectorStore {
    /// Creates or opens the embeddings table.
    pub fn new(
        conn: Connection,
        embedder: Box<dyn Embedder>,
        dims: usize,
    ) -> Result<Self, MemoryError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_embeddings (
                id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL
            )",
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            embedder,
            dims,
        })
    }

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        let texts = vec![text.to_string()];
        let vectors = self.embedder.embed(&texts).await?;
        if vectors.is_empty() || vectors[0].is_empty() {
            return Err(MemoryError::Other("empty embedding result".to_string()));
        }

        let f64_vec = &vectors[0];
        let norm: f64 = f64_vec.iter().map(|v| v * v).sum::<f64>().sqrt();
        let f32_vec: Vec<f32> = f64_vec
            .iter()
            .map(|v| if norm > 0.0 { (v / norm) as f32 } else { *v as f32 })
            .collect();
        Ok(f32_vec)
    }
}

#[async_trait::async_trait]
impl VectorStorer for SqliteVectorStore {
    async fn upsert(
        &self,
        id: &str,
        content: &str,
        _meta: &HashMap<String, String>,
    ) -> Result<(), MemoryError> {
        let vec = self.embed_text(content).await?;
        let blob = encode_embedding(&vec);
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR REPLACE INTO memory_embeddings(id, embedding) VALUES (?1, ?2)",
            params![id, blob],
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM memory_embeddings WHERE id = ?1", params![id])
            .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }

    async fn query(
        &self,
        query_text: &str,
        n_results: usize,
    ) -> Result<Vec<VectorResult>, MemoryError> {
        let query_vec = self.embed_text(query_text).await?;

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare("SELECT id, embedding FROM memory_embeddings")
            .map_err(|e| MemoryError::Database(e.to_string()))?;
        let mut results: Vec<VectorResult> = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, blob))
            })
            .map_err(|e| MemoryError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .map(|(id, blob)| {
                let vec = decode_embedding(&blob);
                let sim = cosine_similarity(&query_vec, &vec);
                VectorResult { id, similarity: sim }
            })
            .collect();

        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(n_results);
        Ok(results)
    }

    fn count(&self) -> usize {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row("SELECT count(*) FROM memory_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap_or(0)
    }
}

/// Converts float32 slice to little-endian bytes.
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for f in v {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    buf
}

/// Converts little-endian bytes back to float32 slice.
pub fn decode_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let original = vec![1.0f32, -0.5, 0.25, 0.0];
        let blob = encode_embedding(&original);
        let decoded = decode_embedding(&blob);
        assert_eq!(original, decoded);
    }
}
