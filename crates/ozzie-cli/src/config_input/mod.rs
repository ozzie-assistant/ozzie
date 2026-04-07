pub mod presets;
pub mod secrets;
pub mod section;
pub mod sections;
pub mod stdin_collector;

// Re-exports for external consumers.
#[allow(unused_imports)]
pub use section::{
    BuildOutput, CollectResult, ConfigSection, FieldKind, FieldSpec, FieldValue, FieldValues,
    InputCollector, SectionId, SelectOption, confirm_add_more,
};
pub use secrets::store_secrets;
pub use stdin_collector::StdinCollector;
