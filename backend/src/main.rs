use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod db;
mod vectors;

pub use vectors::config::VectorConfig;

/// Shared application state accessible by all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub vector_service: Arc<vectors::VectorService>,
    pub db: Arc<db::Database>,
    pub ingestion_broadcast: api::ingestion::IngestionBroadcast,
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

    tracing::info!("Starting Emailibrium backend");

    // Load configuration
    let config = VectorConfig::load()?;

    // Initialize database
    let db = Arc::new(db::Database::connect(&config.database_url).await?);
    db.run_migrations().await?;

    // Initialize vector service
    let vector_service = Arc::new(vectors::VectorService::new(config.clone(), db.clone()).await?);

    let state = AppState {
        vector_service,
        db,
        ingestion_broadcast: api::ingestion::IngestionBroadcast::default(),
    };

    // Build router
    let app = Router::new()
        .nest("/api/v1", api::routes())
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
