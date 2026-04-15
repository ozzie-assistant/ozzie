use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

const RECORDS_FILE: &str = "workspace_records.json";

/// Tracks consolidation progress for a project workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub project_name: String,
    /// Git commit SHA at last scan (watermark).
    pub last_commit: String,
    /// Memory entry IDs created from this workspace.
    pub memory_ids: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

/// Persists `WorkspaceRecord`s as a JSON file in `$OZZIE_PATH`.
pub struct WorkspaceRecordStore {
    path: PathBuf,
}

impl WorkspaceRecordStore {
    pub fn new(ozzie_path: &Path) -> Self {
        Self {
            path: ozzie_path.join(RECORDS_FILE),
        }
    }

    /// Loads all records, keyed by project name.
    pub fn load_all(&self) -> HashMap<String, WorkspaceRecord> {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                warn!(error = %e, "corrupt workspace records file, starting fresh");
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        }
    }

    /// Saves a single record (upserts by project name).
    pub fn save(&self, record: &WorkspaceRecord) -> anyhow::Result<()> {
        let mut all = self.load_all();
        all.insert(record.project_name.clone(), record.clone());
        let json = serde_json::to_string_pretty(&all)?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceRecordStore::new(dir.path());

        let record = WorkspaceRecord {
            project_name: "coaching".to_string(),
            last_commit: "abc123".to_string(),
            memory_ids: vec!["mem_1".to_string()],
            updated_at: Utc::now(),
        };
        store.save(&record).unwrap();

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["coaching"].last_commit, "abc123");
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceRecordStore::new(dir.path());
        assert!(store.load_all().is_empty());
    }

    #[test]
    fn upserts_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceRecordStore::new(dir.path());

        let mut record = WorkspaceRecord {
            project_name: "coaching".to_string(),
            last_commit: "abc".to_string(),
            memory_ids: vec![],
            updated_at: Utc::now(),
        };
        store.save(&record).unwrap();

        record.last_commit = "def456".to_string();
        record.memory_ids.push("mem_2".to_string());
        store.save(&record).unwrap();

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["coaching"].last_commit, "def456");
        assert_eq!(loaded["coaching"].memory_ids.len(), 1);
    }
}
