// Re-export from worm-profile (domain) and worm-profile-fs (filesystem backend).
pub use worm_profile::{
    InMemoryProfileRepository, ProfileError, ProfileRepository, ProfileSynthesizer, UserProfile,
    WhoamiEntry, WhoamiSource, PROFILE_FILE,
};
pub use worm_profile_fs::FsProfileRepository;
