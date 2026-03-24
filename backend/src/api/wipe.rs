//! Remote wipe API endpoints (ADR-008: Device Loss Mitigation).
//!
//! - POST   /api/v1/wipe/user/{user_id}        — wipe user data
//! - POST   /api/v1/wipe/vectors                — wipe all vectors
//! - POST   /api/v1/wipe/all                    — full wipe (with confirmation)
//! - GET    /api/v1/wipe/scheduled              — list scheduled wipes
//! - DELETE /api/v1/wipe/scheduled/{user_id}    — cancel scheduled wipe

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::vectors::remote_wipe::{ScheduledWipe, WipeResult};
use crate::AppState;

/// Build wipe API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/user/{user_id}", post(wipe_user))
        .route("/vectors", post(wipe_vectors))
        .route("/all", post(wipe_all))
        .route("/scheduled", get(list_scheduled))
        .route("/scheduled/{user_id}", delete(cancel_scheduled))
}

// --- Request / Response types ---

/// Request body for full platform wipe (requires confirmation token).
#[derive(Debug, Deserialize)]
pub struct WipeAllRequest {
    /// Must be the string "CONFIRM_FULL_WIPE" to proceed.
    pub confirmation_token: String,
}

/// Request body for scheduling a delayed wipe.
#[derive(Debug, Deserialize)]
pub struct ScheduleWipeRequest {
    /// Delay in seconds before the wipe executes.
    pub delay_seconds: i64,
}

/// Wipe operation response.
#[derive(Debug, Serialize)]
pub struct WipeResponse {
    pub message: String,
    #[serde(flatten)]
    pub result: WipeResult,
}

/// Scheduled wipes list response.
#[derive(Debug, Serialize)]
pub struct ScheduledWipesResponse {
    pub scheduled_wipes: Vec<ScheduledWipe>,
    pub count: usize,
}

/// Cancel result response.
#[derive(Debug, Serialize)]
pub struct CancelWipeResponse {
    pub cancelled: bool,
    pub message: String,
}

// --- Handlers ---

/// POST /api/v1/wipe/user/{user_id}
///
/// Wipe all data for a specific user. Requires an authenticated session.
async fn wipe_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<WipeResponse>, (StatusCode, String)> {
    if user_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "user_id must not be empty".to_string(),
        ));
    }

    // Sanitize user_id: only allow alphanumeric, hyphens, underscores.
    if !user_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "user_id contains invalid characters".to_string(),
        ));
    }

    let result = state
        .vector_service
        .remote_wipe_service
        .wipe_user_data(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(WipeResponse {
        message: format!("User data wiped for {}", user_id),
        result,
    }))
}

/// POST /api/v1/wipe/vectors
///
/// Wipe all vector store data, keeping config and metadata.
async fn wipe_vectors(
    State(state): State<AppState>,
) -> Result<Json<WipeResponse>, (StatusCode, String)> {
    let result = state
        .vector_service
        .remote_wipe_service
        .wipe_vectors_only()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(WipeResponse {
        message: "Vector store data wiped".to_string(),
        result,
    }))
}

/// POST /api/v1/wipe/all
///
/// Full platform data wipe (admin only). Requires confirmation token.
async fn wipe_all(
    State(state): State<AppState>,
    Json(body): Json<WipeAllRequest>,
) -> Result<Json<WipeResponse>, (StatusCode, String)> {
    if body.confirmation_token != "CONFIRM_FULL_WIPE" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid confirmation token. Send {\"confirmation_token\": \"CONFIRM_FULL_WIPE\"}"
                .to_string(),
        ));
    }

    let result = state
        .vector_service
        .remote_wipe_service
        .wipe_all_data()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(WipeResponse {
        message: "Full platform data wipe completed".to_string(),
        result,
    }))
}

/// GET /api/v1/wipe/scheduled
///
/// List all pending scheduled wipes.
async fn list_scheduled(
    State(state): State<AppState>,
) -> Result<Json<ScheduledWipesResponse>, (StatusCode, String)> {
    let wipes = state
        .vector_service
        .remote_wipe_service
        .list_scheduled_wipes()
        .await;

    let count = wipes.len();
    Ok(Json(ScheduledWipesResponse {
        scheduled_wipes: wipes,
        count,
    }))
}

/// DELETE /api/v1/wipe/scheduled/{user_id}
///
/// Cancel a pending scheduled wipe for a user.
async fn cancel_scheduled(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<CancelWipeResponse>, (StatusCode, String)> {
    if user_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "user_id must not be empty".to_string(),
        ));
    }

    let cancelled = state
        .vector_service
        .remote_wipe_service
        .cancel_scheduled_wipe(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let message = if cancelled {
        format!("Scheduled wipe for {} cancelled", user_id)
    } else {
        format!("No scheduled wipe found for {}", user_id)
    };

    Ok(Json(CancelWipeResponse { cancelled, message }))
}
