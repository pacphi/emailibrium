//! Apply API handlers (Phase C, ADR-030 §9).
//!
//! Routes (mounted under `/api/v1/cleanup`):
//!   POST /apply/:planId?riskMax=…   → 202 + jobId
//!   GET  /apply/:jobId/stream       → SSE stream
//!   POST /apply/:jobId/cancel       → 204
//!   GET  /apply/:jobId              → JSON CleanupApplyJob

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::cleanup::domain::operation::RiskMax;
use crate::cleanup::domain::plan::{JobId, PlanId};
use crate::cleanup::orchestrator::apply::ApplyOptions;
use crate::cleanup::repository::CleanupPlanRepository;
use crate::AppState;

use super::plan::ErrorBody;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/apply/{plan_id}", post(begin_apply))
        .route("/apply/{job_id}/stream", get(apply_stream))
        .route("/apply/{job_id}/cancel", post(cancel_apply))
        .route("/apply/{job_id}", get(get_apply_job))
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

// ---------------------------------------------------------------------------
// POST /apply/:planId
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginApplyQuery {
    pub user_id: String,
    pub risk_max: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginApplyBody {
    #[serde(default)]
    pub acknowledged_high_risk_seqs: Vec<u64>,
    #[serde(default)]
    pub acknowledged_medium_groups: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginApplyResponse {
    pub job_id: JobId,
}

async fn begin_apply(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
    Query(q): Query<BeginApplyQuery>,
    body: Option<Json<BeginApplyBody>>,
) -> Result<(StatusCode, Json<BeginApplyResponse>), (StatusCode, Json<ErrorBody>)> {
    let body = body.map(|j| j.0).unwrap_or_default();
    let risk_max = match q.risk_max.as_deref() {
        Some("low") | None => RiskMax::Low,
        Some("medium") => RiskMax::Medium,
        Some("high") => RiskMax::High,
        Some(other) => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "invalid_risk_max",
                &format!("unknown riskMax: {other}"),
            ))
        }
    };

    let plan = state
        .cleanup_plan_repo
        .load(&q.user_id, plan_id as PlanId)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "load_failed",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "not_found", "plan not found"))?;

    let opts = ApplyOptions {
        risk_max,
        acknowledged_high_risk_seqs: body.acknowledged_high_risk_seqs,
        acknowledged_medium_groups: body.acknowledged_medium_groups,
    };

    let orch = state.apply_orchestrator.clone();
    let job_id = orch.begin_apply(&plan, opts).await.map_err(|e| {
        use crate::cleanup::orchestrator::apply::BeginApplyError as E;
        match e {
            E::BadStatus(_) | E::Expired => {
                err(StatusCode::CONFLICT, "plan_not_applyable", &e.to_string())
            }
            E::HardDrift(_) => err(StatusCode::CONFLICT, "hard_drift", &e.to_string()),
            E::Repo(_) | E::Drift(_) => err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "begin_apply_failed",
                &e.to_string(),
            ),
        }
    })?;

    Ok((StatusCode::ACCEPTED, Json(BeginApplyResponse { job_id })))
}

// ---------------------------------------------------------------------------
// GET /apply/:jobId/stream — SSE
// ---------------------------------------------------------------------------

async fn apply_stream(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorBody>)> {
    let orch = state.apply_orchestrator.clone();

    let snapshot = orch.build_snapshot(job_id).await;
    let rx = orch.subscribe(job_id).await;

    let live = rx.map(BroadcastStream::new);

    let snapshot_stream = futures::stream::iter(snapshot.into_iter().map(|ev| {
        let event = Event::default()
            .json_data(&ev)
            .unwrap_or_else(|_| Event::default().data(""));
        Ok::<_, Infallible>(event)
    }));

    let live_stream = futures::stream::iter(live.into_iter().flat_map(|s| {
        // We can't easily flatten a BroadcastStream here without async,
        // so fall through to the manual mapping below.
        std::iter::once(s)
    }))
    .flat_map(|s| {
        s.filter_map(|msg| async move {
            match msg {
                Ok(ev) => {
                    let event = Event::default()
                        .json_data(&ev)
                        .unwrap_or_else(|_| Event::default().data(""));
                    Some(Ok::<_, Infallible>(event))
                }
                Err(_) => None,
            }
        })
    });

    let stream = snapshot_stream.chain(live_stream);

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

// ---------------------------------------------------------------------------
// POST /apply/:jobId/cancel
// ---------------------------------------------------------------------------

async fn cancel_apply(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    state
        .apply_orchestrator
        .cancel(job_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "not_found", "job not found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /apply/:jobId
// ---------------------------------------------------------------------------

async fn get_apply_job(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<crate::cleanup::domain::plan::CleanupApplyJob>, (StatusCode, Json<ErrorBody>)> {
    state
        .apply_orchestrator
        .job_repo
        .load(job_id)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "load_failed",
                &e.to_string(),
            )
        })?
        .map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "not_found", "job not found"))
}
