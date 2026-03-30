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
#[cfg(feature = "builtin-llm")]
pub mod generative_builtin;
pub mod generative_router;
pub mod hdbscan;
pub mod inference_session;
pub mod ingestion;
pub mod insights;
pub mod interactions;
pub mod learning;
pub mod metrics;
pub mod model_catalog;
pub mod model_download;
pub mod model_integrity;
pub mod model_registry;
pub mod models;
pub mod privacy;
pub mod qdrant_store;
pub mod quantization;
pub mod rag;
pub mod reindex;
pub mod remote_wipe;
pub mod ruvector_store;
pub mod search;
pub mod sqlite_store;
pub mod store;
pub mod types;
pub mod user_learning;
pub mod yaml_config;

use std::sync::Arc;

use crate::cache::RedisCache;
use crate::db::Database;
use audit::CloudApiAuditLogger;
use config::VectorConfig;
use embedding::EmbeddingPipeline;
use evaluation::EvaluationEngine;
use generative::GenerationParams;
use generative_router::GenerativeRouter;
use inference_session::InferenceSessionManager;
use store::VectorStoreBackend;
use yaml_config::YamlConfig;

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
    /// Handle to the built-in LLM (if configured) for idle-timeout monitoring.
    /// This is `None` when the generative provider is not "builtin".
    #[cfg(feature = "builtin-llm")]
    pub builtin_model: Option<Arc<generative_builtin::BuiltInGenerativeModel>>,
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
    ///
    /// `yaml_config` provides global LLM tuning parameters and the model catalog
    /// so that per-model and global generation settings are plumbed into every
    /// generative model implementation (no hardcoded values).
    pub async fn new(
        config: VectorConfig,
        db: Arc<Database>,
        redis: Option<Arc<RedisCache>>,
        yaml_config: Option<&YamlConfig>,
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

        // Load category centroids from DB, seed if empty (ADR-004)
        match categorizer.load_centroids_from_db(&db).await {
            Ok(0) => match categorizer.seed_initial_centroids(&db).await {
                Ok(seeded) => tracing::info!("Seeded {seeded} initial category centroids"),
                Err(e) => tracing::warn!("Failed to seed category centroids: {e}"),
            },
            Ok(loaded) => tracing::info!("Loaded {loaded} category centroids from database"),
            Err(e) => tracing::warn!("Failed to load category centroids: {e}"),
        }

        // Load tuning parameters from YAML config for ingestion, clustering, and error recovery.
        let yaml_tuning = yaml_config::load_yaml_config("../config")
            .map(|c| c.tuning)
            .unwrap_or_default();

        // Initialize hybrid search
        let hybrid_search = Arc::new(search::HybridSearch::new(
            store.clone(),
            embedding.clone(),
            db.clone(),
            config.search.clone(),
        ));

        // Initialize cluster engine with tuning parameters from YAML config
        let cluster_engine = Arc::new(clustering::ClusterEngine::new_with_tuning(
            store.clone(),
            db.clone(),
            config.clustering.clone(),
            yaml_tuning.clustering.clone(),
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

        // Detect stale embedding status: if the vector store is empty but the DB
        // has emails marked as 'embedded', reset them to 'pending' so the next
        // ingestion run re-processes them. This handles in-memory store restarts.
        let store_count = store.count().await.unwrap_or(0);
        if store_count == 0 {
            let (embedded_in_db,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM emails WHERE embedding_status = 'embedded'")
                    .fetch_one(&db.pool)
                    .await
                    .unwrap_or((0,));

            if embedded_in_db > 0 {
                let reset = sqlx::query(
                    "UPDATE emails SET embedding_status = 'pending' WHERE embedding_status = 'embedded'",
                )
                .execute(&db.pool)
                .await
                .map(|r| r.rows_affected())
                .unwrap_or(0);
                tracing::info!(
                    "Vector store empty but {embedded_in_db} emails marked as embedded — \
                     reset {reset} to pending for re-embedding on next sync"
                );
            }
        }

        // Initialize quantization engine
        let quantization_engine = Arc::new(quantization::QuantizationEngine::new(
            config.quantization.clone(),
        ));

        // Initialize ingestion pipeline (generative model injected after init below)
        let mut ingestion_pipeline = ingestion::IngestionPipeline::new(
            embedding.clone(),
            store.clone(),
            categorizer.clone(),
            db.clone(),
            yaml_tuning.ingestion.clone(),
            yaml_tuning.error_recovery.clone(),
        );

        // ── Resolve LLM tuning parameters ────────────────────────────────
        // Global defaults from tuning.yaml, with per-model overrides from
        // models-llm.yaml resolved at construction time.
        let default_yaml = YamlConfig::default();
        let yaml_ref = yaml_config.unwrap_or(&default_yaml);
        let llm_tuning = &yaml_ref.tuning.llm;

        // Helper: look up per-model tuning from the LLM catalog for a given
        // provider name and model ID.
        let find_model_tuning =
            |provider_name: &str, model_id: &str| -> Option<yaml_config::ModelTuning> {
                yaml_ref
                    .llm_catalog
                    .providers
                    .get(provider_name)?
                    .models
                    .iter()
                    .find(|m| m.id == model_id)?
                    .tuning
                    .clone()
            };

        // Resolve prompts from YAML config (or defaults).
        let prompts_cfg = yaml_ref.prompts.clone();

        // Initialize generative model based on config (ADR-012)
        // Also track the concrete builtin model handle for idle-timeout monitoring.
        #[cfg(feature = "builtin-llm")]
        let mut builtin_model_handle: Option<Arc<generative_builtin::BuiltInGenerativeModel>> = None;

        let gen_model: Option<Arc<dyn generative::GenerativeModel>> = match config
            .generative
            .provider
            .as_str()
        {
            "ollama" => {
                let per_model =
                    find_model_tuning("ollama", &config.generative.ollama.chat_model);
                let params = GenerationParams::resolve(llm_tuning, per_model.as_ref());
                Some(Arc::new(generative::OllamaGenerativeModel::with_params_and_prompts(
                    &config.generative.ollama,
                    params,
                    prompts_cfg.clone(),
                )))
            }
            "cloud" => match generative::CloudGenerativeModel::with_params_and_prompts(
                &config.generative.cloud,
                GenerationParams::resolve(llm_tuning, None),
                prompts_cfg.clone(),
            ) {
                Ok(model) => Some(Arc::new(model)),
                Err(e) => {
                    tracing::warn!("Cloud generative model init failed: {e}, falling back to none");
                    None
                }
            },
            "builtin" => {
                #[cfg(feature = "builtin-llm")]
                {
                    let per_model = find_model_tuning("builtin", &config.generative.builtin.model_id);
                    let params = GenerationParams::resolve(llm_tuning, per_model.as_ref());
                    match generative_builtin::BuiltInGenerativeModel::with_params_and_prompts(
                        &config.generative.builtin,
                        params,
                        prompts_cfg.clone(),
                    ) {
                        Ok(model) => {
                            tracing::info!(
                                "Generative provider: builtin ({}) via llama.cpp",
                                config.generative.builtin.model_id,
                            );
                            let arc_model = Arc::new(model);
                            builtin_model_handle = Some(arc_model.clone());
                            Some(arc_model as Arc<dyn generative::GenerativeModel>)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Built-in LLM init failed: {e}, falling back to rule-based"
                            );
                            None
                        }
                    }
                }
                #[cfg(not(feature = "builtin-llm"))]
                {
                    tracing::info!(
                        "Generative provider: builtin selected but 'builtin-llm' feature not enabled. \
                         Rebuild with: cargo build --features builtin-llm"
                    );
                    None
                }
            }
            "openrouter" => {
                // OpenRouter uses the OpenAI-compatible API with extra headers.
                // Read config from the already-loaded YAML catalog.
                let or_provider = yaml_ref.llm_catalog.providers.get("openrouter");

                // Fall back through: catalog → app.yaml providers → hardcoded default
                let app_or = &yaml_ref.app.providers.openrouter;
                let api_key_env = or_provider
                    .and_then(|p| p.api_key_env.clone())
                    .unwrap_or_else(|| {
                        if app_or.api_key_env.is_empty() {
                            "OPENROUTER_API_KEY".to_string()
                        } else {
                            app_or.api_key_env.clone()
                        }
                    });
                let base_url = or_provider
                    .and_then(|p| p.base_url.clone())
                    .unwrap_or_else(|| app_or.base_url.clone());
                let extra_headers = or_provider
                    .and_then(|p| p.required_headers.clone())
                    .unwrap_or_else(|| app_or.required_headers.clone());

                // Use the cloud config model field as the OpenRouter model ID.
                let model_id = &config.generative.cloud.model;

                let per_model = find_model_tuning("openrouter", model_id);
                let params = GenerationParams::resolve(llm_tuning, per_model.as_ref());

                match generative::OpenRouterGenerativeModel::with_params_and_prompts(
                    &api_key_env,
                    model_id,
                    &base_url,
                    extra_headers,
                    params,
                    prompts_cfg.clone(),
                ) {
                    Ok(model) => {
                        tracing::info!(
                            "Generative provider: openrouter ({model_id}) via {}",
                            base_url,
                        );
                        Some(Arc::new(model) as Arc<dyn generative::GenerativeModel>)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "OpenRouter generative model init failed: {e}, falling back to none"
                        );
                        None
                    }
                }
            }
            "none" => None,
            _ => {
                tracing::warn!(
                    "Unknown generative provider '{}', defaulting to none",
                    config.generative.provider
                );
                None
            }
        };

        // Inject generative model into ingestion pipeline for categorize_with_fallback (DEFECT-2)
        ingestion_pipeline.set_generative(gen_model.clone());
        // Inject classification config from YAML (categories + domain/keyword rules)
        if let Some(yc) = yaml_config {
            ingestion_pipeline.set_classification_config(yc.classification.clone());
        }
        // Inject cluster engine for post-ingestion clustering (ADR-009)
        ingestion_pipeline.set_cluster_engine(cluster_engine.clone());
        let ingestion_pipeline = Arc::new(ingestion_pipeline);

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
                "openrouter" => ProviderType::OpenRouter,
                "builtin" => ProviderType::BuiltIn,
                "none" => ProviderType::None,
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
            #[cfg(feature = "builtin-llm")]
            builtin_model: builtin_model_handle,
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
