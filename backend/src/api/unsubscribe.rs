//! Bulk unsubscribe API endpoints (R-04).
//!
//! - POST /api/v1/unsubscribe               — batch unsubscribe
//! - POST /api/v1/unsubscribe/undo/:batch_id — undo a batch (within 5-min window)
//! - GET  /api/v1/unsubscribe/preview        — preview what would happen

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::email::unsubscribe::{
    BatchResult, SubscriptionTarget, UnsubscribePreview, UnsubscribeService,
};
use crate::AppState;

/// Build unsubscribe API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(batch_unsubscribe))
        .route("/undo/{batch_id}", post(undo_batch))
        .route("/preview", get(preview).post(preview_post))
}

// --- Request / Response types ---

/// Request body for batch unsubscribe.
#[derive(Debug, Deserialize)]
pub struct BatchUnsubscribeRequest {
    pub subscriptions: Vec<SubscriptionTarget>,
}

/// Request body for preview (POST variant for complex queries).
#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub subscriptions: Vec<SubscriptionTarget>,
    /// Optional engagement rates per sender (0.0-1.0).
    #[serde(default)]
    pub engagement_rates: HashMap<String, f32>,
}

/// Query parameters for GET preview.
#[derive(Debug, Deserialize)]
pub struct PreviewQueryParams {
    /// Comma-separated sender addresses to preview.
    pub senders: Option<String>,
}

/// Response for batch unsubscribe.
#[derive(Debug, Serialize)]
pub struct BatchUnsubscribeResponse {
    #[serde(flatten)]
    pub result: BatchResult,
}

/// Response for undo operation.
#[derive(Debug, Serialize)]
pub struct UndoResponse {
    pub batch_id: String,
    pub status: String,
    pub message: String,
}

/// Response for preview.
#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub previews: Vec<UnsubscribePreview>,
    pub total: usize,
}

// --- Handlers ---

/// POST /api/v1/unsubscribe
///
/// Execute a batch unsubscribe for the provided subscription targets.
async fn batch_unsubscribe(
    State(state): State<AppState>,
    Json(req): Json<BatchUnsubscribeRequest>,
) -> Result<Json<BatchUnsubscribeResponse>, (StatusCode, String)> {
    if req.subscriptions.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "At least one subscription target is required".to_string(),
        ));
    }

    if req.subscriptions.len() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Maximum 100 subscriptions per batch".to_string(),
        ));
    }

    let service = state
        .vector_service
        .unsubscribe_service
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Unsubscribe service not available".to_string(),
            )
        })?;

    let result = service.batch_unsubscribe(req.subscriptions).await;

    Ok(Json(BatchUnsubscribeResponse { result }))
}

/// POST /api/v1/unsubscribe/undo/:batch_id
///
/// Undo a previous batch unsubscribe (within the 5-minute undo window).
async fn undo_batch(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Result<Json<UndoResponse>, (StatusCode, String)> {
    if batch_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "batch_id must not be empty".to_string(),
        ));
    }

    let service = state
        .vector_service
        .unsubscribe_service
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Unsubscribe service not available".to_string(),
            )
        })?;

    service.undo(&batch_id).await.map_err(|e| {
        if e.contains("expired") {
            (StatusCode::GONE, e)
        } else if e.contains("No undo entry") {
            (StatusCode::NOT_FOUND, e)
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, e)
        }
    })?;

    Ok(Json(UndoResponse {
        batch_id: batch_id.clone(),
        status: "undone".to_string(),
        message: format!(
            "Batch '{}' undo initiated. HTTP-based unsubscribes may require \
             manual re-subscription.",
            batch_id
        ),
    }))
}

/// GET /api/v1/unsubscribe/preview?senders=a@x.com,b@y.com
///
/// Preview what would happen for the given senders (simplified).
async fn preview(
    State(state): State<AppState>,
    Query(params): Query<PreviewQueryParams>,
) -> Result<Json<PreviewResponse>, (StatusCode, String)> {
    let service = state
        .vector_service
        .unsubscribe_service
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Unsubscribe service not available".to_string(),
            )
        })?;

    let targets: Vec<SubscriptionTarget> = params
        .senders
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| SubscriptionTarget {
            sender: s.trim().to_string(),
            list_unsubscribe_header: None,
            list_unsubscribe_post: None,
            email_id: None,
        })
        .collect();

    let previews = service.preview(&targets, &HashMap::new());
    let total = previews.len();

    Ok(Json(PreviewResponse { previews, total }))
}

/// POST /api/v1/unsubscribe/preview
///
/// Preview with full subscription targets and engagement data.
async fn preview_post(
    State(state): State<AppState>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, (StatusCode, String)> {
    let service = state
        .vector_service
        .unsubscribe_service
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Unsubscribe service not available".to_string(),
            )
        })?;

    let previews = service.preview(&req.subscriptions, &req.engagement_rates);
    let total = previews.len();

    Ok(Json(PreviewResponse { previews, total }))
}
