mod types;

pub use types::{UserProfile, WhoamiEntry};

use std::path::Path;

const PROFILE_FILE: &str = "profile.jsonc";

/// Loads the user profile from `$OZZIE_PATH/profile.jsonc`.
/// Returns `None` if the file does not exist.
pub fn load(ozzie_path: &Path) -> Result<Option<UserProfile>, String> {
    let path = ozzie_path.join(PROFILE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profile: UserProfile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(profile))
}

/// Saves the user profile to `$OZZIE_PATH/profile.jsonc`.
pub fn save(ozzie_path: &Path, profile: &UserProfile) -> Result<(), String> {
    let path = ozzie_path.join(PROFILE_FILE);
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}
