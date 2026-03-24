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
    pub hybrid_search: Arc<search::HybridSearch>,
    pub cluster_engine: Arc<clustering::ClusterEngine>,
    pub learning_engine: Arc<learning::LearningEngine>,
    pub interaction_tracker: Arc<interactions::InteractionTracker>,
    pub insight_engine: Arc<insights::InsightEngine>,
    pub backup_service: Arc<backup::VectorBackupService>,
    pub quantization_engine: Arc<quantization::QuantizationEngine>,
    pub ingestion_pipeline: Arc<ingestion::IngestionPipeline>,
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

        // Initialize hybrid search
        let hybrid_search = Arc::new(search::HybridSearch::new(
            store.clone(),
            embedding.clone(),
            db.clone(),
            config.search.clone(),
        ));

        // Initialize cluster engine
        let cluster_engine = Arc::new(clustering::ClusterEngine::new(
            store.clone(),
            db.clone(),
            config.clustering.clone(),
        ));

        // Initialize SONA learning engine
        let learning_engine = Arc::new(learning::LearningEngine::new(
            categorizer.clone(),
            store.clone(),
            db.clone(),
            config.learning.clone(),
        ));

        // Initialize interaction tracker
        let interaction_tracker = Arc::new(interactions::InteractionTracker::new(db.clone()));

        // Initialize insight engine
        let insight_engine = Arc::new(insights::InsightEngine::new(db.clone(), store.clone()));

        // Initialize backup service (no encryption handle passed for now)
        let backup_service = Arc::new(backup::VectorBackupService::new(
            db.clone(),
            store.clone(),
            None,
        ));

        // Restore vectors from backup on startup if enabled
        if config.backup.enabled {
            let restored = backup_service.restore_all().await.unwrap_or_default();
            if !restored.is_empty() {
                let count = restored.len();
                let _ = store.batch_insert(restored).await;
                tracing::info!("Restored {} vectors from backup", count);
            }
        }

        // Initialize quantization engine
        let quantization_engine = Arc::new(quantization::QuantizationEngine::new(
            config.quantization.clone(),
        ));

        // Initialize ingestion pipeline
        let ingestion_pipeline = Arc::new(ingestion::IngestionPipeline::new(
            embedding.clone(),
            store.clone(),
            categorizer.clone(),
            db.clone(),
        ));

        Ok(Self {
            embedding,
            store,
            categorizer,
            hybrid_search,
            cluster_engine,
            learning_engine,
            interaction_tracker,
            insight_engine,
            backup_service,
            quantization_engine,
            ingestion_pipeline,
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
