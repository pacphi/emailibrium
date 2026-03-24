//! Insight API endpoints (S2-06).
//!
//! - GET /api/v1/insights/subscriptions — detected subscriptions
//! - GET /api/v1/insights/recurring     — recurring sender analysis
//! - GET /api/v1/insights/report        — aggregated inbox report

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};

use crate::vectors::insights::InsightEngine;
use crate::AppState;

/// Build insight API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscriptions", get(subscriptions))
        .route("/recurring", get(recurring))
        .route("/report", get(report))
}

/// GET /api/v1/insights/subscriptions
async fn subscriptions(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::vectors::insights::SubscriptionInsight>>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(subs))
}

/// GET /api/v1/insights/recurring
async fn recurring(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::vectors::insights::RecurringSenderInsight>>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    let senders = engine
        .analyze_recurring_senders()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(senders))
}

/// GET /api/v1/insights/report
async fn report(
    State(state): State<AppState>,
) -> Result<Json<crate::vectors::insights::InboxReport>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    let report = engine
        .generate_report()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(report))
}
