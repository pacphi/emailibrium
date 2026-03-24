//! API routes for Emailibrium.

mod ai;
mod backup;
mod clustering;
mod consent;
mod evaluation;
pub mod ingestion;
mod insights;
mod interactions;
mod learning;
mod vectors;

use crate::AppState;
use axum::Router;

/// Build all API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .nest("/vectors", vectors::routes())
        .nest("/ingestion", ingestion::routes())
        .nest("/insights", insights::routes())
        .nest("/clustering", clustering::routes())
        .nest("/learning", learning::routes())
        .nest("/interactions", interactions::routes())
        .nest("/evaluation", evaluation::routes())
        .nest("/backup", backup::routes())
        .nest("/ai", ai::routes())
        .nest("/consent", consent::routes())
}
