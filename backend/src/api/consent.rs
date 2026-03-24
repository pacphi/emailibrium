//! Consent management API endpoints (ADR-012).
//!
//! - GET    /api/v1/consent              -- get all consent records
//! - POST   /api/v1/consent              -- grant consent for a provider
//! - DELETE /api/v1/consent/:provider     -- revoke consent for a provider
//! - GET    /api/v1/consent/audit         -- paginated audit log

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::vectors::consent::{AuditPage, ConsentRecord};
use crate::AppState;

/// Build consent API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_consent).post(grant_consent))
        .route("/audit", get(get_audit_log))
        .route("/{provider}", delete(revoke_consent))
}

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
pub struct GrantConsentRequest {
    pub provider: String,
    pub acknowledgment: String,
}

#[derive(Debug, Serialize)]
pub struct ConsentResponse {
    pub records: Vec<ConsentRecord>,
}

#[derive(Debug, Serialize)]
pub struct GrantConsentResponse {
    pub provider: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct RevokeConsentResponse {
    pub provider: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct AuditQueryParams {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

// --- Handlers ---

/// GET /api/v1/consent
async fn get_consent(
    State(state): State<AppState>,
) -> Result<Json<ConsentResponse>, (StatusCode, String)> {
    let records = state
        .vector_service
        .consent_manager
        .get_all_consent()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ConsentResponse { records }))
}

/// POST /api/v1/consent
async fn grant_consent(
    State(state): State<AppState>,
    Json(req): Json<GrantConsentRequest>,
) -> Result<Json<GrantConsentResponse>, (StatusCode, String)> {
    state
        .vector_service
        .consent_manager
        .grant_consent(&req.provider, &req.acknowledgment)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(GrantConsentResponse {
        provider: req.provider,
        status: "granted".to_string(),
    }))
}

/// DELETE /api/v1/consent/:provider
async fn revoke_consent(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<RevokeConsentResponse>, (StatusCode, String)> {
    state
        .vector_service
        .consent_manager
        .revoke_consent(&provider)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(RevokeConsentResponse {
        provider,
        status: "revoked".to_string(),
    }))
}

/// GET /api/v1/consent/audit
async fn get_audit_log(
    State(state): State<AppState>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditPage>, (StatusCode, String)> {
    let page = state
        .vector_service
        .consent_manager
        .get_audit_log(params.page, params.per_page)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(page))
}
