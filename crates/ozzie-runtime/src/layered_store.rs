//! File-based implementation of `ArchiveStore` for layered context persistence.
//!
//! Layout:
//! ```text
//! {sessions_dir}/{session_id}/layered/index.json
//! {sessions_dir}/{session_id}/layered/archives/archive_{id}.json
//! ```

use std::path::{Path, PathBuf};

use ozzie_core::layered::store::{ArchiveStore, StoreError};
use ozzie_core::layered::{ArchivePayload, Index};

/// File-based archive store, rooted at a sessions directory.
pub struct FileArchiveStore {
    sessions_dir: PathBuf,
}

impl FileArchiveStore {
    /// Creates a store rooted at the given sessions directory.
    pub fn new(sessions_dir: impl Into<PathBuf>) -> Self {
        Self {
            sessions_dir: sessions_dir.into(),
        }
    }

    /// Returns the sessions directory.
    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    fn layered_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(session_id).join("layered")
    }

    fn archives_dir(&self, session_id: &str) -> PathBuf {
        self.layered_dir(session_id).join("archives")
    }

    fn index_path(&self, session_id: &str) -> PathBuf {
        self.layered_dir(session_id).join("index.json")
    }

    fn archive_path(&self, session_id: &str, node_id: &str) -> PathBuf {
        self.archives_dir(session_id)
            .join(format!("archive_{node_id}.json"))
    }

    fn ensure_dirs(&self, session_id: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(self.archives_dir(session_id))
    }
}

impl ArchiveStore for FileArchiveStore {
    fn load_index(&self, session_id: &str) -> Result<Option<Index>, StoreError> {
        let path = self.index_path(session_id);
        match std::fs::read_to_string(&path) {
            Ok(data) => {
                let idx: Index =
                    serde_json::from_str(&data).map_err(|e| StoreError::Parse(e.to_string()))?;
                Ok(Some(idx))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    fn save_index(&self, session_id: &str, idx: &Index) -> Result<(), StoreError> {
        self.ensure_dirs(session_id)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        let data =
            serde_json::to_string_pretty(idx).map_err(|e| StoreError::Parse(e.to_string()))?;

        let path = self.index_path(session_id);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &data).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    fn write_archive(
        &self,
        session_id: &str,
        node_id: &str,
        payload: &ArchivePayload,
    ) -> Result<(), StoreError> {
        self.ensure_dirs(session_id)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        let data = serde_json::to_string_pretty(payload)
            .map_err(|e| StoreError::Parse(e.to_string()))?;

        let path = self.archive_path(session_id, node_id);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &data).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(())
    }

    fn read_archive(
        &self,
        session_id: &str,
        node_id: &str,
    ) -> Result<Option<ArchivePayload>, StoreError> {
        let path = self.archive_path(session_id, node_id);
        match std::fs::read_to_string(&path) {
            Ok(data) => {
                let payload: ArchivePayload =
                    serde_json::from_str(&data).map_err(|e| StoreError::Parse(e.to_string()))?;
                Ok(Some(payload))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    fn cleanup_archives(
        &self,
        session_id: &str,
        valid_node_ids: &[String],
    ) -> Result<(), StoreError> {
        let dir = self.archives_dir(session_id);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StoreError::Io(e.to_string())),
        };

        let valid: std::collections::HashSet<String> = valid_node_ids
            .iter()
            .map(|id| format!("archive_{id}.json"))
            .collect();

        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                && !valid.contains(name)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worm_layered::Message;
    use ozzie_core::layered::{
        Config, Indexer, Node, NodeMetadata, NodeTokenEstimate, Root, fallback_summarizer,
    };

    // ── Store unit tests ─────────────────────────────────────────────

    fn test_index(session_id: &str) -> Index {
        use chrono::Utc;
        Index {
            version: 1,
            session_id: session_id.to_string(),
            root: Root {
                id: "root".to_string(),
                abstract_text: "test abstract".to_string(),
                summary: "test summary".to_string(),
                keywords: vec!["test".to_string()],
                child_ids: vec!["abc123".to_string()],
            },
            nodes: vec![Node {
                id: "abc123".to_string(),
                abstract_text: "node abstract".to_string(),
                summary: "node summary".to_string(),
                resource_path: "archives/archive_abc123.json".to_string(),
                checksum: "abc123def456".to_string(),
                keywords: vec!["rust".to_string()],
                metadata: NodeMetadata {
                    message_count: 8,
                    recency_rank: 0,
                },
                token_estimate: NodeTokenEstimate {
                    abstract_tokens: 5,
                    summary_tokens: 20,
                    transcript_tokens: 100,
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn index_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let idx = test_index("sess_test");

        store.save_index("sess_test", &idx).unwrap();
        let loaded = store.load_index("sess_test").unwrap().unwrap();
        assert_eq!(loaded.session_id, "sess_test");
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.root.child_ids, vec!["abc123"]);
    }

    #[test]
    fn load_nonexistent_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let result = store.load_index("sess_missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn archive_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());

        let payload = ArchivePayload {
            node_id: "abc123".to_string(),
            transcript: "[user]: hello\n[assistant]: hi\n".to_string(),
        };
        store
            .write_archive("sess_test", "abc123", &payload)
            .unwrap();

        let loaded = store.read_archive("sess_test", "abc123").unwrap().unwrap();
        assert_eq!(loaded.node_id, "abc123");
        assert!(loaded.transcript.contains("[user]: hello"));
    }

    #[test]
    fn read_nonexistent_archive() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let result = store.read_archive("sess_test", "missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn cleanup_archives() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());

        for id in &["aaa", "bbb", "ccc"] {
            let payload = ArchivePayload {
                node_id: id.to_string(),
                transcript: "test".to_string(),
            };
            store.write_archive("sess_test", id, &payload).unwrap();
        }

        store
            .cleanup_archives("sess_test", &["aaa".to_string(), "ccc".to_string()])
            .unwrap();

        assert!(store.read_archive("sess_test", "aaa").unwrap().is_some());
        assert!(store.read_archive("sess_test", "bbb").unwrap().is_none());
        assert!(store.read_archive("sess_test", "ccc").unwrap().is_some());
    }

    // ── Indexer integration tests ────────────────────────────────────

    fn make_indexer(dir: &Path) -> Indexer {
        let store = Box::new(FileArchiveStore::new(dir));
        Indexer::new(store, Box::new(fallback_summarizer), Config::default())
    }

    #[test]
    fn indexer_build_creates_index() {
        let dir = tempfile::tempdir().unwrap();
        let indexer = make_indexer(dir.path());

        let msgs: Vec<Message> = (0..16)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("question {i} about rust programming"))
                } else {
                    Message::assistant(format!("answer {i} about rust systems"))
                }
            })
            .collect();

        let index = indexer.build_or_update("sess_test", &msgs).unwrap();
        assert_eq!(index.session_id, "sess_test");
        assert_eq!(index.nodes.len(), 2); // 16 msgs / 8 = 2 chunks
        assert_eq!(index.root.child_ids.len(), 2);
    }

    #[test]
    fn indexer_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let indexer = make_indexer(dir.path());

        let msgs: Vec<Message> = (0..8).map(|i| Message::user(format!("msg{i}"))).collect();

        let idx1 = indexer.build_or_update("sess_test", &msgs).unwrap();
        let idx2 = indexer.build_or_update("sess_test", &msgs).unwrap();

        assert_eq!(idx1.nodes[0].checksum, idx2.nodes[0].checksum);
        assert_eq!(idx1.nodes[0].id, idx2.nodes[0].id);
    }

    #[test]
    fn indexer_incremental() {
        let dir = tempfile::tempdir().unwrap();
        let indexer = make_indexer(dir.path());

        let msgs1: Vec<Message> = (0..8).map(|i| Message::user(format!("msg{i}"))).collect();
        let idx1 = indexer.build_or_update("sess_test", &msgs1).unwrap();
        assert_eq!(idx1.nodes.len(), 1);

        let mut msgs2 = msgs1;
        for i in 8..16 {
            msgs2.push(Message::user(format!("msg{i}")));
        }
        let idx2 = indexer.build_or_update("sess_test", &msgs2).unwrap();
        assert_eq!(idx2.nodes.len(), 2);
        assert_eq!(idx1.nodes[0].id, idx2.nodes[0].id);
    }

    // ── Retriever integration tests ──────────────────────────────────

    fn build_test_index(dir: &Path) -> Index {
        let indexer = make_indexer(dir);

        let msgs: Vec<Message> = (0..32)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!(
                        "Tell me about rust programming and systems design question {i}"
                    ))
                } else {
                    Message::assistant(format!(
                        "Here is information about rust programming and memory safety answer {i}"
                    ))
                }
            })
            .collect();

        indexer.build_or_update("sess_test", &msgs).unwrap()
    }

    #[test]
    fn retrieve_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());

        let index = Index {
            version: 1,
            session_id: "sess_test".to_string(),
            root: Root::default(),
            nodes: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let retriever = ozzie_core::layered::retriever::Retriever::new(&store, Config::default());
        let result = retriever.retrieve("sess_test", &index, "anything");
        assert!(result.selections.is_empty());
    }

    #[test]
    fn retrieve_returns_selections() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let index = build_test_index(dir.path());

        let retriever = ozzie_core::layered::retriever::Retriever::new(&store, Config::default());
        let result = retriever.retrieve("sess_test", &index, "rust programming memory safety");

        assert!(
            !result.selections.is_empty(),
            "should have selections for matching query"
        );
        assert!(result.decision.reached_layer.is_some());
    }

    #[test]
    fn retrieve_budget_respected() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let index = build_test_index(dir.path());

        let cfg = Config {
            max_prompt_tokens: 1000,
            ..Config::default()
        };
        let retriever = ozzie_core::layered::retriever::Retriever::new(&store, cfg);
        let result = retriever.retrieve("sess_test", &index, "rust programming");

        let budget = (1000.0_f64 * 0.45).floor() as usize;
        assert!(
            result.token_usage.used <= budget,
            "used {} > budget {}",
            result.token_usage.used,
            budget
        );
    }

    #[test]
    fn retrieve_unrelated_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileArchiveStore::new(dir.path());
        let index = build_test_index(dir.path());

        let retriever = ozzie_core::layered::retriever::Retriever::new(&store, Config::default());
        let result = retriever.retrieve("sess_test", &index, "cooking recipes for dinner");
        assert!(result.decision.reached_layer.is_some());
    }

    // ── Manager integration tests ────────────────────────────────────

    fn make_history(count: usize) -> Vec<Message> {
        (0..count)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!(
                        "question {i} about rust programming and systems design"
                    ))
                } else {
                    Message::assistant(format!(
                        "answer {i} about memory safety and performance"
                    ))
                }
            })
            .collect()
    }

    fn make_manager(dir: &Path, cfg: Config) -> ozzie_core::layered::Manager {
        let store = Box::new(FileArchiveStore::new(dir));
        ozzie_core::layered::Manager::new(store, cfg, Box::new(fallback_summarizer))
    }

    #[test]
    fn short_history_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = make_manager(dir.path(), Config::default());

        let history = make_history(10);
        let (result, stats) = mgr.apply("sess_test", &history).unwrap();
        assert_eq!(result.len(), 10);
        assert!(stats.is_none());
    }

    #[test]
    fn long_history_compressed() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            max_recent_messages: 4,
            archive_chunk_size: 4,
            ..Config::default()
        };
        let mgr = make_manager(dir.path(), cfg);

        let history = make_history(20);
        let (result, stats) = mgr.apply("sess_test", &history).unwrap();

        let stats = stats.unwrap();
        assert!(stats.nodes > 0, "should have selected nodes");
        assert!(
            result.len() < history.len(),
            "compressed {} should be < original {}",
            result.len(),
            history.len()
        );
    }

    #[test]
    fn apply_creates_index() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            max_recent_messages: 4,
            archive_chunk_size: 4,
            ..Config::default()
        };
        let mgr = make_manager(dir.path(), cfg);

        let history = make_history(20);
        mgr.apply("sess_test", &history).unwrap();

        let check_store = FileArchiveStore::new(dir.path());
        let idx = check_store.load_index("sess_test").unwrap();
        assert!(idx.is_some());
    }
}
