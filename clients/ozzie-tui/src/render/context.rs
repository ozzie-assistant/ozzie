/// Shared rendering context passed to all render functions.
/// Extensible — add fields here instead of threading parameters everywhere.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    /// Preferred language from Ozzie config (e.g. "fr").
    pub language: Option<String>,
}
