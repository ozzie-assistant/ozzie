use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::types::UserProfile;
use crate::PROFILE_FILE;

/// Errors from profile persistence operations.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
}

/// Port for user profile persistence.
#[async_trait::async_trait]
pub trait ProfileRepository: Send + Sync {
    /// Loads the user profile. Returns `None` if it doesn't exist yet.
    async fn load(&self) -> Result<Option<UserProfile>, ProfileError>;

    /// Saves (overwrites) the user profile.
    async fn save(&self, profile: &UserProfile) -> Result<(), ProfileError>;
}

/// File-based profile repository.
///
/// Stores the profile as `<base_path>/profile.jsonc`.
pub struct FsProfileRepository {
    base_path: PathBuf,
}

impl FsProfileRepository {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }
}

#[async_trait::async_trait]
impl ProfileRepository for FsProfileRepository {
    async fn load(&self) -> Result<Option<UserProfile>, ProfileError> {
        let path = self.base_path.join(PROFILE_FILE);
        match tokio::fs::read_to_string(&path).await {
            Ok(raw) => {
                let profile: UserProfile =
                    serde_json::from_str(&raw).map_err(|e| ProfileError::Parse(e.to_string()))?;
                Ok(Some(profile))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ProfileError::Io(e.to_string())),
        }
    }

    async fn save(&self, profile: &UserProfile) -> Result<(), ProfileError> {
        let path = self.base_path.join(PROFILE_FILE);
        let json =
            serde_json::to_string_pretty(profile).map_err(|e| ProfileError::Parse(e.to_string()))?;
        tokio::fs::write(&path, json)
            .await
            .map_err(|e| ProfileError::Io(e.to_string()))?;
        Ok(())
    }
}

/// In-memory profile repository for testing.
pub struct InMemoryProfileRepository {
    profile: RwLock<HashMap<&'static str, UserProfile>>,
}

impl InMemoryProfileRepository {
    pub fn new() -> Self {
        Self {
            profile: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_profile(profile: UserProfile) -> Self {
        let mut map = HashMap::new();
        map.insert("default", profile);
        Self {
            profile: RwLock::new(map),
        }
    }
}

impl Default for InMemoryProfileRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ProfileRepository for InMemoryProfileRepository {
    async fn load(&self) -> Result<Option<UserProfile>, ProfileError> {
        let map = self.profile.read().unwrap();
        Ok(map.get("default").cloned())
    }

    async fn save(&self, profile: &UserProfile) -> Result<(), ProfileError> {
        let mut map = self.profile.write().unwrap();
        map.insert("default", profile.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fs_load_nonexistent_returns_none() {
        let dir = std::env::temp_dir().join("worm_profile_repo_test_nonexistent");
        let repo = FsProfileRepository::new(&dir);
        let result = repo.load().await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn fs_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("worm_profile_repo_test_roundtrip");
        tokio::fs::create_dir_all(&dir).await.ok();
        let repo = FsProfileRepository::new(&dir);

        let profile = UserProfile::new("Alice".into(), vec!["loves Rust".into()]);
        repo.save(&profile).await.expect("save");

        let loaded = repo.load().await.expect("load").expect("should exist");
        assert_eq!(loaded.name, "Alice");
        assert_eq!(loaded.whoami.len(), 1);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn in_memory_empty() {
        let repo = InMemoryProfileRepository::new();
        assert!(repo.load().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_roundtrip() {
        let repo = InMemoryProfileRepository::new();
        let profile = UserProfile::new("Bob".into(), vec![]);

        repo.save(&profile).await.unwrap();
        let loaded = repo.load().await.unwrap().expect("should exist");
        assert_eq!(loaded.name, "Bob");
    }

    #[tokio::test]
    async fn in_memory_with_profile() {
        let profile = UserProfile::new("Carol".into(), vec!["test".into()]);
        let repo = InMemoryProfileRepository::with_profile(profile);

        let loaded = repo.load().await.unwrap().expect("should exist");
        assert_eq!(loaded.name, "Carol");
    }
}
