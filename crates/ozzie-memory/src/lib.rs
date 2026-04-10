mod consolidation;
mod decay;
mod entry;
pub mod frontmatter;
mod markdown_store;
pub mod page_frontmatter;
mod page_store;
mod pipeline;
mod retriever;
mod sqlite_store;
mod vector_store;

pub use consolidation::*;
pub use decay::*;
pub use entry::*;
pub use markdown_store::MarkdownStore;
pub use page_store::MarkdownPageStore;
pub use pipeline::*;
pub use retriever::*;
pub use sqlite_store::*;
pub use vector_store::*;

/// Store defines the interface for memory persistence.
#[async_trait::async_trait]
pub trait Store: Send + Sync {
    async fn create(&self, entry: &mut MemoryEntry, content: &str) -> Result<(), MemoryError>;
    async fn get(&self, id: &str) -> Result<(MemoryEntry, String), MemoryError>;
    async fn update(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError>;
    async fn delete(&self, id: &str) -> Result<(), MemoryError>;
    async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError>;
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("{0}")]
    Other(String),
}

impl From<rusqlite::Error> for MemoryError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}
