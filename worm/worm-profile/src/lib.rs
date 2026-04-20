mod repository;
mod synthesizer;
mod types;

pub use repository::{
    FsProfileRepository, InMemoryProfileRepository, ProfileError, ProfileRepository,
};
pub use synthesizer::ProfileSynthesizer;
pub use types::{UserProfile, WhoamiEntry, WhoamiSource};

/// Default filename used by [`FsProfileRepository`].
pub const PROFILE_FILE: &str = "profile.jsonc";
