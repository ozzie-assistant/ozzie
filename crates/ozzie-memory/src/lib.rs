//! Ozzie memory layer — re-exports from worm-memory (logic) and worm-memory-sqlite (backend).

// Types, traits, and pure logic from worm-memory.
pub use worm_memory::*;

// Re-export frontmatter modules (public for consumer access).
pub use worm_memory::{frontmatter, page_frontmatter};

// SQLite backend implementations.
pub use worm_memory_sqlite::{
    decode_embedding, encode_embedding, MarkdownPageStore, MarkdownStore, SqliteStore,
    SqliteVectorStore,
};
