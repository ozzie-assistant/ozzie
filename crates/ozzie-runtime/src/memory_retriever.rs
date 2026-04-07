//! Memory retriever — bridges the MemoryStore domain port to the MemoryRetriever trait.

use std::sync::Arc;

use ozzie_core::domain::{MemoryError, MemoryRetriever, MemoryStore, RetrievedMemory};

/// FTS-based memory retriever backed by any `MemoryStore` implementation.
pub struct FtsMemoryRetriever {
    store: Arc<dyn MemoryStore>,
}

impl FtsMemoryRetriever {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl MemoryRetriever for FtsMemoryRetriever {
    async fn retrieve(
        &self,
        query: &str,
        _tags: &[String],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>, MemoryError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Build FTS query with prefix matching: each word becomes "word*"
        // and terms are OR-ed for broader recall.
        let fts_query = build_fts_query(query);

        let entries = self.store.search_text(&fts_query, limit).await?;

        let mut results = Vec::new();
        for entry in entries {
            let content = match self.store.get_content(&entry.id).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            results.push(RetrievedMemory {
                id: entry.id,
                title: entry.title,
                memory_type: entry.memory_type,
                content,
                score: 1.0,
                tags: entry.tags,
            });
        }

        Ok(results)
    }
}

/// Converts a natural language query into an FTS5 query with prefix matching.
///
/// "Comment on deploie en prod" → "deploie* OR prod*"
/// Stop words (< 3 chars, common French/English) are filtered out.
fn build_fts_query(query: &str) -> String {
    const STOP_WORDS: &[&str] = &[
        "le", "la", "les", "un", "une", "des", "de", "du", "en", "et", "ou", "on", "ne", "pas",
        "ce", "se", "sa", "son", "ses", "au", "aux", "par", "pour", "avec", "dans", "sur", "est",
        "the", "is", "are", "was", "be", "to", "of", "and", "or", "in", "on", "at", "by", "for",
        "an", "it", "do", "we", "our", "how", "what", "qui", "que", "comment", "quel", "quelle",
        "nous", "vous", "ils", "chez",
    ];

    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .map(|w| format!("{w}*"))
        .collect();

    if tokens.is_empty() {
        // Fallback: use the raw query
        return query.to_string();
    }

    tokens.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_basic() {
        let q = build_fts_query("Comment on deploie en prod");
        assert!(q.contains("deploie*"));
        assert!(q.contains("prod*"));
        assert!(!q.contains("comment*")); // stop word
        assert!(!q.contains("on*")); // too short
    }

    #[test]
    fn fts_query_empty() {
        assert_eq!(build_fts_query(""), "");
    }

    #[test]
    fn fts_query_all_stop_words() {
        let q = build_fts_query("le la les");
        // Fallback to raw query
        assert_eq!(q, "le la les");
    }

    #[test]
    fn fts_query_strips_special_chars() {
        let q = build_fts_query("Est-ce que tu te souviens d'une clé de benchmark ?");
        // Apostrophe and hyphens must be stripped to avoid FTS5 parse errors
        assert!(q.contains("estce*")); // hyphen removed
        assert!(q.contains("souviens*"));
        assert!(q.contains("dune*")); // apostrophe removed
        assert!(q.contains("clé*"));
        assert!(q.contains("benchmark*"));
        assert!(!q.contains("'")); // no raw apostrophe
        assert!(!q.contains("-")); // no raw hyphen
        assert!(!q.contains("?")); // no question mark
    }
}
