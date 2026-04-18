use std::path::Path;

use crate::types::{UserProfile, WhoamiEntry};
use crate::PROFILE_FILE;

/// Loads a user profile from `<base_path>/profile.jsonc`.
///
/// Returns `Ok(None)` if the file does not exist.
pub fn load(base_path: &Path) -> Result<Option<UserProfile>, String> {
    let path = base_path.join(PROFILE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profile: UserProfile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(profile))
}

/// Saves a user profile to `<base_path>/profile.jsonc`.
pub fn save(base_path: &Path, profile: &UserProfile) -> Result<(), String> {
    let path = base_path.join(PROFILE_FILE);
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Trait for LLM-driven profile consolidation.
///
/// The consumer provides their own LLM implementation.
/// The consolidator takes a list of compressible entries and returns
/// a reduced set of consolidated entries.
#[async_trait::async_trait]
pub trait ProfileSynthesizer: Send + Sync {
    /// Consolidate multiple whoami entries into a smaller set.
    ///
    /// The implementation should:
    /// - Group related facts
    /// - Remove redundancy
    /// - Preserve meaning
    /// - Return entries with [`WhoamiSource::Consolidated`]
    async fn consolidate(&self, entries: &[WhoamiEntry]) -> Result<Vec<WhoamiEntry>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = std::env::temp_dir().join("worm_profile_test_nonexistent");
        let result = load(&dir).expect("should not error");
        assert!(result.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("worm_profile_test_roundtrip");
        std::fs::create_dir_all(&dir).ok();
        let profile = UserProfile::new("Alice".into(), vec!["loves Rust".into()]);

        save(&dir, &profile).expect("save");
        let loaded = load(&dir).expect("load").expect("should exist");

        assert_eq!(loaded.name, "Alice");
        assert_eq!(loaded.whoami.len(), 1);

        // cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}
