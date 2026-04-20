/// Errors from memory operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("{0}")]
    Other(String),
}
