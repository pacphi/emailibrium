//! API routes for Emailibrium.

pub mod accounts;
pub mod ai;
pub mod attachments;
mod backup;
mod clustering;
mod consent;
mod emails;
mod evaluation;
pub mod ingestion;
mod insights;
mod interactions;
mod learning;
pub mod provider_helpers;
mod rules;
mod unsubscribe;
mod vectors;
mod wipe;

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
        .nest("/auth", accounts::routes())
        .nest("/emails", emails::routes())
        .nest("/rules", rules::routes())
        .nest("/unsubscribe", unsubscribe::routes())
        .nest("/cleanup", crate::cleanup::routes())
        .nest("/wipe", wipe::routes())
}
