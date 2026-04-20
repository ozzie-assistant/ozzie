use crate::types::{ArchivePayload, Index};

/// Errors from archive store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
}

/// Port for layered context persistence.
///
/// Implementations handle reading/writing the index and archive files
/// for each session. The concrete file-based implementation lives in
/// `ozzie-runtime::layered_store`.
#[async_trait::async_trait]
pub trait ArchiveStore: Send + Sync {
    /// Loads the index for a session. Returns `None` if it doesn't exist.
    async fn load_index(&self, session_id: &str) -> Result<Option<Index>, StoreError>;

    /// Saves (overwrites) the index for a session.
    async fn save_index(&self, session_id: &str, idx: &Index) -> Result<(), StoreError>;

    /// Writes a full transcript archive for a node.
    async fn write_archive(
        &self,
        session_id: &str,
        node_id: &str,
        payload: &ArchivePayload,
    ) -> Result<(), StoreError>;

    /// Reads a full transcript archive for a node.
    async fn read_archive(
        &self,
        session_id: &str,
        node_id: &str,
    ) -> Result<Option<ArchivePayload>, StoreError>;

    /// Removes archive files whose node IDs are not in the valid set.
    async fn cleanup_archives(
        &self,
        session_id: &str,
        valid_node_ids: &[String],
    ) -> Result<(), StoreError>;
}
