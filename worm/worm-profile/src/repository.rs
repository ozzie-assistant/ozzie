use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::UserProfile;

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
