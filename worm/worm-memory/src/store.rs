use chrono::{DateTime, Utc};

use crate::entry::MemoryEntry;
use crate::error::MemoryError;

/// Full CRUD interface for memory persistence.
///
/// Implemented by storage backends (SQLite, in-memory, etc.).
#[async_trait::async_trait]
pub trait Store: Send + Sync {
    async fn create(&self, entry: &mut MemoryEntry, content: &str) -> Result<(), MemoryError>;
    async fn get(&self, id: &str) -> Result<(MemoryEntry, String), MemoryError>;
    async fn update(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError>;
    async fn delete(&self, id: &str) -> Result<(), MemoryError>;
    async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError>;
}

/// Read-side port for memory storage (FTS + content retrieval).
///
/// Used by tools and the gateway API. Separate from [`Store`] to avoid
/// exposing mutation methods to read-only consumers.
#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync {
    /// Full-text search returning metadata (no content).
    async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemorySearchEntry>, MemoryError>;
    /// Fetches full content for a specific memory entry.
    async fn get_content(&self, id: &str) -> Result<String, MemoryError>;
    /// Lists all active memory entries (metadata only).
    async fn list_entries(&self) -> Result<Vec<MemoryEntryMeta>, MemoryError> {
        Ok(Vec::new())
    }
    /// Fetches full entry metadata + content by ID.
    async fn get_entry(&self, id: &str) -> Result<(MemoryEntryMeta, String), MemoryError> {
        let content = self.get_content(id).await?;
        Ok((
            MemoryEntryMeta {
                id: id.to_string(),
                title: String::new(),
                memory_type: String::new(),
                tags: Vec::new(),
                source: String::new(),
                importance: "normal".to_string(),
                confidence: 0.0,
                created_at: DateTime::<Utc>::default(),
                updated_at: DateTime::<Utc>::default(),
            },
            content,
        ))
    }
}

/// A single result from a text-based memory search (metadata only, no content).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemorySearchEntry {
    pub id: String,
    pub title: String,
    pub memory_type: String,
    pub tags: Vec<String>,
}

/// A full memory entry with metadata (no content).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryEntryMeta {
    pub id: String,
    pub title: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub source: String,
    pub importance: String,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
