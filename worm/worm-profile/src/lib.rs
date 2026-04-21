mod repository;
mod synthesizer;
mod types;

pub use repository::{InMemoryProfileRepository, ProfileError, ProfileRepository};
pub use synthesizer::ProfileSynthesizer;
pub use types::{UserProfile, WhoamiEntry, WhoamiSource};

/// Default filename for profile persistence.
pub const PROFILE_FILE: &str = "profile.jsonc";
