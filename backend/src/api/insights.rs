//! Insight API endpoints (S2-06).
//!
//! - GET /api/v1/insights/subscriptions — detected subscriptions
//! - GET /api/v1/insights/recurring     — recurring sender analysis
//! - GET /api/v1/insights/report        — aggregated inbox report
//! - GET /api/v1/insights/temporal      — temporal analytics (volume, categories, day/hour)

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;

use crate::vectors::insights::InsightEngine;
use crate::AppState;

// ---------------------------------------------------------------------------
// Temporal analytics types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalInsights {
    /// Daily email counts for last 90 days.
    pub daily_volume: Vec<DailyCount>,
    /// Category counts per day for last 90 days.
    pub category_daily: Vec<CategoryDailyCount>,
    /// Email count by day of week (0=Sunday, 6=Saturday).
    pub day_of_week: Vec<DayOfWeekCount>,
    /// Email count by hour of day (0-23).
    pub hour_of_day: Vec<HourOfDayCount>,
}

#[derive(Debug, Serialize)]
pub struct DailyCount {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDailyCount {
    pub date: String,
    pub category: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct DayOfWeekCount {
    pub day: i32,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct HourOfDayCount {
    pub hour: i32,
    pub count: i64,
}

/// Build insight API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscriptions", get(subscriptions))
        .route("/recurring-senders", get(recurring))
        .route("/report", get(report))
        .route("/temporal", get(temporal_insights))
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
) -> Result<Json<Vec<crate::vectors::insights::SubscriptionInsight>>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    // Reuse the subscription detection which returns full SubscriptionInsight
    // (senderAddress, senderDomain, frequency, lastSeen, etc.) — matching
    // what the frontend SendersPanel and TopicsPanel expect.
    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(subs))
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

/// GET /api/v1/insights/temporal
async fn temporal_insights(
    State(state): State<AppState>,
) -> Result<Json<TemporalInsights>, (StatusCode, String)> {
    // 1. Daily volume for 90 days
    let daily_volume: Vec<DailyCount> = sqlx::query_as::<_, (String, i64)>(
        "SELECT DATE(received_at) as date, COUNT(*) as count \
         FROM emails WHERE received_at >= DATE('now', '-90 days') \
         GROUP BY DATE(received_at) ORDER BY date ASC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .into_iter()
    .map(|(date, count)| DailyCount { date, count })
    .collect();

    // 2. Category per day for 90 days
    let category_daily: Vec<CategoryDailyCount> = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT DATE(received_at) as date, COALESCE(category, 'Uncategorized') as category, \
         COUNT(*) as count FROM emails WHERE received_at >= DATE('now', '-90 days') \
         GROUP BY date, category ORDER BY date ASC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .into_iter()
    .map(|(date, category, count)| CategoryDailyCount {
        date,
        category,
        count,
    })
    .collect();

    // 3. Day of week distribution
    let day_of_week: Vec<DayOfWeekCount> = sqlx::query_as::<_, (i32, i64)>(
        "SELECT CAST(STRFTIME('%w', received_at) AS INTEGER) as day, COUNT(*) as count \
         FROM emails GROUP BY day ORDER BY day ASC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .into_iter()
    .map(|(day, count)| DayOfWeekCount { day, count })
    .collect();

    // 4. Hour of day distribution
    let hour_of_day: Vec<HourOfDayCount> = sqlx::query_as::<_, (i32, i64)>(
        "SELECT CAST(STRFTIME('%H', received_at) AS INTEGER) as hour, COUNT(*) as count \
         FROM emails GROUP BY hour ORDER BY hour ASC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .into_iter()
    .map(|(hour, count)| HourOfDayCount { hour, count })
    .collect();

    Ok(Json(TemporalInsights {
        daily_volume,
        category_daily,
        day_of_week,
        hour_of_day,
    }))
}
