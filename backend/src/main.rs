use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    http::{header, HeaderValue, Method},
    Router,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod cache;
pub mod config;
pub mod content;
mod db;
pub mod email;
pub mod events;
mod middleware;
mod rules;
mod vectors;

pub use vectors::config::VectorConfig;

/// Shared application state accessible by all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub vector_service: Arc<vectors::VectorService>,
    pub db: Arc<db::Database>,
    pub redis: Option<Arc<cache::RedisCache>>,
    pub ingestion_broadcast: api::ingestion::IngestionBroadcast,
    pub oauth_manager: Arc<email::oauth::OAuthManager>,
    pub event_bus: events::EventBus,
    /// Chat service for AI-assisted email conversations (R-07).
    /// `None` when no generative model is configured.
    pub chat_service: Option<Arc<vectors::chat::ChatService>>,
    /// RAG pipeline for email-aware chat (ADR-022).
    pub rag_pipeline: Option<Arc<vectors::rag::RagPipeline>>,
    /// Background poll scheduler for periodic email sync.
    pub poll_scheduler: Option<email::poll_scheduler::PollSchedulerHandle>,
    /// YAML configuration loaded from `config/` directory.
    pub yaml_config: Arc<vectors::yaml_config::YamlConfig>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with log-scrubbing safety net (R-05).
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "emailibrium=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(middleware::log_scrub::ScrubLayer)
        .init();

    // ── CLI: --download-model <model_id> (ADR-013, Phase 3) ──────────
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--download-model") {
        if let Some(model_id) = args.get(pos + 1) {
            return vectors::model_download::run_download_model_by_id(model_id)
                .map_err(|e| anyhow::anyhow!("{e}"));
        } else {
            eprintln!("Usage: emailibrium --download-model <model-id>");
            eprintln!("Run 'make models' to see available models.");
            std::process::exit(1);
        }
    }

    // ── CLI: --download-models (ADR-013, item #33) ────────────────────
    if args.iter().any(|a| a == "--download-models") {
        // Parse optional --models-dir and --model flags.
        let models_dir = args
            .windows(2)
            .find(|w| w[0] == "--models-dir")
            .map(|w| w[1].clone());

        let specific_models: Option<Vec<String>> = {
            let model_args: Vec<String> = args
                .windows(2)
                .filter(|w| w[0] == "--model")
                .map(|w| w[1].clone())
                .collect();
            if model_args.is_empty() {
                None
            } else {
                Some(model_args)
            }
        };

        return vectors::model_download::run_download_models_cli(specific_models, models_dir)
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    // ── CLI: --verify-models (ADR-013, item #32) ──────────────────────
    if args.iter().any(|a| a == "--verify-models") {
        let models_dir = args
            .windows(2)
            .find(|w| w[0] == "--models-dir")
            .map(|w| w[1].clone());

        let dir = vectors::model_integrity::resolve_models_dir(models_dir.as_deref());
        let manifest = vectors::model_integrity::ModelManifest::default();
        let results = vectors::model_integrity::verify_all_models(&dir, &manifest);

        println!("Model Integrity Verification");
        println!("============================");
        for result in &results {
            let status = if result.verified { "OK" } else { "FAIL" };
            println!("  [{status}] {}: {}", result.model, result.file_path);
            if let Some(err) = &result.error {
                println!("       {err}");
            }
        }
        let ok_count = results.iter().filter(|r| r.verified).count();
        println!("\n{ok_count}/{} models verified", results.len());
        return Ok(());
    }

    tracing::info!("Starting Emailibrium backend");

    // Load YAML configuration from config/ directory (prompts, tuning, classification, etc.)
    let yaml_config = Arc::new(
        vectors::yaml_config::load_yaml_config("../config").unwrap_or_else(|e| {
            tracing::warn!("Failed to load YAML config: {e} — using defaults");
            vectors::yaml_config::YamlConfig::default()
        }),
    );

    // ── Provider validation summary (Phase 1) ──────────────────────────
    validate_provider_catalog(&yaml_config);

    // Load configuration
    let config = VectorConfig::load()?;

    // Initialize database
    let db = Arc::new(db::Database::connect(&config.database_url).await?);
    db.run_migrations().await?;

    // Initialize Redis cache (optional -- graceful degradation if unavailable)
    let redis = if config.redis.enabled {
        match cache::RedisCache::connect(&config.redis.url).await {
            Ok(cache) => {
                tracing::info!("Redis cache enabled at {}", config.redis.url);
                Some(Arc::new(cache))
            }
            Err(e) => {
                tracing::warn!("Redis unavailable, continuing without cache: {e}");
                None
            }
        }
    } else {
        tracing::info!("Redis cache disabled by configuration");
        None
    };

    // Initialize vector service (pass Redis for L2 embedding cache)
    let vector_service =
        Arc::new(vectors::VectorService::new(config.clone(), db.clone(), redis.clone()).await?);

    // ── Log AI configuration status ────────────────────────────────────
    tracing::info!(
        "Embedding: {} ({})",
        config.embedding.provider,
        config.embedding.model,
    );

    match config.generative.provider.as_str() {
        "builtin" => {
            let model = &config.generative.builtin.model_id;
            let gpu = config.generative.builtin.gpu_layers;

            if vector_service.generative.is_some() {
                tracing::info!(
                    "Generative: builtin ({}, gpu_layers={}) — ready",
                    model,
                    gpu,
                );
            } else {
                tracing::info!(
                    "Generative: builtin ({}) — not available (build with --features builtin-llm)",
                    model,
                );
            }
        }
        "ollama" => {
            tracing::info!(
                "Generative: Ollama ({}/{})",
                config.generative.ollama.classification_model,
                config.generative.ollama.chat_model,
            );
        }
        "cloud" => {
            tracing::info!(
                "Generative: cloud/{} ({})",
                config.generative.cloud.provider,
                config.generative.cloud.model,
            );
        }
        "openrouter" => {
            tracing::info!("Generative: openrouter ({})", config.generative.cloud.model,);
        }
        "none" => {
            tracing::info!("Generative: disabled (rule-based fallback only)");
        }
        other => {
            tracing::warn!(
                "Generative: unknown provider '{}', falling back to rule-based",
                other,
            );
        }
    }

    // Initialize OAuth manager for email account connections (DDD-005)
    let oauth_manager = Arc::new(email::oauth::OAuthManager::new(
        db.pool.clone(),
        config.encryption.master_password.as_deref(),
    ));

    // Initialize domain event bus (Audit Item #20)
    let event_bus = events::EventBus::default_capacity();

    // Register a tracing handler for domain events
    event_bus
        .on_event(std::sync::Arc::new(|envelope: &events::EventEnvelope| {
            tracing::debug!(
                event_type = %envelope.event_type,
                aggregate_id = %envelope.aggregate_id,
                event_id = %envelope.event_id,
                "Domain event published"
            );
        }))
        .await;

    // Initialize chat service using the generative router for provider failover (R-07).
    // Always created — when no backend provider is configured (e.g. "builtin"),
    // the frontend handles chat locally via its own generative router.
    let chat_service = Some(Arc::new(
        vectors::chat::ChatService::new(
            Duration::from_secs(yaml_config.tuning.chat.session_ttl_secs),
            yaml_config.tuning.chat.max_history_messages,
            vector_service.generative_router.clone(),
        )
        .with_system_prompt(yaml_config.prompts.chat_assistant.clone()),
    ));

    // Initialize RAG pipeline for email-aware chat (ADR-022, DDD-010).
    let rag_pipeline = Some(Arc::new(vectors::rag::RagPipeline::new(
        vector_service.hybrid_search.clone(),
        db.clone(),
        vector_service.config.rag.clone(),
    )));

    let state = AppState {
        vector_service,
        db,
        redis,
        ingestion_broadcast: api::ingestion::IngestionBroadcast::default(),
        oauth_manager: oauth_manager.clone(),
        event_bus,
        chat_service,
        rag_pipeline,
        poll_scheduler: None, // Initialized below after state creation.
        yaml_config: yaml_config.clone(),
    };

    // Start the background email poll scheduler.
    // The sync closure bridges the poll scheduler (lib crate) to the ingestion
    // code (binary crate) by capturing `state` and calling the same flow as
    // POST /api/v1/ingestion/start.
    let sync_state = state.clone();
    let sync_fn: email::poll_scheduler::SyncAccountFn = std::sync::Arc::new(move |account_id| {
        let s = sync_state.clone();
        Box::pin(async move {
            // Phase 0: Sync from provider.
            let synced = api::ingestion::sync_emails_from_provider(&s, &account_id)
                .await
                .map_err(|(_status, msg)| msg)?;

            // Phase 1+: Run ingestion pipeline on pending emails.
            // Always attempt ingestion — there may be pending embeddings from
            // a prior incomplete run even when no new emails were synced.
            if let Err(e) = s
                .vector_service
                .ingestion_pipeline
                .start_ingestion(&account_id)
                .await
            {
                // "already in progress" is expected when a job is running; don't warn.
                let msg = e.to_string();
                if !msg.contains("already in progress") {
                    tracing::warn!(
                        account_id = %account_id,
                        "Poll scheduler: ingestion pipeline failed: {e}"
                    );
                }
            }
            Ok(synced)
        })
    });
    let poll_handle = email::poll_scheduler::start(oauth_manager, sync_fn);
    let state = AppState {
        poll_scheduler: Some(poll_handle),
        ..state
    };

    // ── CORS middleware (audit item #6) ────────────────────────────────
    let origins: Vec<HeaderValue> = config
        .security
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
        .allow_credentials(true);

    // Build router
    let mut app = Router::new()
        .nest("/api/v1", api::routes())
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors);

    // ── Security headers (audit item #13 — CSP + hardening) ──────────
    // Uses the comprehensive security_headers_middleware which sets CSP,
    // X-Content-Type-Options, X-Frame-Options, X-XSS-Protection,
    // Referrer-Policy, Permissions-Policy, and HSTS in one middleware.
    if config.security.csp_enabled {
        // Exercise SecurityHeadersConfig::from_env() for env-based overrides
        let _sec_cfg = middleware::security_headers::SecurityHeadersConfig::from_env();
        app = app.layer(axum::middleware::from_fn(
            middleware::security_headers::security_headers_middleware,
        ));
    }

    // ── HSTS header (R-05) ────────────────────────────────────────────
    if config.security.hsts.enabled {
        app = app.layer(middleware::hsts::hsts_layer(
            config.security.hsts.max_age_secs,
            config.security.hsts.include_subdomains,
        ));
        tracing::info!(
            "HSTS enabled (max-age={}s, includeSubDomains={})",
            config.security.hsts.max_age_secs,
            config.security.hsts.include_subdomains,
        );
    }

    // ── Log scrubbing middleware (R-05) ─────────────────────────────
    app = app.layer(axum::middleware::from_fn(
        middleware::log_scrubbing::log_scrubbing_middleware,
    ));

    // ── Rate limiting (R-05) ──────────────────────────────────────────
    if config.security.rate_limit.enabled {
        let rl_config = middleware::rate_limit::RateLimitConfig::from_env();
        let (capacity, refill_rate) = rl_config.get_capacity_and_rate("global");
        let limiter = std::sync::Arc::new(middleware::rate_limit::RateLimiter::new_in_memory(
            capacity,
            refill_rate,
            "global".to_string(),
        ));
        app = app
            .layer(axum::middleware::from_fn(
                middleware::rate_limit::rate_limit_middleware,
            ))
            .layer(axum::Extension(limiter));
        tracing::info!(
            "Rate limiting enabled (capacity={}, refill_rate={:.2}/s, preset=env)",
            capacity,
            refill_rate,
        );
    }

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Log a startup validation summary for each provider in the model catalogs.
///
/// - **builtin**: checks if GGUF files exist in the HuggingFace cache or local cache dir.
/// - **ollama**: pings the base URL (non-blocking, just logs).
/// - **cloud providers**: checks if the required API key env var is set.
fn validate_provider_catalog(yaml_config: &vectors::yaml_config::YamlConfig) {
    // ── LLM catalog ────────────────────────────────────────────────────
    let hf_cache = dirs::home_dir()
        .map(|h| h.join(".cache/huggingface/hub"))
        .unwrap_or_default();

    for (provider_name, provider) in &yaml_config.llm_catalog.providers {
        let total = provider.models.len();

        match provider_name.as_str() {
            "builtin" => {
                let mut cached_count = 0u32;
                for model in &provider.models {
                    let repo = model.repo_id.as_deref().unwrap_or("");
                    let file = model.filename.as_deref().unwrap_or("");
                    let cache_key = repo.replace('/', "--");
                    let in_hf = hf_cache.join(format!("models--{cache_key}")).exists();
                    // Also check the default local cache dir.
                    let in_local = if !file.is_empty() {
                        std::path::Path::new(file).exists()
                    } else {
                        false
                    };
                    let is_cached = in_hf || in_local;
                    if is_cached {
                        cached_count += 1;
                    }
                    tracing::debug!(
                        model_id = %model.id,
                        cached = is_cached,
                        "builtin model cache status"
                    );
                }
                tracing::info!(
                    "Provider builtin: {total} models configured, {cached_count} cached"
                );
            }
            "ollama" => {
                let base_url = provider
                    .base_url
                    .as_deref()
                    .unwrap_or("http://localhost:11434");
                tracing::info!("Provider ollama: {total} models configured, base_url={base_url}");
                // Non-blocking ping — spawn a task so it doesn't delay startup.
                let url = format!("{base_url}/api/tags");
                tokio::spawn(async move {
                    match reqwest::Client::new()
                        .get(&url)
                        .timeout(std::time::Duration::from_secs(3))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            tracing::info!("Provider ollama: reachable at {url}");
                        }
                        Ok(resp) => {
                            tracing::warn!(
                                "Provider ollama: responded with status {} at {url}",
                                resp.status()
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Provider ollama: not reachable at {url} — {e}");
                        }
                    }
                });
            }
            _ => {
                // Cloud providers (openai, anthropic, openrouter, etc.)
                if let Some(env_var) = &provider.api_key_env {
                    if env_var.is_empty() {
                        tracing::info!(
                            "Provider {provider_name}: {total} models configured, no API key env var specified"
                        );
                    } else if std::env::var(env_var).is_ok() {
                        tracing::info!(
                            "Provider {provider_name}: {total} models configured, API key set ({env_var})"
                        );
                    } else {
                        tracing::warn!(
                            "Provider {provider_name}: {total} models configured, API key NOT set ({env_var})"
                        );
                    }
                } else {
                    tracing::info!("Provider {provider_name}: {total} models configured");
                }
            }
        }
    }

    // ── Embedding catalog ──────────────────────────────────────────────
    for (provider_name, provider) in &yaml_config.embedding_catalog.providers {
        let total = provider.models.len();

        if let Some(env_var) = &provider.api_key_env {
            if !env_var.is_empty() {
                if std::env::var(env_var).is_ok() {
                    tracing::info!(
                        "Embedding provider {provider_name}: {total} models configured, API key set ({env_var})"
                    );
                } else {
                    tracing::warn!(
                        "Embedding provider {provider_name}: {total} models configured, API key NOT set ({env_var})"
                    );
                }
                continue;
            }
        }
        tracing::info!("Embedding provider {provider_name}: {total} models configured");
    }
}
