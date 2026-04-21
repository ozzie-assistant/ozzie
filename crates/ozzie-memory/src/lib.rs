mod consolidation;
pub mod frontmatter;
mod markdown_store;
pub mod page_frontmatter;
mod page_store;
mod sqlite_store;
mod vector_store;

// Re-export types and traits from worm-memory (canonical source of truth).
pub use worm_memory::{
    apply_decay, build_embed_meta, build_embed_text, EmbedJob, Embedder, HybridRetriever,
    ImportanceLevel, KeywordRetriever, MemoryEntry, MemoryEntryMeta, MemoryError, MemoryRetriever,
    MemorySearchEntry, MemoryStore, MemoryType, Pipeline, RetrievedMemory, Store, VectorResult,
    VectorStorer,
};

// Re-export wiki types from worm-memory.
pub use worm_memory::{MemorySchema, PageSearchResult, PageStore, WikiPage};

// Bridge: rusqlite::Error → MemoryError (orphan rule prevents From impl).
pub(crate) fn db_err(e: rusqlite::Error) -> MemoryError {
    MemoryError::Database(e.to_string())
}

// Local modules (still in ozzie-memory, implementing traits against ozzie-core).
pub use consolidation::*;
pub use markdown_store::MarkdownStore;
pub use page_store::MarkdownPageStore;
pub use sqlite_store::*;
pub use vector_store::*;
