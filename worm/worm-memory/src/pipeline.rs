use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::warn;

use crate::entry::MemoryEntry;
use crate::store::Store;
use crate::vector::VectorStorer;

/// A single embedding task.
#[derive(Debug)]
pub struct EmbedJob {
    pub id: String,
    pub content: String,
    pub meta: std::collections::HashMap<String, String>,
    pub delete: bool,
}

/// Asynchronous embedding job pipeline with a single worker.
pub struct Pipeline {
    tx: mpsc::Sender<EmbedJob>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Pipeline {
    /// Creates and starts a new embedding pipeline.
    pub fn new<S: Store + 'static>(
        vector: Arc<RwLock<Option<Box<dyn VectorStorer>>>>,
        store: Arc<S>,
        model_name: String,
        queue_size: usize,
    ) -> Self {
        let queue_size = if queue_size == 0 { 100 } else { queue_size };
        let (tx, mut rx) = mpsc::channel::<EmbedJob>(queue_size);

        let handle = tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let guard = vector.read().await;
                let Some(vs) = guard.as_ref() else {
                    continue;
                };

                if job.delete {
                    if let Err(e) = vs.delete(&job.id).await {
                        warn!(id = %job.id, error = %e, "embedding pipeline: delete failed");
                    }
                    continue;
                }

                if let Err(e) = vs.upsert(&job.id, &job.content, &job.meta).await {
                    warn!(id = %job.id, error = %e, "embedding pipeline: upsert failed");
                    continue;
                }

                // Mark entry as indexed
                if let Ok((mut entry, content)) = store.get(&job.id).await {
                    entry.embedding_model = model_name.clone();
                    entry.indexed_at = Some(chrono::Utc::now());
                    if let Err(e) = store.update(&entry, &content).await {
                        warn!(id = %job.id, error = %e, "failed to mark entry as indexed");
                    }
                }
            }
        });

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Enqueues a job. Non-blocking: drops the job if the queue is full.
    pub fn enqueue(&self, job: EmbedJob) {
        if self.tx.try_send(job).is_err() {
            warn!("embedding pipeline queue full, dropping job");
        }
    }

    /// Stops the pipeline and waits for completion.
    pub async fn stop(&mut self) {
        drop(self.tx.clone());
        if let Some(handle) = self.handle.take()
            && let Err(e) = handle.await
        {
            warn!(error = %e, "embedding pipeline task panicked");
        }
    }
}

/// Formats a memory entry for embedding.
/// Format: "Title [tag1, tag2]\ncontent"
pub fn build_embed_text(entry: &MemoryEntry, content: &str) -> String {
    let mut s = entry.title.clone();
    if !entry.tags.is_empty() {
        s.push_str(&format!(" [{}]", entry.tags.join(", ")));
    }
    s.push('\n');
    s.push_str(content);
    s
}

/// Extracts metadata from a memory entry for vector storage.
pub fn build_embed_meta(entry: &MemoryEntry) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    m.insert("type".to_string(), entry.memory_type.as_str().to_string());
    m.insert("source".to_string(), entry.source.clone());
    m.insert("title".to_string(), entry.title.clone());
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{ImportanceLevel, MemoryType};

    #[test]
    fn build_embed_text_with_tags() {
        let entry = MemoryEntry {
            id: "mem_test".to_string(),
            title: "Test Title".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec!["rust".to_string(), "coding".to_string()],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };
        let text = build_embed_text(&entry, "Some content");
        assert_eq!(text, "Test Title [rust, coding]\nSome content");
    }

    #[test]
    fn build_embed_text_no_tags() {
        let entry = MemoryEntry {
            id: "mem_test".to_string(),
            title: "Title".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };
        let text = build_embed_text(&entry, "Content");
        assert_eq!(text, "Title\nContent");
    }
}
