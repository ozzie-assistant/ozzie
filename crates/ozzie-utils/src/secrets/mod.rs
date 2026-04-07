mod ops;
mod store;

pub use ops::*;
pub use store::SecretStore;

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("encryption error: {0}")]
    Encryption(String),
    #[error("{0}")]
    Other(String),
}
