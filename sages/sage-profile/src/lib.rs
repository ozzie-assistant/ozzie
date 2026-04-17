mod types;
mod store;

pub use store::{load, save, ProfileSynthesizer};
pub use types::{UserProfile, WhoamiEntry, WhoamiSource};

/// Default filename used by [`load`] and [`save`].
pub const PROFILE_FILE: &str = "profile.jsonc";
