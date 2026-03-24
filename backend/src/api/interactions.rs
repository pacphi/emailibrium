//! Interaction tracking API endpoints (S3-08).
//!
//! - POST /api/v1/interactions/search        — record a search query
//! - POST /api/v1/interactions/:id/click     — record click on result
//! - POST /api/v1/interactions/:id/feedback  — record relevance feedback
//! - GET  /api/v1/interactions/recent        — get recent interactions

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Build interaction API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/search", post(record_search))
        .route("/{id}/click", post(record_click))
        .route("/{id}/feedback", post(record_feedback))
        .route("/recent", get(recent_interactions))
}

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
pub struct RecordSearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct RecordSearchResponse {
    pub interaction_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RecordClickRequest {
    pub email_id: String,
    pub rank: u32,
}

#[derive(Debug, Serialize)]
pub struct ClickResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct RecordFeedbackRequest {
    pub email_id: String,
    pub feedback: String,
}

#[derive(Debug, Serialize)]
pub struct InteractionFeedbackResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct RecentQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct InteractionItem {
    pub id: String,
    pub query_text: String,
    pub result_email_id: String,
    pub result_rank: u32,
    pub clicked: bool,
    pub feedback: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct RecentInteractionsResponse {
    pub interactions: Vec<InteractionItem>,
    pub total: usize,
}

// --- Handlers ---

/// POST /api/v1/interactions/search
async fn record_search(
    State(state): State<AppState>,
    Json(req): Json<RecordSearchRequest>,
) -> Result<Json<RecordSearchResponse>, (StatusCode, String)> {
    let interaction_id = state
        .vector_service
        .interaction_tracker
        .record_search(&req.query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(RecordSearchResponse { interaction_id }))
}

/// POST /api/v1/interactions/:id/click
async fn record_click(
    State(state): State<AppState>,
    Path(interaction_id): Path<String>,
    Json(req): Json<RecordClickRequest>,
) -> Result<Json<ClickResponse>, (StatusCode, String)> {
    state
        .vector_service
        .interaction_tracker
        .record_click(&interaction_id, &req.email_id, req.rank)
        .await
        .map_err(|e| match e {
            crate::vectors::error::VectorError::NotFound(_) => {
                (StatusCode::NOT_FOUND, e.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    Ok(Json(ClickResponse { success: true }))
}

/// POST /api/v1/interactions/:id/feedback
async fn record_feedback(
    State(state): State<AppState>,
    Path(interaction_id): Path<String>,
    Json(req): Json<RecordFeedbackRequest>,
) -> Result<Json<InteractionFeedbackResponse>, (StatusCode, String)> {
    state
        .vector_service
        .interaction_tracker
        .record_feedback(&interaction_id, &req.email_id, &req.feedback)
        .await
        .map_err(|e| match e {
            crate::vectors::error::VectorError::NotFound(_) => {
                (StatusCode::NOT_FOUND, e.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    Ok(Json(InteractionFeedbackResponse { success: true }))
}

/// GET /api/v1/interactions/recent
async fn recent_interactions(
    State(state): State<AppState>,
    Query(params): Query<RecentQuery>,
) -> Result<Json<RecentInteractionsResponse>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(50);

    let interactions = state
        .vector_service
        .interaction_tracker
        .get_interactions(limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<InteractionItem> = interactions
        .iter()
        .map(|i| InteractionItem {
            id: i.id.clone(),
            query_text: i.query_text.clone(),
            result_email_id: i.result_email_id.clone(),
            result_rank: i.result_rank,
            clicked: i.clicked,
            feedback: i.feedback.clone(),
            created_at: i.created_at.to_rfc3339(),
        })
        .collect();

    let total = items.len();

    Ok(Json(RecentInteractionsResponse {
        interactions: items,
        total,
    }))
}
