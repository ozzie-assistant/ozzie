mod loader;
mod registry;
mod types;

pub use loader::{discover_projects, load_project, ProjectLoadError};
pub use registry::ProjectRegistry;
pub use types::{ExtractionHint, ProjectManifest, ProjectMemoryConfig};
