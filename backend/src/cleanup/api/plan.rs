//! Plan API handlers (Phase A surface — ADR-030 §9).
//!
//! Routes (mounted under `/api/v1/cleanup`):
//!   POST   /plan
//!   GET    /plan/:id
//!   DELETE /plan/:id
//!   GET    /plan/:id/operations
//!   GET    /plan/:id/sample
//!   POST   /plan/:id/refresh
//!   GET    /plans
//!
//! Phase A wiring: PlanBuilder is constructed per-request from the DB pool
//! and stub adapters in `cleanup::repository::adapters`. Phase B/C will
//! replace those stubs with adapters that read real `emails` /
//! `topic_clusters` / subscription tables.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cleanup::domain::builder::PlanBuilder;
use crate::cleanup::domain::classifier::RiskClassifier;
use crate::cleanup::domain::operation::{PlanStatus, Provider};
use crate::cleanup::domain::plan::{CleanupPlan, PlanId, WizardSelections};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RuleEvaluator, SubscriptionRepository,
};
use crate::cleanup::repository::{
    CleanupPlanRepository, OpsFilter, SqlxAccountStateProvider, SqlxClusterRepository,
    SqlxEmailRepository, SqlxRuleEvaluator, SqlxSubscriptionRepository,
};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/plan", post(create_plan))
        .route("/plan/{id}", get(get_plan).delete(cancel_plan))
        .route("/plan/{id}/operations", get(list_operations))
        .route("/plan/{id}/sample", get(sample_operations))
        .route("/plan/{id}/refresh", post(refresh_account))
        .route("/plans", get(list_plans))
}

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserQuery {
    pub user_id: String,
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

fn build_plan_builder(state: &AppState) -> PlanBuilder {
    let pool = state.db.pool.clone();
    PlanBuilder {
        emails: Arc::new(SqlxEmailRepository { pool: pool.clone() }) as Arc<dyn EmailRepository>,
        subs: Arc::new(SqlxSubscriptionRepository { pool: pool.clone() })
            as Arc<dyn SubscriptionRepository>,
        clusters: Arc::new(SqlxClusterRepository { pool: pool.clone() })
            as Arc<dyn ClusterRepository>,
        rules: Arc::new(SqlxRuleEvaluator { pool: pool.clone() }) as Arc<dyn RuleEvaluator>,
        accounts: Arc::new(SqlxAccountStateProvider { pool }) as Arc<dyn AccountStateProvider>,
        classifier: Arc::new(RiskClassifier::new()),
        // Phase A: default to Gmail; Phase C will read provider per account.
        provider_for: Arc::new(|_| Provider::Gmail),
        plan_ttl_minutes: 30,
    }
}

// ---------------------------------------------------------------------------
// POST /plan
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlanQuery {
    pub user_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlanResponse {
    pub plan_id: PlanId,
    pub valid_until: chrono::DateTime<chrono::Utc>,
    pub totals: crate::cleanup::domain::plan::PlanTotals,
    pub risk: crate::cleanup::domain::plan::RiskRollup,
    pub status: PlanStatus,
    pub warnings_count: u64,
}

async fn create_plan(
    State(state): State<AppState>,
    Query(q): Query<CreatePlanQuery>,
    Json(selections): Json<WizardSelections>,
) -> Result<(StatusCode, Json<CreatePlanResponse>), (StatusCode, Json<ErrorBody>)> {
    if q.user_id.trim().is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_user",
            "userId required",
        ));
    }
    let builder = build_plan_builder(&state);
    let plan: CleanupPlan = builder.build(&q.user_id, selections).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "build_failed",
            &e.to_string(),
        )
    })?;
    state.cleanup_plan_repo.save(&plan).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save_failed",
            &e.to_string(),
        )
    })?;
    Ok((
        StatusCode::CREATED,
        Json(CreatePlanResponse {
            plan_id: plan.id,
            valid_until: plan.valid_until,
            totals: plan.totals,
            risk: plan.risk,
            status: plan.status,
            warnings_count: plan.warnings.len() as u64,
        }),
    ))
}

// ---------------------------------------------------------------------------
// GET /plan/:id
// ---------------------------------------------------------------------------

async fn get_plan(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<UserQuery>,
) -> Result<Json<CleanupPlan>, (StatusCode, Json<ErrorBody>)> {
    let plan = state
        .cleanup_plan_repo
        .load(&q.user_id, id)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "load_failed",
                &e.to_string(),
            )
        })?;
    plan.map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "not_found", "plan not found"))
}

// ---------------------------------------------------------------------------
// DELETE /plan/:id
// ---------------------------------------------------------------------------

async fn cancel_plan(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(_q): Query<UserQuery>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    state.cleanup_plan_repo.cancel(id).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cancel_failed",
            &e.to_string(),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /plan/:id/operations
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOpsQuery {
    pub user_id: String,
    pub cursor: Option<u64>,
    pub limit: Option<u32>,
    pub risk: Option<String>,
    pub action: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOpsResponse {
    pub items: Vec<crate::cleanup::domain::operation::PlannedOperation>,
    pub next_cursor: Option<u64>,
}

async fn list_operations(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListOpsQuery>,
) -> Result<Json<ListOpsResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = q.limit.unwrap_or(100).min(1000);
    let filter = OpsFilter {
        risk: q.risk,
        action: q.action,
        account_id: q.account_id,
    };
    let (items, next_cursor) = state
        .cleanup_plan_repo
        .list_operations(id, filter, q.cursor, limit)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_failed",
                &e.to_string(),
            )
        })?;
    Ok(Json(ListOpsResponse { items, next_cursor }))
}

// ---------------------------------------------------------------------------
// GET /plan/:id/sample
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleQuery {
    pub user_id: String,
    pub source: String,
    pub n: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleResponse {
    pub email_ids: Vec<String>,
}

async fn sample_operations(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<SampleQuery>,
) -> Result<Json<SampleResponse>, (StatusCode, Json<ErrorBody>)> {
    let n = q.n.unwrap_or(5).min(20);
    let email_ids = state
        .cleanup_plan_repo
        .sample_operations(id, &q.source, n)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "sample_failed",
                &e.to_string(),
            )
        })?;
    Ok(Json(SampleResponse { email_ids }))
}

// ---------------------------------------------------------------------------
// POST /plan/:id/refresh
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshQuery {
    pub user_id: String,
    pub account_id: String,
}

async fn refresh_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<RefreshQuery>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    // Phase C race guard: reject if a job is currently running for this
    // plan. Otherwise refresh would delete rows whose `applied_at` was
    // already written, corrupting the audit trail (DDD-008 addendum).
    if state.apply_orchestrator.is_running_for_plan(id).await {
        return Err(err(
            StatusCode::CONFLICT,
            "apply_in_progress",
            "cannot refresh while an apply job is running for this plan",
        ));
    }
    state
        .cleanup_plan_repo
        .replace_account_rows(id, &q.account_id, Vec::new())
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "refresh_failed",
                &e.to_string(),
            )
        })?;
    Ok(StatusCode::ACCEPTED)
}

// ---------------------------------------------------------------------------
// GET /plans
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPlansQuery {
    pub user_id: String,
    pub status: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPlansResponse {
    pub items: Vec<crate::cleanup::domain::plan::CleanupPlanSummary>,
}

async fn list_plans(
    State(state): State<AppState>,
    Query(q): Query<ListPlansQuery>,
) -> Result<Json<ListPlansResponse>, (StatusCode, Json<ErrorBody>)> {
    let status = q.status.as_deref().and_then(PlanStatus::from_str_opt);
    let limit = q.limit.unwrap_or(20).min(100);
    let items = state
        .cleanup_plan_repo
        .list_by_user(&q.user_id, status, limit)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_failed",
                &e.to_string(),
            )
        })?;
    Ok(Json(ListPlansResponse { items }))
}
