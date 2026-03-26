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

    // ── CLI: --download-models (ADR-013, item #33) ────────────────────
    let args: Vec<String> = std::env::args().collect();
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

    // Initialize chat service using the generative router for provider failover (R-07)
    let chat_service = if vector_service.generative.is_some() {
        Some(Arc::new(vectors::chat::ChatService::new(
            Duration::from_secs(3600), // 1 hour session TTL
            20,                        // 20-message sliding window
            vector_service.generative_router.clone(),
        )))
    } else {
        None
    };

    let state = AppState {
        vector_service,
        db,
        redis,
        ingestion_broadcast: api::ingestion::IngestionBroadcast::default(),
        oauth_manager,
        event_bus,
        chat_service,
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
