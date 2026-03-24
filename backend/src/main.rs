use std::sync::Arc;

use axum::{
    http::{header, HeaderValue, Method},
    Router,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod cache;
mod db;
pub mod email;
pub mod events;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "emailibrium=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
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

    let state = AppState {
        vector_service,
        db,
        redis,
        ingestion_broadcast: api::ingestion::IngestionBroadcast::default(),
        oauth_manager,
        event_bus,
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

    // ── Security headers (audit item #13 — CSP + hardening) ──────────
    let backend_origin = format!("http://{}:{}", config.host, config.port);
    let connect_src = config
        .security
        .allowed_origins
        .iter()
        .chain(std::iter::once(&backend_origin))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let csp_value = format!(
        "default-src 'self'; \
         script-src 'self'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; \
         connect-src 'self' {connect_src}"
    );

    // Build router
    let mut app = Router::new()
        .nest("/api/v1", api::routes())
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors);

    if config.security.csp_enabled {
        app = app
            .layer(SetResponseHeaderLayer::overriding(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_str(&csp_value).expect("valid CSP header value"),
            ))
            .layer(SetResponseHeaderLayer::overriding(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ))
            .layer(SetResponseHeaderLayer::overriding(
                header::X_FRAME_OPTIONS,
                HeaderValue::from_static("DENY"),
            ))
            .layer(SetResponseHeaderLayer::overriding(
                header::X_XSS_PROTECTION,
                HeaderValue::from_static("1; mode=block"),
            ))
            .layer(SetResponseHeaderLayer::overriding(
                header::REFERRER_POLICY,
                HeaderValue::from_static("strict-origin-when-cross-origin"),
            ));
    }

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
