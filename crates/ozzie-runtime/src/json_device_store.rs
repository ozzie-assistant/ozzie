use std::path::{Path, PathBuf};
use std::sync::RwLock;

use chrono::Utc;

use ozzie_core::domain::{DeviceRecord, DeviceStorage, PairingError};

/// Device pairing store backed by a JSON file (`$OZZIE_PATH/devices.json`).
pub struct JsonDeviceStore {
    records: RwLock<Vec<DeviceRecord>>,
    path: PathBuf,
}

impl JsonDeviceStore {
    pub fn new(dir: &Path) -> Self {
        let path = dir.join("devices.json");
        let records = Self::load_from(&path).unwrap_or_default();
        Self {
            records: RwLock::new(records),
            path,
        }
    }

    fn load_from(path: &Path) -> Result<Vec<DeviceRecord>, PairingError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(path)
            .map_err(|e| PairingError::Io(format!("read devices: {e}")))?;
        serde_json::from_str(&data)
            .map_err(|e| PairingError::Io(format!("parse devices: {e}")))
    }

    fn save_to(path: &Path, records: &[DeviceRecord]) -> Result<(), PairingError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PairingError::Io(format!("create dir: {e}")))?;
        }
        let data = serde_json::to_string_pretty(records)
            .map_err(|e| PairingError::Io(format!("serialize: {e}")))?;
        std::fs::write(path, data).map_err(|e| PairingError::Io(format!("write: {e}")))
    }
}

impl DeviceStorage for JsonDeviceStore {
    fn add(&self, record: DeviceRecord) -> Result<(), PairingError> {
        let mut records = self.records.write().unwrap();
        records.push(record);
        Self::save_to(&self.path, &records)
    }

    fn verify_token(&self, token: &str) -> Option<DeviceRecord> {
        let records = self.records.read().unwrap();
        records.iter().find(|r| r.token == token).cloned()
    }

    fn list(&self) -> Vec<DeviceRecord> {
        self.records.read().unwrap().clone()
    }

    fn revoke(&self, device_id: &str) -> Result<bool, PairingError> {
        let mut records = self.records.write().unwrap();
        let before = records.len();
        records.retain(|r| r.device_id != device_id);
        let removed = records.len() < before;
        if removed {
            Self::save_to(&self.path, &records)?;
        }
        Ok(removed)
    }

    fn touch(&self, device_id: &str) -> Result<(), PairingError> {
        let mut records = self.records.write().unwrap();
        if let Some(r) = records.iter_mut().find(|r| r.device_id == device_id) {
            r.last_seen = Some(Utc::now());
            Self::save_to(&self.path, &records)?;
            Ok(())
        } else {
            Err(PairingError::NotFound(device_id.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_record(id: &str, token: &str) -> DeviceRecord {
        DeviceRecord {
            device_id: id.to_string(),
            client_type: "tui".to_string(),
            label: Some("Test Device".to_string()),
            token: token.to_string(),
            paired_at: Utc::now(),
            last_seen: None,
        }
    }

    #[test]
    fn add_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonDeviceStore::new(dir.path());
        store.add(new_record("dev_1", "tok_abc")).unwrap();
        let found = store.verify_token("tok_abc").unwrap();
        assert_eq!(found.device_id, "dev_1");
        assert!(store.verify_token("tok_unknown").is_none());
    }

    #[test]
    fn revoke_device() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonDeviceStore::new(dir.path());
        store.add(new_record("dev_1", "tok_abc")).unwrap();
        assert!(store.revoke("dev_1").unwrap());
        assert!(!store.revoke("dev_1").unwrap());
        assert!(store.verify_token("tok_abc").is_none());
    }

    #[test]
    fn touch_updates_last_seen() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonDeviceStore::new(dir.path());
        store.add(new_record("dev_1", "tok_abc")).unwrap();
        store.touch("dev_1").unwrap();
        let rec = store.verify_token("tok_abc").unwrap();
        assert!(rec.last_seen.is_some());
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = JsonDeviceStore::new(dir.path());
            store.add(new_record("dev_1", "tok_abc")).unwrap();
        }
        let store = JsonDeviceStore::new(dir.path());
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].device_id, "dev_1");
    }
}
