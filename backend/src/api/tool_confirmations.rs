//! `POST /api/v1/tools/confirmations/{id}/approve` — the separate approval
//! call a `requires_confirmation` tool's denied caller cannot itself make.
//!
//! This is the other half of `tools::confirmation::ConfirmationStore`: the
//! MCP/chat dispatch path (`ToolRegistry::invoke`) issues the confirmation
//! id, and this endpoint is the only place that ever approves one. Reaching
//! this endpoint still only requires this engine's one shared credential
//! (bearer or session cookie, per `middleware::local_auth`) — see
//! `tools::confirmation`'s module doc for why that is a real but incomplete
//! fix, not a claim of true human/agent separation.

use axum::extract::{Path, State};
use axum::response::Json;
use axum::routing::post;
use axum::Router;
use serde::Serialize;

use crate::tools::confirmation::ApproveError;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/confirmations/{id}/approve", post(approve))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApproveResponse {
    confirmation_id: String,
    status: &'static str,
}

async fn approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApproveResponse>, (axum::http::StatusCode, String)> {
    match state.tools.confirmations().approve(&id) {
        Ok(()) => Ok(Json(ApproveResponse {
            confirmation_id: id,
            status: "approved",
        })),
        Err(ApproveError::NotFound) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("No pending confirmation with id: {id} (already used or never existed)"),
        )),
        Err(ApproveError::Expired) => Err((
            axum::http::StatusCode::GONE,
            format!("Confirmation {id} expired or does not exist — the caller must request a fresh one"),
        )),
    }
}
