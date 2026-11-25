//! Error types for `rix-store`.

use thiserror::Error;

/// Result alias for `rix-store` operations.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Errors returned by store, manifest, and P2P operations.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("blob store error: {0}")]
    BlobStore(String),

    #[error("invalid store path or key: {0}")]
    InvalidPath(String),

    #[error("invalid store manifest: {0}")]
    InvalidManifest(String),

    #[error("invalid key or signature: {0}")]
    Crypto(String),

    #[error("untrusted store path '{0}': no valid signature from a trusted key")]
    UntrustedPath(String),

    #[error("store path not found: {0}")]
    NotFound(String),

    #[error("P2P network error: {0}")]
    P2p(String),

    #[error("DHT error: {0}")]
    Dht(String),

    #[error("configuration error: {0}")]
    Config(String),
}
