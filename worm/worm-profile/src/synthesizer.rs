use crate::types::WhoamiEntry;

/// Trait for LLM-driven profile consolidation.
///
/// The consumer provides their own LLM implementation.
/// The consolidator takes a list of compressible entries and returns
/// a reduced set of consolidated entries.
#[async_trait::async_trait]
pub trait ProfileSynthesizer: Send + Sync {
    /// Consolidate multiple whoami entries into a smaller set.
    ///
    /// The implementation should:
    /// - Group related facts
    /// - Remove redundancy
    /// - Preserve meaning
    /// - Return entries with [`WhoamiSource::Consolidated`]
    async fn consolidate(&self, entries: &[WhoamiEntry]) -> Result<Vec<WhoamiEntry>, String>;
}
