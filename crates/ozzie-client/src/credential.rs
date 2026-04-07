use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// A stored credential for authenticating with the Ozzie gateway.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    pub device_id: String,
    pub token: String,
    pub gateway_url: String,
}

/// Error from credential store operations.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("io error: {0}")]
    Io(String),
}

/// Stores and retrieves device credentials, keyed by gateway URL.
pub trait CredentialStore: Send + Sync {
    fn save(&self, cred: &Credential) -> Result<(), CredentialError>;
    fn load(&self, gateway_url: &str) -> Result<Option<Credential>, CredentialError>;
    fn clear(&self, gateway_url: &str) -> Result<(), CredentialError>;
}

/// File-backed credential store (persisted to disk).
///
/// Stores a JSON map of gateway URL → credential so one installation can
/// hold tokens for multiple gateways simultaneously.
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default credential path: `$OZZIE_PATH/.credential.json` or `~/.ozzie/.credential.json`.
    pub fn default_path() -> PathBuf {
        let ozzie_path = std::env::var("OZZIE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".ozzie")
            });
        ozzie_path.join(".credential.json")
    }

    fn read_map(&self) -> Result<HashMap<String, Credential>, CredentialError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let json = std::fs::read_to_string(&self.path)
            .map_err(|e| CredentialError::Io(e.to_string()))?;
        let map: HashMap<String, Credential> = serde_json::from_str(&json)
            .map_err(|e| CredentialError::Io(format!("parse: {e}")))?;
        Ok(map)
    }

    fn write_map(&self, map: &HashMap<String, Credential>) -> Result<(), CredentialError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CredentialError::Io(e.to_string()))?;
        }
        let json = serde_json::to_string_pretty(map)
            .map_err(|e| CredentialError::Io(e.to_string()))?;
        std::fs::write(&self.path, json).map_err(|e| CredentialError::Io(e.to_string()))
    }
}

impl CredentialStore for FileCredentialStore {
    fn save(&self, cred: &Credential) -> Result<(), CredentialError> {
        let mut map = self.read_map()?;
        map.insert(cred.gateway_url.clone(), cred.clone());
        self.write_map(&map)
    }

    fn load(&self, gateway_url: &str) -> Result<Option<Credential>, CredentialError> {
        let map = self.read_map()?;
        Ok(map.get(gateway_url).cloned())
    }

    fn clear(&self, gateway_url: &str) -> Result<(), CredentialError> {
        let mut map = self.read_map()?;
        map.remove(gateway_url);
        self.write_map(&map)
    }
}

/// In-memory credential store (for tests or ephemeral use).
pub struct MemoryCredentialStore {
    creds: Mutex<HashMap<String, Credential>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self {
            creds: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn save(&self, cred: &Credential) -> Result<(), CredentialError> {
        self.creds.lock().unwrap_or_else(|e| e.into_inner()).insert(cred.gateway_url.clone(), cred.clone());
        Ok(())
    }

    fn load(&self, gateway_url: &str) -> Result<Option<Credential>, CredentialError> {
        Ok(self.creds.lock().unwrap_or_else(|e| e.into_inner()).get(gateway_url).cloned())
    }

    fn clear(&self, gateway_url: &str) -> Result<(), CredentialError> {
        self.creds.lock().unwrap_or_else(|e| e.into_inner()).remove(gateway_url);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryCredentialStore::new();
        assert!(store.load("http://localhost:18420").unwrap().is_none());

        store
            .save(&Credential {
                device_id: "dev_1".to_string(),
                token: "tok_abc".to_string(),
                gateway_url: "http://localhost:18420".to_string(),
            })
            .unwrap();

        let loaded = store.load("http://localhost:18420").unwrap().unwrap();
        assert_eq!(loaded.device_id, "dev_1");
        assert_eq!(loaded.token, "tok_abc");
    }

    #[test]
    fn memory_store_clear() {
        let store = MemoryCredentialStore::new();
        store
            .save(&Credential {
                device_id: "dev_1".to_string(),
                token: "tok_abc".to_string(),
                gateway_url: "http://localhost:18420".to_string(),
            })
            .unwrap();
        store.clear("http://localhost:18420").unwrap();
        assert!(store.load("http://localhost:18420").unwrap().is_none());
    }

    #[test]
    fn memory_store_multi_gateway() {
        let store = MemoryCredentialStore::new();
        store.save(&Credential { device_id: "d1".into(), token: "t1".into(), gateway_url: "http://gw1".into() }).unwrap();
        store.save(&Credential { device_id: "d2".into(), token: "t2".into(), gateway_url: "http://gw2".into() }).unwrap();

        assert_eq!(store.load("http://gw1").unwrap().unwrap().token, "t1");
        assert_eq!(store.load("http://gw2").unwrap().unwrap().token, "t2");

        store.clear("http://gw1").unwrap();
        assert!(store.load("http://gw1").unwrap().is_none());
        assert!(store.load("http://gw2").unwrap().is_some());
    }

    #[test]
    fn file_store_roundtrip() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ozzie_cred_test_{unique}.json"));
        let store = FileCredentialStore::new(&path);

        assert!(store.load("http://localhost:18420").unwrap().is_none());

        store
            .save(&Credential {
                device_id: "dev_2".to_string(),
                token: "tok_xyz".to_string(),
                gateway_url: "http://localhost:18420".to_string(),
            })
            .unwrap();

        let loaded = store.load("http://localhost:18420").unwrap().unwrap();
        assert_eq!(loaded.device_id, "dev_2");
        assert_eq!(loaded.token, "tok_xyz");

        store.clear("http://localhost:18420").unwrap();
        assert!(store.load("http://localhost:18420").unwrap().is_none());
    }
}
