//! Vector intelligence layer for Emailibrium.
//!
//! This module implements the Email Intelligence bounded context (DDD-001),
//! providing embedding, vector storage, search, and classification capabilities.

pub mod audit;
pub mod backup;
pub mod categorizer;
pub mod chat;
pub mod clustering;
pub mod config;
pub mod consent;
pub mod embedding;
pub mod encryption;
pub mod error;
pub mod evaluation;
pub mod ewc;
pub mod generative;
pub mod generative_router;
pub mod hdbscan;
pub mod inference_session;
pub mod ingestion;
pub mod insights;
pub mod interactions;
pub mod learning;
pub mod metrics;
pub mod model_download;
pub mod model_integrity;
pub mod model_registry;
pub mod models;
pub mod privacy;
pub mod qdrant_store;
pub mod quantization;
pub mod reindex;
pub mod remote_wipe;
pub mod ruvector_store;
pub mod search;
pub mod sqlite_store;
pub mod store;
pub mod types;
pub mod user_learning;

use std::sync::Arc;

use crate::cache::RedisCache;
use crate::db::Database;
use audit::CloudApiAuditLogger;
use config::VectorConfig;
use embedding::EmbeddingPipeline;
use evaluation::EvaluationEngine;
use generative_router::GenerativeRouter;
use inference_session::InferenceSessionManager;
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
    pub reindex_orchestrator: Arc<reindex::ReindexOrchestrator>,
    pub generative: Option<Arc<dyn generative::GenerativeModel>>,
    pub consent_manager: Arc<consent::ConsentManager>,
    pub remote_wipe_service: Arc<remote_wipe::RemoteWipeService>,
    pub privacy_service: Arc<privacy::PrivacyService>,
    pub unsubscribe_service: Option<Arc<crate::email::unsubscribe::UnsubscribeService>>,
    pub audit_logger: Arc<CloudApiAuditLogger>,
    pub evaluation_engine: Arc<EvaluationEngine>,
    pub generative_router: Arc<GenerativeRouter>,
    pub inference_session_manager: Arc<InferenceSessionManager>,
    pub config: VectorConfig,
    pub db: Arc<Database>,
}

impl VectorService {
    /// Create a new VectorService with the given configuration.
    ///
    /// `redis` is optional -- when provided, the embedding pipeline uses it as
    /// an L2 cache to avoid re-computing embeddings across restarts.
    pub async fn new(
        config: VectorConfig,
        db: Arc<Database>,
        redis: Option<Arc<RedisCache>>,
    ) -> Result<Self, error::VectorError> {
        // Initialize embedding pipeline with fallback chain + optional Redis L2 cache
        let embedding = Arc::new(
            EmbeddingPipeline::new(&config.embedding)?
                .with_redis(redis, config.redis.cache_ttl_secs),
        );

        // Initialize vector store backend based on config (ADR-003).
        // Fallback chain: ruvector (default) -> qdrant -> sqlite -> memory.
        let raw_store: Arc<dyn VectorStoreBackend> = match config.store.backend.as_str() {
            "memory" => {
                tracing::info!("Vector store: in-memory brute-force backend");
                Arc::new(store::InMemoryVectorStore::new())
            }
            "qdrant" => {
                let qdrant_cfg = qdrant_store::QdrantConfig {
                    url: config
                        .store
                        .qdrant_url
                        .clone()
                        .unwrap_or_else(|| "http://localhost:6333".to_string()),
                    collection_prefix: config
                        .store
                        .qdrant_collection_prefix
                        .clone()
                        .unwrap_or_else(|| "emailibrium".to_string()),
                    api_key: config.store.qdrant_api_key.clone(),
                    dimensions: config.embedding.dimensions,
                };
                match qdrant_store::QdrantVectorStore::new(qdrant_cfg).await {
                    Ok(qs) => {
                        tracing::info!("Vector store: Qdrant HNSW backend");
                        Arc::new(qs)
                    }
                    Err(e) => {
                        tracing::warn!("Qdrant init failed ({e}), falling back to in-memory");
                        Arc::new(store::InMemoryVectorStore::new())
                    }
                }
            }
            "sqlite" => {
                match sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(&config.database_url)
                    .await
                {
                    Ok(pool) => match sqlite_store::SqliteVectorStore::new(pool).await {
                        Ok(ss) => {
                            tracing::info!("Vector store: SQLite brute-force emergency backend");
                            Arc::new(ss)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "SQLite vector store init failed ({e}), falling back to in-memory"
                            );
                            Arc::new(store::InMemoryVectorStore::new())
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            "SQLite pool creation failed ({e}), falling back to in-memory"
                        );
                        Arc::new(store::InMemoryVectorStore::new())
                    }
                }
            }
            _ => {
                // Default to RuVector HNSW backend.
                match ruvector_store::RuVectorStore::new(
                    &config.store,
                    &config.index,
                    config.embedding.dimensions,
                ) {
                    Ok(rv) => {
                        tracing::info!("Vector store: RuVector HNSW backend");
                        Arc::new(rv)
                    }
                    Err(e) => {
                        tracing::warn!("RuVector init failed ({e}), falling back to in-memory");
                        Arc::new(store::InMemoryVectorStore::new())
                    }
                }
            }
        };

        let store: Arc<dyn VectorStoreBackend> = if config.encryption.enabled {
            Arc::new(encryption::EncryptedVectorStore::new(
                raw_store,
                &config.encryption,
            )?)
        } else {
            raw_store
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

        // Initialize generative model based on config (ADR-012)
        let gen_model: Option<Arc<dyn generative::GenerativeModel>> =
            match config.generative.provider.as_str() {
                "ollama" => Some(Arc::new(generative::OllamaGenerativeModel::new(
                    &config.generative.ollama,
                ))),
                "cloud" => match generative::CloudGenerativeModel::new(&config.generative.cloud) {
                    Ok(model) => Some(Arc::new(model)),
                    Err(e) => {
                        tracing::warn!(
                            "Cloud generative model init failed: {e}, falling back to none"
                        );
                        None
                    }
                },
                _ => None,
            };

        // Initialize consent manager
        let consent_manager = Arc::new(consent::ConsentManager::new(db.clone()));

        // Initialize remote wipe service (ADR-008: device loss mitigation)
        let remote_wipe_service = Arc::new(remote_wipe::RemoteWipeService::new(db.clone()));
        if let Err(e) = remote_wipe_service.ensure_table().await {
            tracing::warn!("Failed to create wipe audit table: {e}");
        }

        // Initialize GDPR privacy service (R-09: consent persistence)
        let privacy_service = Arc::new(privacy::PrivacyService::new(db.clone()));
        if let Err(e) = privacy_service.ensure_tables().await {
            tracing::warn!("Failed to create GDPR consent tables: {e}");
        }

        // Initialize unsubscribe service (R-04: bulk unsubscribe)
        let unsubscribe_service = Some(Arc::new(
            crate::email::unsubscribe::UnsubscribeService::new(),
        ));

        // Initialize cloud API audit logger (ADR-008, item #39)
        let audit_logger = Arc::new(CloudApiAuditLogger::new(db.clone()));
        if let Err(e) = audit_logger.ensure_table().await {
            tracing::warn!("Failed to create cloud API audit table: {e}");
        }

        // Initialize A/B evaluation engine (ADR-004, item #22)
        let evaluation_engine = Arc::new(EvaluationEngine::new(db.clone()));
        if let Err(e) = evaluation_engine.ensure_tables().await {
            tracing::warn!("Failed to create evaluation tables: {e}");
        }

        // Initialize generative router with failover (DDD-006, item #38)
        let generative_router = Arc::new(GenerativeRouter::new());
        if let Some(ref gen) = gen_model {
            use model_registry::ProviderType;
            let provider_type = match config.generative.provider.as_str() {
                "ollama" => ProviderType::Ollama,
                "cloud" => ProviderType::OpenAi,
                _ => ProviderType::None,
            };
            generative_router
                .register(provider_type, gen.clone(), 1)
                .await;
            tracing::info!(
                "Registered generative provider in router: {}",
                config.generative.provider
            );
        }

        // Initialize inference session manager (DDD-006, item #38)
        let inference_session_manager = Arc::new(InferenceSessionManager::new(100));

        // Initialize re-index orchestrator and check for model changes
        let reindex_orchestrator = Arc::new(reindex::ReindexOrchestrator::new(db.clone()));
        if let Ok(needs_reindex) = reindex_orchestrator
            .check_model_change(&config.embedding.model, config.embedding.dimensions)
            .await
        {
            if needs_reindex {
                let count = reindex_orchestrator.mark_all_stale().await.unwrap_or(0);
                tracing::info!(
                    "Marked {} emails for re-indexing due to model change",
                    count
                );
            }
        }

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
            reindex_orchestrator,
            generative: gen_model,
            consent_manager,
            remote_wipe_service,
            privacy_service,
            unsubscribe_service,
            audit_logger,
            evaluation_engine,
            generative_router,
            inference_session_manager,
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
