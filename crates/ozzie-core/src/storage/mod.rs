//! Generic typed configuration storage port.
//!
//! Provides [`ConfigStore`] — a trait for persistent typed stores.
//! Concrete implementations (e.g. file-backed) live in infrastructure crates.

/// Error type for storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lock poisoned")]
    Poisoned,
}

/// Trait for typed persistent stores.
///
/// Implementations must be `Send + Sync` to be shareable via `Arc`.
pub trait ConfigStore<T>: Send + Sync {
    /// Reads the current value, returning `T::default()` if not yet persisted.
    fn read(&self) -> Result<T, StorageError>;

    /// Atomically reads, transforms, and writes back the value.
    fn patch(&self, f: Box<dyn FnOnce(T) -> T + Send>) -> Result<T, StorageError>;

    /// Overwrites the stored value.
    fn save(&self, value: T) -> Result<(), StorageError>;
}
