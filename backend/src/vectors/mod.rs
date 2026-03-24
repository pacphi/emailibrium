//! Vector intelligence layer for Emailibrium.
//!
//! This module implements the Email Intelligence bounded context (DDD-001),
//! providing embedding, vector storage, search, and classification capabilities.

pub mod backup;
pub mod categorizer;
pub mod clustering;
pub mod config;
pub mod embedding;
pub mod encryption;
pub mod error;
pub mod ingestion;
pub mod insights;
pub mod interactions;
pub mod learning;
pub mod metrics;
pub mod quantization;
pub mod search;
pub mod store;
pub mod types;

use std::sync::Arc;

use crate::db::Database;
use config::VectorConfig;
use embedding::EmbeddingPipeline;
use store::VectorStoreBackend;

/// Top-level vector service facade (DDD-001: EmbeddingAggregate + ClassificationAggregate).
///
/// Coordinates embedding, storage, search, and classification operations.
/// This is the primary entry point for vector intelligence capabilities.
pub struct VectorService {
    pub embedding: Arc<EmbeddingPipeline>,
    pub store: Arc<dyn VectorStoreBackend>,
    pub categorizer: Arc<categorizer::VectorCategorizer>,
    pub config: VectorConfig,
    pub db: Arc<Database>,
}

impl VectorService {
    /// Create a new VectorService with the given configuration.
    pub async fn new(config: VectorConfig, db: Arc<Database>) -> Result<Self, error::VectorError> {
        // Initialize embedding pipeline with fallback chain
        let embedding = Arc::new(EmbeddingPipeline::new(&config.embedding)?);

        // Initialize vector store (in-memory for now, RuVector behind feature flag)
        let store: Arc<dyn VectorStoreBackend> = if config.encryption.enabled {
            let inner = Arc::new(store::InMemoryVectorStore::new());
            Arc::new(encryption::EncryptedVectorStore::new(
                inner,
                &config.encryption,
            )?)
        } else {
            Arc::new(store::InMemoryVectorStore::new())
        };

        // Initialize categorizer
        let categorizer = Arc::new(categorizer::VectorCategorizer::new(
            store.clone(),
            embedding.clone(),
            config.categorizer.confidence_threshold,
        ));

        Ok(Self {
            embedding,
            store,
            categorizer,
            config,
            db,
        })
    }

    /// Get health status of the vector service.
    pub async fn health(&self) -> Result<types::HealthStatus, error::VectorError> {
        let store_health = self.store.health().await?;
        let embedding_available = self.embedding.is_available().await;

        Ok(types::HealthStatus {
            status: if store_health && embedding_available {
                "healthy".to_string()
            } else {
                "degraded".to_string()
            },
            store_healthy: store_health,
            embedding_available,
            store_stats: self.store.stats().await?,
        })
    }

    /// Get detailed statistics about the vector store.
    pub async fn stats(&self) -> Result<types::VectorStats, error::VectorError> {
        self.store.stats().await
    }
}
