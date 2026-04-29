use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ozzie_core::domain::DreamRecord;
use tracing::warn;

const RECORDS_FILE: &str = "dream_records.json";

/// Persists `DreamRecord`s as a JSON file in `$OZZIE_PATH`.
pub struct DreamRecordStore {
    path: PathBuf,
}

impl DreamRecordStore {
    pub fn new(ozzie_path: &Path) -> Self {
        Self {
            path: ozzie_path.join(RECORDS_FILE),
        }
    }

    /// Loads all records, keyed by conversation_id.
    pub fn load_all(&self) -> HashMap<String, DreamRecord> {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                warn!(error = %e, "corrupt dream records file, starting fresh");
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        }
    }

    /// Saves a single record (upserts by conversation_id).
    pub fn save(&self, record: &DreamRecord) -> anyhow::Result<()> {
        let mut all = self.load_all();
        all.insert(record.session_id.clone(), record.clone());
        self.write_all(&all)
    }

    fn write_all(&self, records: &HashMap<String, DreamRecord>) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(records)?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = DreamRecordStore::new(dir.path());

        let record = DreamRecord {
            session_id: "sess_test".to_string(),
            consolidated_up_to: 10,
            profile_entries: vec!["user is a developer".to_string()],
            memory_ids: vec!["mem_abc".to_string()],
            updated_at: Utc::now(),
        };

        store.save(&record).unwrap();

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        let got = &loaded["sess_test"];
        assert_eq!(got.consolidated_up_to, 10);
        assert_eq!(got.profile_entries.len(), 1);
        assert_eq!(got.memory_ids.len(), 1);
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = DreamRecordStore::new(dir.path());
        assert!(store.load_all().is_empty());
    }

    #[test]
    fn upserts_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = DreamRecordStore::new(dir.path());

        let mut record = DreamRecord {
            session_id: "sess_a".to_string(),
            consolidated_up_to: 5,
            profile_entries: vec![],
            memory_ids: vec![],
            updated_at: Utc::now(),
        };
        store.save(&record).unwrap();

        record.consolidated_up_to = 15;
        record.profile_entries.push("new entry".to_string());
        store.save(&record).unwrap();

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["sess_a"].consolidated_up_to, 15);
        assert_eq!(loaded["sess_a"].profile_entries.len(), 1);
    }
}
