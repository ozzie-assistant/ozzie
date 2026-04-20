use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::MemoryError;

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

/// Domain port for wiki page storage (synthesized thematic pages).
#[async_trait::async_trait]
pub trait PageStore: Send + Sync {
    /// Creates or updates a page. On update, bumps revision automatically.
    async fn upsert(&self, page: &mut WikiPage, content: &str) -> Result<(), MemoryError>;
    /// Fetches a page by ID.
    async fn get(&self, id: &str) -> Result<(WikiPage, String), MemoryError>;
    /// Fetches a page by slug.
    async fn get_by_slug(&self, slug: &str) -> Result<(WikiPage, String), MemoryError>;
    /// Deletes a page by ID.
    async fn delete(&self, id: &str) -> Result<(), MemoryError>;
    /// Lists all pages (metadata only).
    async fn list(&self) -> Result<Vec<WikiPage>, MemoryError>;
    /// Full-text search returning metadata (no content).
    async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PageSearchResult>, MemoryError>;
}

/// Governance rules for wiki page synthesis.
///
/// Loaded from `$OZZIE_PATH/memory_schema.md` (markdown with YAML frontmatter).
/// When the file is absent, defaults are used.
#[derive(Debug, Clone)]
pub struct MemorySchema {
    /// Max content length in chars before triggering a page split.
    pub max_page_chars: usize,
    /// Language directive for page content (e.g. "fr", "en").
    pub language: Option<String>,
    /// Extra instructions injected into synthesis prompts (the markdown body).
    pub instructions: String,
}

const DEFAULT_MAX_PAGE_CHARS: usize = 6000;

impl Default for MemorySchema {
    fn default() -> Self {
        Self {
            max_page_chars: DEFAULT_MAX_PAGE_CHARS,
            language: None,
            instructions: String::new(),
        }
    }
}

impl MemorySchema {
    /// Loads the schema from `$OZZIE_PATH/memory_schema.md`.
    /// Returns defaults if the file doesn't exist.
    pub fn load(ozzie_path: &Path) -> Self {
        let path = ozzie_path.join("memory_schema.md");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                debug!("no memory_schema.md found, using defaults");
                return Self::default();
            }
        };

        parse_schema(&text).unwrap_or_default()
    }
}

fn parse_schema(text: &str) -> Option<MemorySchema> {
    let (yaml, body) = split_frontmatter(text)?;

    let mut max_page_chars = DEFAULT_MAX_PAGE_CHARS;
    let mut language = None;

    for line in yaml.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "max_page_chars" => {
                    if let Ok(n) = value.parse::<usize>() {
                        max_page_chars = n;
                    }
                }
                "language" if !value.is_empty() => {
                    language = Some(value.to_string());
                }
                _ => {}
            }
        }
    }

    Some(MemorySchema {
        max_page_chars,
        language,
        instructions: body.trim().to_string(),
    })
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if !text.starts_with("---") {
        return Some(("", text));
    }
    let after_first = &text[3..].trim_start_matches(['\r', '\n']);
    let end = after_first.find("\n---")?;
    let yaml = &after_first[..end];
    let body = after_first[end + 4..].trim_start_matches(['\r', '\n']);
    Some((yaml, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_schema() {
        let text = "---\nmax_page_chars: 4000\nlanguage: fr\n---\n\n## Conventions\n\nUse headers.\n";
        let schema = parse_schema(text).unwrap();
        assert_eq!(schema.max_page_chars, 4000);
        assert_eq!(schema.language.as_deref(), Some("fr"));
        assert!(schema.instructions.contains("Use headers."));
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let text = "---\nlanguage: en\n---\n\nKeep it simple.";
        let schema = parse_schema(text).unwrap();
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert_eq!(schema.language.as_deref(), Some("en"));
        assert_eq!(schema.instructions, "Keep it simple.");
    }

    #[test]
    fn parse_no_frontmatter() {
        let text = "Just some instructions.";
        let schema = parse_schema(text).unwrap();
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert!(schema.language.is_none());
        assert_eq!(schema.instructions, "Just some instructions.");
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let schema = MemorySchema::load(Path::new("/nonexistent"));
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert!(schema.language.is_none());
        assert!(schema.instructions.is_empty());
    }
}
