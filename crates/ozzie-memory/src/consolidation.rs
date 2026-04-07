use crate::{
    ImportanceLevel, MemoryEntry, MemoryError, MemoryType, SqliteStore, Store, VectorStorer,
    build_embed_text,
};
use chrono::Utc;
use tracing::{info, warn};

const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.85;

/// LLM-based text summarizer for consolidation.
#[async_trait::async_trait]
pub trait LlmSummarizer: Send + Sync {
    async fn summarize(&self, prompt: &str) -> Result<String, MemoryError>;
}

/// Configuration for the Consolidator.
pub struct ConsolidatorConfig {
    pub store: SqliteStore,
    pub vector: Box<dyn VectorStorer>,
    pub summarizer: Box<dyn LlmSummarizer>,
    /// Cosine similarity threshold for merge candidates (default: 0.85).
    pub threshold: f64,
}

/// Results of a consolidation run.
#[derive(Debug, Default)]
pub struct ConsolidateStats {
    pub checked: usize,
    pub merged: usize,
    pub errors: usize,
}

/// Merges similar memories using LLM summarization.
pub struct Consolidator {
    store: SqliteStore,
    vector: Box<dyn VectorStorer>,
    summarizer: Box<dyn LlmSummarizer>,
    threshold: f64,
}

impl Consolidator {
    pub fn new(cfg: ConsolidatorConfig) -> Self {
        let threshold = if cfg.threshold <= 0.0 {
            DEFAULT_SIMILARITY_THRESHOLD
        } else {
            cfg.threshold
        };
        Self {
            store: cfg.store,
            vector: cfg.vector,
            summarizer: cfg.summarizer,
            threshold,
        }
    }

    /// Finds and merges similar memory clusters.
    pub async fn run(&self) -> Result<ConsolidateStats, MemoryError> {
        let entries = self.store.list().await?;
        let mut stats = ConsolidateStats {
            checked: entries.len(),
            ..Default::default()
        };
        let mut merged_ids = std::collections::HashSet::new();

        for entry in &entries {
            if merged_ids.contains(&entry.id) {
                continue;
            }

            let (_, content) = self.store.get(&entry.id).await?;
            let text = build_embed_text(entry, &content);
            let results = match self.vector.query(&text, 5).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Collect candidates above threshold
            let candidates: Vec<String> = results
                .iter()
                .filter(|r| {
                    r.id != entry.id
                        && !merged_ids.contains(&r.id)
                        && (r.similarity as f64) >= self.threshold
                })
                .map(|r| r.id.clone())
                .collect();

            if candidates.is_empty() {
                continue;
            }

            let mut group = vec![entry.id.clone()];
            group.extend(candidates);

            match self.merge_group(&group).await {
                Ok(_) => {
                    for id in &group {
                        merged_ids.insert(id.clone());
                    }
                    stats.merged += 1;
                }
                Err(e) => {
                    warn!(group = ?group, error = %e, "consolidate: merge failed");
                    stats.errors += 1;
                }
            }
        }

        Ok(stats)
    }

    async fn merge_group(&self, ids: &[String]) -> Result<(), MemoryError> {
        let mut memory_texts = Vec::new();
        for id in ids {
            if let Ok((entry, content)) = self.store.get(id).await {
                memory_texts.push(format!(
                    "[{}] {} (type: {}, tags: {})\n{}",
                    entry.id,
                    entry.title,
                    entry.memory_type.as_str(),
                    entry.tags.join(", "),
                    content
                ));
            }
        }

        if memory_texts.len() < 2 {
            return Ok(());
        }

        let prompt = format!(
            "You are merging similar memories into a single consolidated entry.\n\n\
             Source memories:\n{}\n\n\
             Create a single merged memory that combines all the information.\n\
             Respond with JSON:\n\
             {{\n  \"title\": \"concise merged title\",\n  \"content\": \"complete merged content in markdown\",\n  \"tags\": [\"merged\", \"tag1\", \"tag2\"],\n  \"type\": \"preference|fact|procedure|context\"\n}}",
            memory_texts.join("\n---\n")
        );

        let response = self.summarizer.summarize(&prompt).await?;
        let response = strip_code_fences(&response);

        #[derive(serde::Deserialize)]
        struct MergedResult {
            title: String,
            content: String,
            tags: Vec<String>,
            #[serde(rename = "type")]
            memory_type: String,
        }

        let merged: MergedResult = serde_json::from_str(&response)
            .map_err(|e| MemoryError::Other(format!("parse merge result: {e}")))?;

        let mut new_entry = MemoryEntry {
            id: String::new(),
            title: merged.title,
            source: "consolidation".to_string(),
            memory_type: merged.memory_type.parse().unwrap_or(MemoryType::Fact),
            tags: merged.tags,
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };

        self.store.create(&mut new_entry, &merged.content).await?;

        // Mark source entries as merged
        let now = Utc::now();
        for id in ids {
            if let Ok((mut entry, content)) = self.store.get(id).await {
                entry.merged_into = Some(new_entry.id.clone());
                entry.updated_at = now;
                if let Err(e) = self.store.update(&entry, &content).await {
                    warn!(id = %id, error = %e, "failed to mark memory as merged");
                }
            }
        }

        info!(sources = ?ids, target = %new_entry.id, title = %new_entry.title, "consolidated memories");
        Ok(())
    }
}

/// Strips markdown code fences from LLM output.
fn strip_code_fences(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json")
        && let Some(content) = rest.strip_suffix("```")
    {
        return content.trim().to_string();
    }
    if let Some(rest) = s.strip_prefix("```")
        && let Some(content) = rest.strip_suffix("```")
    {
        return content.trim().to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_fences_json() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_fences_plain() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_fences_none() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), "{\"key\": \"value\"}");
    }
}
