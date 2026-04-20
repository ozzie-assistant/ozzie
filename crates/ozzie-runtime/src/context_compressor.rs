//! Context compressor — wraps the layered context Manager as a ContextCompressor port.

use std::path::Path;
use std::sync::Arc;

use ozzie_core::domain::{CompressionError, ContextCompressor, Message};
use ozzie_core::layered::{Config, FallbackSummarizer, Manager};

use crate::layered_store::FileArchiveStore;

/// Context compressor backed by the layered context Manager (L0/L1/L2 + BM25).
///
/// Pass to `EventRunnerConfig::compressor` to enable automatic history compression
/// once the session exceeds `cfg.max_recent_messages`.
///
/// Uses the heuristic fallback summarizer (no LLM call). To plug in an LLM
/// summarizer, build the `Manager` manually via `Manager::new(store, cfg, summarizer)`
/// and wrap it with `LayeredContextCompressor::from_manager`.
pub struct LayeredContextCompressor {
    manager: Manager,
}

impl LayeredContextCompressor {
    /// Creates a compressor using the heuristic fallback summarizer.
    pub fn new(sessions_dir: &Path, cfg: Config) -> Self {
        let store = Box::new(FileArchiveStore::new(sessions_dir));
        Self {
            manager: Manager::new(store, cfg, Arc::new(FallbackSummarizer)),
        }
    }

    /// Creates a compressor from a pre-built Manager (e.g. with LLM summarizer).
    pub fn from_manager(manager: Manager) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl ContextCompressor for LayeredContextCompressor {
    async fn compress(
        &self,
        session_id: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, CompressionError> {
        let layered_msgs = ozzie_core::layered::to_layered_messages(history);
        let (result_msgs, _stats) = self
            .manager
            .apply(session_id, &layered_msgs)
            .await
            .map_err(|e| CompressionError::Other(e.to_string()))?;

        // Convert back to domain messages
        Ok(result_msgs
            .into_iter()
            .map(|m| Message::new(m.role, m.content))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_history_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let compressor = LayeredContextCompressor::new(dir.path(), Config::default());

        let history: Vec<Message> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("question {i}"))
                } else {
                    Message::assistant(format!("answer {i}"))
                }
            })
            .collect();

        let compressed = compressor.compress("sess_test", &history).await.unwrap();
        assert_eq!(compressed.len(), history.len());
    }

    #[tokio::test]
    async fn long_history_compressed() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            max_recent_messages: 4,
            archive_chunk_size: 4,
            ..Config::default()
        };
        let compressor = LayeredContextCompressor::new(dir.path(), cfg);

        let history: Vec<Message> = (0..20)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("question {i} about rust and memory management"))
                } else {
                    Message::assistant(format!("answer {i} about ownership and lifetimes"))
                }
            })
            .collect();

        let compressed = compressor.compress("sess_test", &history).await.unwrap();
        assert!(
            compressed.len() < history.len(),
            "compressed {} should be < original {}",
            compressed.len(),
            history.len()
        );
    }
}
