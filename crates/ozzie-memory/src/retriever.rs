use chrono::Utc;
use tracing::warn;

use crate::{MemoryEntry, MemoryError, Store, VectorResult, VectorStorer, apply_decay};

/// A retrieved memory with its relevance score.
#[derive(Debug, Clone)]
pub struct RetrievedMemory {
    pub entry: MemoryEntry,
    pub content: String,
    pub score: f64,
}

/// Retrieves relevant memories for context injection.
#[async_trait::async_trait]
pub trait MemoryRetriever: Send + Sync {
    async fn retrieve(
        &self,
        query: &str,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>, MemoryError>;
}

const KEYWORD_WEIGHT: f64 = 0.3;
const SEMANTIC_WEIGHT: f64 = 0.7;
const MIN_RETRIEVAL_SCORE: f64 = 0.25;

/// Keyword-based memory retrieval with scoring.
pub struct KeywordRetriever<S: Store> {
    store: S,
}

impl<S: Store> KeywordRetriever<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn retrieve(
        &self,
        query: &str,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>, MemoryError> {
        let entries = self.store.list().await?;
        let limit = if limit == 0 { 5 } else { limit };

        let query_words = tokenize(query);
        let tag_set: std::collections::HashSet<String> =
            tags.iter().map(|t| t.to_lowercase()).collect();

        let mut results = Vec::new();
        for entry in entries {
            let score = score_entry(&entry, &query_words, &tag_set);
            if score <= 0.0 {
                continue;
            }
            let (_, content) = self.store.get(&entry.id).await?;
            results.push(RetrievedMemory {
                entry,
                content,
                score,
            });
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }
}

/// Hybrid keyword + semantic retriever.
pub struct HybridRetriever<S: Store> {
    store: S,
    vector: Option<Box<dyn VectorStorer>>,
}

impl<S: Store> HybridRetriever<S> {
    pub fn new(store: S, vector: Option<Box<dyn VectorStorer>>) -> Self {
        Self { store, vector }
    }

    /// Atomically replaces the vector store. Pass None for keyword-only.
    pub fn swap_vector(&mut self, vs: Option<Box<dyn VectorStorer>>) {
        self.vector = vs;
    }
}

#[async_trait::async_trait]
impl<S: Store> MemoryRetriever for HybridRetriever<S> {
    async fn retrieve(
        &self,
        query: &str,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>, MemoryError> {
        let limit = if limit == 0 { 5 } else { limit };
        let fetch_limit = limit * 2;

        // Keyword search
        let keyword_results = keyword_search(&self.store, query, tags, fetch_limit).await?;

        let vector = match &self.vector {
            Some(v) => v,
            None => {
                let mut results = keyword_results;
                results = filter_by_threshold(results);
                results.truncate(limit);
                reinforce_results(&self.store, &results).await;
                return Ok(results);
            }
        };

        // Semantic search
        let semantic_results = match vector.query(query, fetch_limit).await {
            Ok(r) => r,
            Err(_) => {
                // Graceful degradation
                let mut results = keyword_results;
                results = filter_by_threshold(results);
                results.truncate(limit);
                reinforce_results(&self.store, &results).await;
                return Ok(results);
            }
        };

        let mut merged = merge_results(&self.store, &keyword_results, &semantic_results, limit).await;
        merged = filter_by_threshold(merged);
        reinforce_results(&self.store, &merged).await;
        Ok(merged)
    }
}

async fn keyword_search<S: Store>(
    store: &S,
    query: &str,
    tags: &[String],
    limit: usize,
) -> Result<Vec<RetrievedMemory>, MemoryError> {
    let entries = store.list().await?;
    let query_words = tokenize(query);
    let tag_set: std::collections::HashSet<String> =
        tags.iter().map(|t| t.to_lowercase()).collect();

    let mut results = Vec::new();
    for entry in entries {
        let score = score_entry(&entry, &query_words, &tag_set);
        if score <= 0.0 {
            continue;
        }
        let (_, content) = store.get(&entry.id).await?;
        results.push(RetrievedMemory {
            entry,
            content,
            score,
        });
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    Ok(results)
}

async fn merge_results<S: Store>(
    store: &S,
    keyword_results: &[RetrievedMemory],
    semantic_results: &[VectorResult],
    limit: usize,
) -> Vec<RetrievedMemory> {
    use std::collections::HashMap;

    struct Scored {
        keyword_score: f64,
        semantic_score: f64,
    }

    let mut merged: HashMap<String, Scored> = HashMap::new();

    // Normalize keyword scores to [0,1]
    let max_keyword = keyword_results
        .iter()
        .map(|r| r.score)
        .fold(0.0f64, f64::max);
    for r in keyword_results {
        let norm = if max_keyword > 0.0 {
            r.score / max_keyword
        } else {
            0.0
        };
        merged
            .entry(r.entry.id.clone())
            .or_insert(Scored {
                keyword_score: 0.0,
                semantic_score: 0.0,
            })
            .keyword_score = norm;
    }

    // Semantic scores: cosine [-1,1] -> [0,1]
    for r in semantic_results {
        let sim = (r.similarity as f64 + 1.0) / 2.0;
        merged
            .entry(r.id.clone())
            .or_insert(Scored {
                keyword_score: 0.0,
                semantic_score: 0.0,
            })
            .semantic_score = sim;
    }

    // Compute hybrid scores and sort
    let mut scored: Vec<(String, f64)> = merged
        .into_iter()
        .map(|(id, s)| {
            let hybrid = KEYWORD_WEIGHT * s.keyword_score + SEMANTIC_WEIGHT * s.semantic_score;
            (id, hybrid)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    // Load full entries
    let mut out = Vec::new();
    for (id, score) in scored {
        if let Ok((entry, content)) = store.get(&id).await {
            out.push(RetrievedMemory {
                entry,
                content,
                score,
            });
        }
    }
    out
}

async fn reinforce_results<S: Store>(store: &S, results: &[RetrievedMemory]) {
    let now = Utc::now();
    for r in results {
        if let Ok((mut entry, content)) = store.get(&r.entry.id).await {
            entry.confidence =
                apply_decay(entry.confidence, entry.last_used_at, now, entry.importance);
            entry.confidence = (entry.confidence + 0.05).min(1.0);
            entry.last_used_at = now;
            if let Err(e) = store.update(&entry, &content).await {
                warn!(id = %r.entry.id, error = %e, "failed to update memory confidence");
            }
        }
    }
}

fn filter_by_threshold(results: Vec<RetrievedMemory>) -> Vec<RetrievedMemory> {
    results
        .into_iter()
        .filter(|r| r.score >= MIN_RETRIEVAL_SCORE)
        .collect()
}

fn score_entry(
    entry: &MemoryEntry,
    query_words: &[String],
    filter_tags: &std::collections::HashSet<String>,
) -> f64 {
    let mut score = 0.0;

    // Tag match x3
    for tag in &entry.tags {
        let lower = tag.to_lowercase();
        if filter_tags.contains(&lower) {
            score += 3.0;
        }
        for qw in query_words {
            if lower == *qw {
                score += 3.0;
            }
        }
    }

    // Title word match x2
    let title_words = tokenize(&entry.title);
    for tw in &title_words {
        for qw in query_words {
            if tw == qw {
                score += 2.0;
            }
        }
    }

    // Recency bonus
    score += recency_bonus(entry.last_used_at);

    // Confidence multiplier
    score *= entry.confidence.max(0.1);

    score
}

fn recency_bonus(last_used: chrono::DateTime<Utc>) -> f64 {
    let days = Utc::now()
        .signed_duration_since(last_used)
        .num_hours() as f64
        / 24.0;
    match days {
        d if d < 7.0 => 1.0,
        d if d < 30.0 => 0.5,
        _ => 0.1,
    }
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| ".,;:!?\"'()[]{}".contains(c)))
        .filter(|w| w.len() > 1)
        .map(|w| w.to_string())
        .collect()
}
