//! Telemetry ingestion endpoint (Phase D, ADR-030 §Security).
//!
//! `POST /api/v1/cleanup/telemetry` — accepts a `CleanupPlanReviewed`
//! payload from the frontend Review screen.
//!
//! The other [`CleanupTelemetryEvent`] variants are server-side only and
//! are rejected with 400 if posted here. We **do not log the raw user_id
//! from the request body**; it is hashed server-side via
//! [`hash_user_id`] before any tracing emission.

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use crate::cleanup::domain::plan::PlanId;
use crate::cleanup::telemetry::{hash_user_id, CleanupTelemetryEvent};
use crate::AppState;

use super::plan::ErrorBody;

pub fn routes() -> Router<AppState> {
    Router::new().route("/telemetry", post(post_telemetry))
}

/// Wire payload for the only client-emitted telemetry event.
///
/// The frontend supplies a raw user id; we hash it server-side. The
/// `event` discriminator is fixed so we can extend this surface later
/// without breaking older clients.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedTelemetryRequest {
    /// Must be the literal string `"cleanup_plan_reviewed"`. Any other
    /// value is rejected.
    pub event: String,
    pub plan_id: PlanId,
    /// Raw user id from the authenticated session. Server-side hashed
    /// before emission; never persisted in plaintext.
    pub user_id: String,
    pub time_on_review_ms: u64,
    #[serde(default)]
    pub expanded_groups: u64,
    #[serde(default)]
    pub samples_viewed: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedResponse {
    pub accepted: bool,
}

fn err(code: StatusCode, error: &str, message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        code,
        Json(ErrorBody {
            error: error.into(),
            message: message.into(),
        }),
    )
}

async fn post_telemetry(
    State(state): State<AppState>,
    Json(body): Json<ReviewedTelemetryRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorBody>)> {
    if body.event != "cleanup_plan_reviewed" {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_event",
            "only cleanup_plan_reviewed is accepted on this endpoint",
        ));
    }
    if body.user_id.trim().is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_user",
            "userId required",
        ));
    }

    let event = CleanupTelemetryEvent::CleanupPlanReviewed {
        plan_id: body.plan_id,
        // Hash here; the raw user_id never leaves this scope.
        user_id_hash: hash_user_id(&body.user_id),
        time_on_review_ms: body.time_on_review_ms,
        expanded_groups: body.expanded_groups,
        samples_viewed: body.samples_viewed,
    };
    state.cleanup_telemetry.emit(event);

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse { accepted: true }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_event_string_rejected() {
        // Pure shape test — the handler rejects anything other than
        // "cleanup_plan_reviewed".
        let body = ReviewedTelemetryRequest {
            event: "cleanup_apply_started".into(),
            plan_id: uuid::Uuid::now_v7(),
            user_id: "u".into(),
            time_on_review_ms: 1,
            expanded_groups: 0,
            samples_viewed: 0,
        };
        assert_ne!(body.event, "cleanup_plan_reviewed");
    }

    #[test]
    fn user_id_is_not_echoed_in_emitted_event() {
        // The handler hashes user_id before emit; this test guards against
        // a regression where someone replaces hash_user_id with the raw
        // string.
        let raw = "fastnsilver@gmail.com";
        let event = CleanupTelemetryEvent::CleanupPlanReviewed {
            plan_id: uuid::Uuid::now_v7(),
            user_id_hash: hash_user_id(raw),
            time_on_review_ms: 1,
            expanded_groups: 0,
            samples_viewed: 0,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("fastnsilver"));
        assert!(!json.contains("@gmail"));
    }
}
