//! API routes for Emailibrium.

pub mod ingestion;
mod insights;
mod vectors;

use crate::AppState;
use axum::Router;

/// Build all API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .nest("/vectors", vectors::routes())
        .nest("/ingestion", ingestion::routes())
        .nest("/insights", insights::routes())
}
