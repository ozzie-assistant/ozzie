use std::path::{Path, PathBuf};
use std::sync::RwLock;

use ozzie_core::connector::Identity;
use ozzie_core::domain::{PairingError, PairingStorage};
use ozzie_core::policy::{Pairing, PairingKey};

/// Manages identity→policy associations. Thread-safe, persisted as JSON.
pub struct JsonPairingStore {
    pairings: RwLock<Vec<Pairing>>,
    path: PathBuf,
}

impl JsonPairingStore {
    pub fn new(dir: &Path) -> Self {
        let path = dir.join("pairings.json");
        let pairings = Self::load_from(&path).unwrap_or_default();
        Self {
            pairings: RwLock::new(pairings),
            path,
        }
    }

    fn load_from(path: &Path) -> Result<Vec<Pairing>, PairingError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(path)
            .map_err(|e| PairingError::Io(format!("read pairings: {e}")))?;
        serde_json::from_str(&data)
            .map_err(|e| PairingError::Io(format!("parse pairings: {e}")))
    }

    fn save_to(path: &Path, pairings: &[Pairing]) -> Result<(), PairingError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PairingError::Io(format!("create dir: {e}")))?;
        }
        let data = serde_json::to_string_pretty(pairings)
            .map_err(|e| PairingError::Io(format!("serialize: {e}")))?;
        std::fs::write(path, data).map_err(|e| PairingError::Io(format!("write: {e}")))
    }
}

impl PairingStorage for JsonPairingStore {
    fn add(&self, p: &Pairing) -> Result<(), PairingError> {
        let mut pairings = self.pairings.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = pairings.iter_mut().find(|e| e.key == p.key) {
            existing.policy_name = p.policy_name.clone();
        } else {
            pairings.push(p.clone());
        }
        Self::save_to(&self.path, &pairings)
    }

    fn remove(&self, key: &PairingKey) -> Result<bool, PairingError> {
        let mut pairings = self.pairings.write().unwrap_or_else(|e| e.into_inner());
        let before = pairings.len();
        pairings.retain(|p| &p.key != key);
        let removed = pairings.len() < before;
        if removed {
            Self::save_to(&self.path, &pairings)?;
        }
        Ok(removed)
    }

    fn resolve(&self, id: &Identity) -> Option<String> {
        let pairings = self.pairings.read().unwrap_or_else(|e| e.into_inner());
        let candidates = [
            PairingKey {
                platform: id.platform.clone(),
                server_id: id.server_id.clone(),
                user_id: id.user_id.clone(),
            },
            PairingKey {
                platform: id.platform.clone(),
                server_id: id.server_id.clone(),
                user_id: "*".to_string(),
            },
            PairingKey {
                platform: id.platform.clone(),
                server_id: "*".to_string(),
                user_id: "*".to_string(),
            },
        ];
        for candidate in &candidates {
            if let Some(p) = pairings.iter().find(|p| &p.key == candidate) {
                return Some(p.policy_name.clone());
            }
        }
        None
    }

    fn list(&self) -> Vec<Pairing> {
        self.pairings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_identity(platform: &str, server_id: &str, user_id: &str) -> Identity {
        Identity {
            platform: platform.to_string(),
            user_id: user_id.to_string(),
            name: String::new(),
            server_id: server_id.to_string(),
            channel_id: String::new(),
        }
    }

    #[test]
    fn add_and_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        store.add(&Pairing {
            key: PairingKey {
                platform: "discord".to_string(),
                server_id: "s1".to_string(),
                user_id: "u1".to_string(),
            },
            policy_name: "admin".to_string(),
        }).unwrap();
        let id = make_identity("discord", "s1", "u1");
        assert_eq!(store.resolve(&id), Some("admin".to_string()));
    }

    #[test]
    fn wildcard_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        store.add(&Pairing {
            key: PairingKey {
                platform: "discord".to_string(),
                server_id: "s1".to_string(),
                user_id: "*".to_string(),
            },
            policy_name: "support".to_string(),
        }).unwrap();
        let id = make_identity("discord", "s1", "u2");
        assert_eq!(store.resolve(&id), Some("support".to_string()));
    }

    #[test]
    fn exact_takes_priority() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        store.add(&Pairing {
            key: PairingKey {
                platform: "discord".to_string(),
                server_id: "*".to_string(),
                user_id: "*".to_string(),
            },
            policy_name: "readonly".to_string(),
        }).unwrap();
        store.add(&Pairing {
            key: PairingKey {
                platform: "discord".to_string(),
                server_id: "s1".to_string(),
                user_id: "u1".to_string(),
            },
            policy_name: "admin".to_string(),
        }).unwrap();
        let id = make_identity("discord", "s1", "u1");
        assert_eq!(store.resolve(&id), Some("admin".to_string()));
    }

    #[test]
    fn no_match_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        let id = make_identity("slack", "s1", "u1");
        assert_eq!(store.resolve(&id), None);
    }

    #[test]
    fn dm_pairing_resolves_in_guild() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        store.add(&Pairing {
            key: PairingKey {
                platform: "discord".to_string(),
                server_id: String::new(),
                user_id: "u1".to_string(),
            },
            policy_name: "support".to_string(),
        }).unwrap();
        let dm_id = make_identity("discord", "", "u1");
        assert_eq!(store.resolve(&dm_id), Some("support".to_string()));
        let guild_id = make_identity("discord", "guild_1", "u1");
        assert_eq!(store.resolve(&guild_id), None);
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = JsonPairingStore::new(dir.path());
            store.add(&Pairing {
                key: PairingKey {
                    platform: "discord".to_string(),
                    server_id: "s1".to_string(),
                    user_id: "u1".to_string(),
                },
                policy_name: "admin".to_string(),
            }).unwrap();
        }
        let store = JsonPairingStore::new(dir.path());
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].policy_name, "admin");
    }

    #[test]
    fn remove_returns_bool() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonPairingStore::new(dir.path());
        let key = PairingKey {
            platform: "discord".to_string(),
            server_id: "s1".to_string(),
            user_id: "u1".to_string(),
        };
        store.add(&Pairing { key: key.clone(), policy_name: "admin".to_string() }).unwrap();
        assert_eq!(store.list().len(), 1);
        assert!(store.remove(&key).unwrap());
        assert_eq!(store.list().len(), 0);
        assert!(!store.remove(&key).unwrap());
    }
}
