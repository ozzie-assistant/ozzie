use std::path::{Path, PathBuf};

use worm_profile::{ProfileError, ProfileRepository, UserProfile, PROFILE_FILE};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn load_nonexistent_returns_none() {
        let dir = std::env::temp_dir().join("worm_profile_fs_test_nonexistent");
        let repo = FsProfileRepository::new(&dir);
        let result = repo.load().await.expect("should not error");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("worm_profile_fs_test_roundtrip");
        tokio::fs::create_dir_all(&dir).await.ok();
        let repo = FsProfileRepository::new(&dir);

        let profile = UserProfile::new("Alice".into(), vec!["loves Rust".into()]);
        repo.save(&profile).await.expect("save");

        let loaded = repo.load().await.expect("load").expect("should exist");
        assert_eq!(loaded.name, "Alice");
        assert_eq!(loaded.whoami.len(), 1);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
