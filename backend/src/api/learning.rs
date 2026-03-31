//! Learning API endpoints (S3-03 .. S3-06).
//!
//! - POST /api/v1/learning/feedback      — submit user feedback
//! - GET  /api/v1/learning/metrics       — get learning metrics
//! - POST /api/v1/learning/consolidate   — trigger session consolidation

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::vectors::learning::{FeedbackAction, UserFeedback};
use crate::vectors::types::EmailCategory;
use crate::AppState;

/// Build learning API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/feedback", post(submit_feedback))
        .route("/metrics", get(get_metrics))
        .route("/consolidate", post(consolidate))
        .route("/session", get(get_session_state))
}

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub email_id: String,
    pub action: FeedbackActionRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedbackActionRequest {
    Reclassify {
        from: EmailCategory,
        to: EmailCategory,
    },
    MoveToGroup {
        group_id: String,
    },
    Star,
    Reply {
        delay_secs: Option<u64>,
    },
    Archive,
    Delete,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub quality: f32,
    pub centroid_updated: bool,
    pub safeguard_triggered: bool,
}

#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub total_feedback: u64,
    pub rank1_clicks: u64,
    pub total_clicks: u64,
    pub centroid_drift: HashMap<String, f32>,
    pub ab_control_queries: u64,
    pub ab_sona_queries: u64,
}

#[derive(Debug, Serialize)]
pub struct ConsolidateResponse {
    pub centroids_updated: u32,
    pub emails_reclassified: u32,
    pub new_clusters: u32,
    pub duration_ms: u64,
}

// --- Handlers ---

/// POST /api/v1/learning/feedback
async fn submit_feedback(
    State(state): State<AppState>,
    Json(req): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, (StatusCode, String)> {
    // If this is a reclassification, persist the new category to the DB immediately
    // so the user sees the change on refetch.
    if let FeedbackActionRequest::Reclassify { ref to, .. } = req.action {
        sqlx::query(
            "UPDATE emails SET category = ?, category_method = 'user_reclassify' WHERE id = ?",
        )
        .bind(to.to_string())
        .bind(&req.email_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let action = match req.action {
        FeedbackActionRequest::Reclassify { from, to } => FeedbackAction::Reclassify { from, to },
        FeedbackActionRequest::MoveToGroup { group_id } => FeedbackAction::MoveToGroup { group_id },
        FeedbackActionRequest::Star => FeedbackAction::Star,
        FeedbackActionRequest::Reply { delay_secs } => FeedbackAction::Reply {
            delay_secs: delay_secs.unwrap_or(0),
        },
        FeedbackActionRequest::Archive => FeedbackAction::Archive,
        FeedbackActionRequest::Delete => FeedbackAction::Delete,
    };

    let feedback = UserFeedback {
        email_id: req.email_id,
        action,
        timestamp: chrono::Utc::now(),
    };

    let result = state
        .vector_service
        .learning_engine
        .on_user_feedback(feedback)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(FeedbackResponse {
        quality: result.quality,
        centroid_updated: result.centroid_updated,
        safeguard_triggered: result.safeguard_triggered,
    }))
}

/// GET /api/v1/learning/metrics
async fn get_metrics(
    State(state): State<AppState>,
) -> Result<Json<MetricsResponse>, (StatusCode, String)> {
    let metrics = state.vector_service.learning_engine.get_metrics().await;

    Ok(Json(MetricsResponse {
        total_feedback: metrics.total_feedback,
        rank1_clicks: metrics.rank1_clicks,
        total_clicks: metrics.total_clicks,
        centroid_drift: metrics.centroid_drift,
        ab_control_queries: metrics.ab_control_queries,
        ab_sona_queries: metrics.ab_sona_queries,
    }))
}

/// POST /api/v1/learning/consolidate
async fn consolidate(
    State(state): State<AppState>,
) -> Result<Json<ConsolidateResponse>, (StatusCode, String)> {
    let report = state
        .vector_service
        .learning_engine
        .hourly_consolidation()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ConsolidateResponse {
        centroids_updated: report.centroids_updated,
        emails_reclassified: report.emails_reclassified,
        new_clusters: report.new_clusters,
        duration_ms: report.duration_ms,
    }))
}

// --- Session learning endpoint (SONA Tier 2) ---

#[derive(Debug, Serialize)]
pub struct SessionStateResponse {
    pub session_id: String,
    pub age_seconds: i64,
    pub interaction_count: u32,
    pub clicked_count: usize,
    pub skipped_count: usize,
    pub has_preference_vector: bool,
    pub preference_vector_dims: Option<usize>,
    /// Sample rerank boost for a zero vector (diagnostic).
    pub sample_rerank_boost: f32,
}

/// GET /api/v1/learning/session
///
/// Returns the current SONA Tier 2 session state including preference vector
/// availability and rerank boost diagnostics.
async fn get_session_state(
    State(state): State<AppState>,
) -> Result<Json<SessionStateResponse>, (StatusCode, String)> {
    let session = state.vector_service.learning_engine.get_session().await;

    let session_id = session.session_id().to_string();
    let age_seconds = session.age().num_seconds();
    let pref = session.preference_vector();
    let dims = state.vector_service.config.embedding.dimensions;

    // Compute a diagnostic rerank boost against a zero vector.
    let zero_vec = vec![0.0f32; dims];
    let gamma = state.vector_service.config.learning.session_rerank_gamma;
    let sample_boost = session.rerank_boost(&zero_vec, gamma);

    Ok(Json(SessionStateResponse {
        session_id,
        age_seconds,
        interaction_count: session.interaction_count,
        clicked_count: session.clicked_embeddings.len(),
        skipped_count: session.skipped_embeddings.len(),
        has_preference_vector: pref.is_some(),
        preference_vector_dims: pref.as_ref().map(|v| v.len()),
        sample_rerank_boost: sample_boost,
    }))
}
