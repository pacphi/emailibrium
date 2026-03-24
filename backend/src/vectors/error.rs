//! Error types for the vector intelligence layer.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum VectorError {
    #[error("Embedding generation failed: {0}")]
    EmbeddingFailed(String),

    #[error("All embedding providers unavailable: {0}")]
    AllProvidersUnavailable(String),

    #[error("Vector store operation failed: {0}")]
    StoreFailed(String),

    #[error("Vector not found: {0}")]
    NotFound(String),

    #[error("Invalid dimensions: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Categorization failed: {0}")]
    CategorizationFailed(String),

    #[error("Backup error: {0}")]
    BackupError(String),

    #[error("Ingestion error: {0}")]
    IngestionError(String),

    #[error("Insight error: {0}")]
    InsightError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Clustering error: {0}")]
    ClusteringError(String),
}
