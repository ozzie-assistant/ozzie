use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A synthesized wiki page grouping related memory entries.
///
/// Pages are created by the dream consolidation pipeline, not by the agent
/// in real-time. They provide thematic summaries that the retriever can
/// return instead of (or alongside) individual memory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    /// Unique page ID, e.g. "page_rust-tooling".
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// URL-safe slug, used as filename stem in markdown SsoT.
    pub slug: String,
    /// Thematic tags for search and clustering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// IDs of source `MemoryEntry` items this page synthesizes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Monotonically increasing revision counter.
    #[serde(default = "default_revision")]
    pub revision: u32,
}

fn default_revision() -> u32 {
    1
}

/// Metadata-only result from a page text search.
#[derive(Debug, Clone, Serialize)]
pub struct PageSearchResult {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub tags: Vec<String>,
}
