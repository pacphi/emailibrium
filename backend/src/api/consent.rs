//! Consent management API endpoints (ADR-012 + R-09 GDPR Consent Persistence).
//!
//! AI consent (existing):
//! - GET    /api/v1/consent              -- get all AI consent records
//! - POST   /api/v1/consent              -- grant AI consent for a provider
//! - DELETE /api/v1/consent/:provider     -- revoke AI consent for a provider
//! - GET    /api/v1/consent/audit         -- paginated AI audit log
//!
//! GDPR consent (R-09):
//! - POST   /api/v1/consent/gdpr         -- record GDPR consent decision
//! - GET    /api/v1/consent/gdpr         -- list all GDPR consent decisions
//! - GET    /api/v1/consent/gdpr/:type   -- get specific GDPR consent
//! - POST   /api/v1/consent/export       -- export all user data (GDPR Art 20)
//! - GET    /api/v1/consent/privacy-audit -- GDPR privacy audit log

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::vectors::consent::{AuditPage, ConsentRecord};
use crate::vectors::generative_router::GenerativeRouterService;
use crate::vectors::model_registry::ProviderType;
use crate::vectors::privacy::{ConsentDecision, ErasureReport, PrivacyAuditPage, UserDataExport};
use crate::AppState;

/// Build consent API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        // Existing AI consent routes.
        .route("/", get(get_consent).post(grant_consent))
        .route("/audit", get(get_audit_log))
        .route("/{provider}", delete(revoke_consent))
        // GDPR consent routes (R-09).
        .route("/gdpr", get(list_gdpr_consents).post(record_gdpr_consent))
        .route("/gdpr/{consent_type}", get(get_gdpr_consent))
        .route("/export", post(export_user_data))
        .route("/privacy-audit", get(get_privacy_audit_log))
        .route("/erase", post(erase_user_data))
}

// --- Request / Response types (existing) ---

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

// --- Request / Response types (GDPR R-09) ---

/// Request body for recording a GDPR consent decision.
#[derive(Debug, Deserialize)]
pub struct GdprConsentRequest {
    /// Consent type: 'cloud_ai', 'data_export', 'analytics', 'third_party'.
    pub consent_type: String,
    /// Whether consent is being granted (true) or revoked (false).
    pub granted: bool,
}

/// Response for a GDPR consent decision.
#[derive(Debug, Serialize)]
pub struct GdprConsentResponse {
    pub decision: ConsentDecision,
}

/// Response listing all GDPR consent decisions.
#[derive(Debug, Serialize)]
pub struct GdprConsentListResponse {
    pub decisions: Vec<ConsentDecision>,
}

/// Response for data export.
#[derive(Debug, Serialize)]
pub struct ExportResponse {
    pub export: UserDataExport,
}

/// Response for data erasure.
#[derive(Debug, Serialize)]
pub struct ErasureResponse {
    pub report: ErasureReport,
}

/// Request body for data erasure (requires confirmation).
#[derive(Debug, Deserialize)]
pub struct ErasureRequest {
    /// Must be "CONFIRM_ERASE_ALL_DATA" to proceed.
    pub confirmation_token: String,
}

// --- Handlers (existing AI consent) ---

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
///
/// When consent is granted for a cloud AI provider, the corresponding generative
/// provider is re-enabled in the router so it participates in failover again.
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

    // Re-enable the provider in the generative router if it maps to a known type.
    if let Ok(provider_type) = req.provider.parse::<ProviderType>() {
        state
            .vector_service
            .generative_router
            .enable_provider(provider_type)
            .await;
    }

    Ok(Json(GrantConsentResponse {
        provider: req.provider,
        status: "granted".to_string(),
    }))
}

/// DELETE /api/v1/consent/:provider
///
/// When consent is revoked for a cloud AI provider, the corresponding generative
/// provider is disabled in the router so it is excluded from failover selection.
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

    // Disable the provider in the generative router if it maps to a known type.
    if let Ok(provider_type) = provider.parse::<ProviderType>() {
        state
            .vector_service
            .generative_router
            .disable_provider(provider_type)
            .await;
    }

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

// --- Handlers (GDPR R-09) ---

/// POST /api/v1/consent/gdpr
///
/// Record a GDPR consent decision (persisted to consent_decisions table).
async fn record_gdpr_consent(
    State(state): State<AppState>,
    Json(req): Json<GdprConsentRequest>,
) -> Result<Json<GdprConsentResponse>, (StatusCode, String)> {
    let valid_types = ["cloud_ai", "data_export", "analytics", "third_party"];
    if !valid_types.contains(&req.consent_type.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid consent_type '{}'. Must be one of: {}",
                req.consent_type,
                valid_types.join(", ")
            ),
        ));
    }

    let decision = state
        .vector_service
        .privacy_service
        .record_consent(&req.consent_type, req.granted, None, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // When cloud_ai consent changes, toggle cloud providers in the generative router.
    if req.consent_type == "cloud_ai" {
        let cloud_providers = [
            ProviderType::OpenAi,
            ProviderType::Anthropic,
            ProviderType::Gemini,
        ];
        for pt in &cloud_providers {
            if req.granted {
                state
                    .vector_service
                    .generative_router
                    .enable_provider(*pt)
                    .await;
            } else {
                state
                    .vector_service
                    .generative_router
                    .disable_provider(*pt)
                    .await;
            }
        }
    }

    Ok(Json(GdprConsentResponse { decision }))
}

/// GET /api/v1/consent/gdpr
///
/// List all current GDPR consent decisions (latest per type).
async fn list_gdpr_consents(
    State(state): State<AppState>,
) -> Result<Json<GdprConsentListResponse>, (StatusCode, String)> {
    let decisions = state
        .vector_service
        .privacy_service
        .list_consents()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(GdprConsentListResponse { decisions }))
}

/// GET /api/v1/consent/gdpr/:type
///
/// Get the current effective consent for a specific type.
async fn get_gdpr_consent(
    State(state): State<AppState>,
    Path(consent_type): Path<String>,
) -> Result<Json<GdprConsentResponse>, (StatusCode, String)> {
    let decision = state
        .vector_service
        .privacy_service
        .get_consent(&consent_type)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("No consent decision found for type '{consent_type}'"),
            )
        })?;

    Ok(Json(GdprConsentResponse { decision }))
}

/// POST /api/v1/consent/export
///
/// Export all user data as JSON (GDPR Article 20: Data Portability).
async fn export_user_data(
    State(state): State<AppState>,
) -> Result<Json<ExportResponse>, (StatusCode, String)> {
    let export = state
        .vector_service
        .privacy_service
        .export_user_data()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ExportResponse { export }))
}

/// GET /api/v1/consent/privacy-audit
///
/// View the GDPR privacy audit log (paginated).
async fn get_privacy_audit_log(
    State(state): State<AppState>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<PrivacyAuditPage>, (StatusCode, String)> {
    let page = state
        .vector_service
        .privacy_service
        .get_audit_log(params.page, params.per_page)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(page))
}

/// POST /api/v1/consent/erase
///
/// Erase all user data (GDPR Article 17: Right to Erasure). Requires confirmation.
async fn erase_user_data(
    State(state): State<AppState>,
    Json(body): Json<ErasureRequest>,
) -> Result<Json<ErasureResponse>, (StatusCode, String)> {
    if body.confirmation_token != "CONFIRM_ERASE_ALL_DATA" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid confirmation token. Send \
             {\"confirmation_token\": \"CONFIRM_ERASE_ALL_DATA\"}"
                .to_string(),
        ));
    }

    let report = state
        .vector_service
        .privacy_service
        .erase_user_data()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ErasureResponse { report }))
}
