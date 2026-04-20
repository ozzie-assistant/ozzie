mod loader;
mod registry;
mod repository;
mod types;

pub use loader::ProjectLoadError;
pub use registry::ProjectRegistry;
pub use repository::{FsProjectRepository, InMemoryProjectRepository, ProjectRepository};
pub use types::{ExtractionHint, ProjectManifest, ProjectMemoryConfig};
