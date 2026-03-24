//! AI model management API endpoints (ADR-013).
//!
//! - GET /api/v1/ai/models         — list all known models with download/active status
//! - GET /api/v1/ai/status         — current AI subsystem status
//! - GET /api/v1/ai/reindex-status — progress of any in-flight re-index

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::vectors::models::{self, ModelStatus};
use crate::vectors::reindex::ReindexStatus;
use crate::AppState;

/// Build AI management routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/status", get(ai_status))
        .route("/reindex-status", get(reindex_status))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all known models with their current status.
async fn list_models(State(state): State<AppState>) -> Json<Vec<ModelStatus>> {
    let vs = &state.vector_service;
    let active_model = &vs.config.embedding.model;
    let cache_dir = &vs.config.store.path;
    let statuses = models::get_model_statuses(active_model, cache_dir);
    Json(statuses)
}

/// Overall AI subsystem status response.
#[derive(Debug, Serialize)]
struct AiStatusResponse {
    /// Currently active embedding model name.
    active_model: String,
    /// Embedding dimensions for the active model.
    dimensions: usize,
    /// Embedding provider backend in use.
    provider: String,
    /// Whether the embedding pipeline is available.
    embedding_available: bool,
    /// Whether a re-index is currently in progress.
    reindex_in_progress: bool,
}

/// Get overall AI subsystem status.
async fn ai_status(State(state): State<AppState>) -> Json<AiStatusResponse> {
    let vs = &state.vector_service;
    let embedding_available = vs.embedding.is_available().await;
    let reindex_status = vs.reindex_orchestrator.get_status().await;

    Json(AiStatusResponse {
        active_model: vs.config.embedding.model.clone(),
        dimensions: vs.config.embedding.dimensions,
        provider: vs.config.embedding.provider.clone(),
        embedding_available,
        reindex_in_progress: reindex_status.in_progress,
    })
}

/// Get the current re-indexing progress.
async fn reindex_status(State(state): State<AppState>) -> Json<ReindexStatus> {
    let status = state.vector_service.reindex_orchestrator.get_status().await;
    Json(status)
}
