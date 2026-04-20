// Re-export from worm-profile — the canonical source of truth.
pub use worm_profile::{
    FsProfileRepository, InMemoryProfileRepository, ProfileError, ProfileRepository,
    ProfileSynthesizer, UserProfile, WhoamiEntry, WhoamiSource, PROFILE_FILE,
};
