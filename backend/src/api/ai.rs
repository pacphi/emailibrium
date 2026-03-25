//! AI model management and chat API endpoints (ADR-013, R-07).
//!
//! - GET    /api/v1/ai/models                — list all known models with download/active status
//! - GET    /api/v1/ai/status                — current AI subsystem status
//! - GET    /api/v1/ai/reindex-status        — progress of any in-flight re-index
//! - POST   /api/v1/ai/chat                  — send a chat message, receive SSE stream
//! - GET    /api/v1/ai/chat/sessions         — list active chat sessions
//! - DELETE /api/v1/ai/chat/sessions/:id     — delete a chat session

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::vectors::chat::{ChatResponse, SessionSummary};
use crate::vectors::generative_router::{GenerativeRouterService, ProviderStatus};
use crate::vectors::model_registry::ProviderType;
use crate::vectors::models::{self, ModelStatus};
use crate::vectors::reindex::ReindexStatus;
use crate::AppState;

/// Build AI management routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/status", get(ai_status))
        .route("/reindex-status", get(reindex_status))
        .route("/providers", get(list_providers))
        .route("/providers/{provider}/disable", post(disable_provider))
        .route("/providers/{provider}/enable", post(enable_provider))
        .route("/chat", post(chat_message))
        .route("/chat/stream", post(chat_message_sse))
        .route("/chat/sessions", get(list_chat_sessions))
        .route("/chat/sessions/{id}", delete(delete_chat_session))
}

// ---------------------------------------------------------------------------
// Handlers — AI management (existing)
// ---------------------------------------------------------------------------

/// List all known models with their current status.
async fn list_models(State(state): State<AppState>) -> Json<Vec<ModelStatus>> {
    let vs = &state.vector_service;
    let active_model = &vs.config.embedding.model;
    let cache_dir = &vs.config.store.path;
    let statuses = models::get_model_statuses(active_model, cache_dir);
    Json(statuses)
}

/// Overall AI subsystem status response.
#[derive(Debug, Serialize)]
struct AiStatusResponse {
    /// Currently active embedding model name.
    active_model: String,
    /// Embedding dimensions for the active model.
    dimensions: usize,
    /// Embedding provider backend in use.
    provider: String,
    /// Whether the embedding pipeline is available.
    embedding_available: bool,
    /// Whether a re-index is currently in progress.
    reindex_in_progress: bool,
}

/// Get overall AI subsystem status.
async fn ai_status(State(state): State<AppState>) -> Json<AiStatusResponse> {
    let vs = &state.vector_service;
    let embedding_available = vs.embedding.is_available().await;
    let reindex_status = vs.reindex_orchestrator.get_status().await;

    Json(AiStatusResponse {
        active_model: vs.config.embedding.model.clone(),
        dimensions: vs.config.embedding.dimensions,
        provider: vs.config.embedding.provider.clone(),
        embedding_available,
        reindex_in_progress: reindex_status.in_progress,
    })
}

/// Get the current re-indexing progress.
async fn reindex_status(State(state): State<AppState>) -> Json<ReindexStatus> {
    let status = state.vector_service.reindex_orchestrator.get_status().await;
    Json(status)
}

// ---------------------------------------------------------------------------
// Handlers — Provider management (DDD-006)
// ---------------------------------------------------------------------------

/// Response for the provider list endpoint.
#[derive(Debug, Serialize)]
struct ProviderListResponse {
    providers: Vec<ProviderStatus>,
}

/// Response for enable / disable actions.
#[derive(Debug, Serialize)]
struct ProviderActionResponse {
    provider: String,
    enabled: bool,
}

/// GET /api/v1/ai/providers — list registered providers with status.
async fn list_providers(State(state): State<AppState>) -> Json<ProviderListResponse> {
    let providers = state
        .vector_service
        .generative_router
        .list_providers()
        .await;
    Json(ProviderListResponse { providers })
}

/// POST /api/v1/ai/providers/:provider/disable — disable a provider at runtime.
async fn disable_provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<ProviderActionResponse>, (StatusCode, String)> {
    let provider_type: ProviderType = provider
        .parse()
        .map_err(|e: String| (StatusCode::BAD_REQUEST, e))?;

    state
        .vector_service
        .generative_router
        .disable_provider(provider_type)
        .await;

    debug!(provider = %provider_type, "Provider disabled via API");

    Ok(Json(ProviderActionResponse {
        provider: provider_type.to_string(),
        enabled: false,
    }))
}

/// POST /api/v1/ai/providers/:provider/enable — re-enable a provider at runtime.
async fn enable_provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<ProviderActionResponse>, (StatusCode, String)> {
    let provider_type: ProviderType = provider
        .parse()
        .map_err(|e: String| (StatusCode::BAD_REQUEST, e))?;

    state
        .vector_service
        .generative_router
        .enable_provider(provider_type)
        .await;

    debug!(provider = %provider_type, "Provider enabled via API");

    Ok(Json(ProviderActionResponse {
        provider: provider_type.to_string(),
        enabled: true,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — Chat (R-07)
// ---------------------------------------------------------------------------

/// Request body for POST /api/v1/ai/chat.
#[derive(Debug, Deserialize)]
struct ChatRequest {
    /// Session ID (created if not found).
    session_id: String,
    /// The user's message.
    message: String,
    /// Optional email IDs to include as conversation context.
    #[serde(default)]
    email_context: Option<Vec<String>>,
}

/// POST /api/v1/ai/chat — send a message and get a JSON response.
///
/// For streaming, use POST /api/v1/ai/chat/stream instead.
async fn chat_message(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let chat_service = match &state.chat_service {
        Some(svc) => svc,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Chat service is not available (no generative model configured)".to_string(),
            ));
        }
    };

    debug!(
        session_id = %req.session_id,
        message_len = req.message.len(),
        "Chat request"
    );

    // Start an inference session to track this chat request.
    let session_mgr = &state.vector_service.inference_session_manager;
    let model_name = state
        .vector_service
        .generative
        .as_ref()
        .map(|g| g.model_name().to_string())
        .unwrap_or_else(|| "generative-router".to_string());
    let provider = state
        .vector_service
        .generative_router
        .active_provider()
        .await
        .unwrap_or(ProviderType::None);
    let inf_session_id = session_mgr.start_session(&model_name, provider).await;

    let audit_timer =
        crate::vectors::audit::AuditTimer::start(&format!("{provider}"), &model_name, "chat", None);

    let result = chat_service
        .chat(&req.session_id, &req.message, req.email_context)
        .await;

    match &result {
        Ok(response) => {
            let approx_tokens = response.reply.len() as u32 / 4;
            session_mgr
                .complete_session(&inf_session_id, req.message.len() as u32 / 4, approx_tokens)
                .await;
            let entry = audit_timer.finish_ok(
                Some(req.message.len() as i64 / 4),
                Some(approx_tokens as i64),
            );
            if let Err(e) = state.vector_service.audit_logger.log(&entry).await {
                tracing::warn!("Failed to write audit log: {e}");
            }
        }
        Err(e) => {
            session_mgr
                .fail_session(&inf_session_id, e.to_string())
                .await;
            let entry = audit_timer.finish_error(&e.to_string());
            if let Err(e) = state.vector_service.audit_logger.log(&entry).await {
                tracing::warn!("Failed to write audit log: {e}");
            }
        }
    }

    let response = result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(response))
}

/// POST /api/v1/ai/chat/stream — send a message and get an SSE stream.
///
/// The response is streamed as Server-Sent Events. Each event has type "chunk"
/// with a JSON data payload. The final event has type "done".
///
/// This follows the same SSE pattern as the ingestion status endpoint
/// (`api/ingestion.rs`). We generate the full response first, then stream it
/// in chunks. True token-level streaming would require changes to the
/// GenerativeModel trait.
async fn chat_message_sse(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let chat_service = match &state.chat_service {
        Some(svc) => svc.clone(),
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Chat service is not available (no generative model configured)".to_string(),
            ));
        }
    };

    debug!(
        session_id = %req.session_id,
        message_len = req.message.len(),
        "Chat SSE stream request"
    );

    // Generate the complete response up-front, then split into SSE events.
    let result = chat_service
        .chat(&req.session_id, &req.message, req.email_context)
        .await;

    let events: Vec<Result<Event, Infallible>> = match result {
        Ok(response) => {
            let chunk_size = 80;
            let reply = &response.reply;
            let raw_chunks: Vec<String> = if reply.is_empty() {
                vec![String::new()]
            } else {
                reply
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .collect()
            };
            let total = raw_chunks.len();

            let mut evts: Vec<Result<Event, Infallible>> = raw_chunks
                .into_iter()
                .enumerate()
                .filter_map(|(i, chunk)| {
                    let payload = serde_json::json!({
                        "session_id": response.session_id,
                        "chunk": chunk,
                        "chunk_index": i,
                        "total_chunks": total,
                    });
                    serde_json::to_string(&payload)
                        .ok()
                        .map(|json| Ok(Event::default().event("chunk").data(json)))
                })
                .collect();

            // Final "done" event with the complete response.
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "session_id": response.session_id,
                "reply": response.reply,
                "message_count": response.message_count,
            })) {
                evts.push(Ok(Event::default().event("done").data(json)));
            }

            evts
        }
        Err(e) => {
            let mut evts = Vec::new();
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "error": e.to_string(),
            })) {
                evts.push(Ok(Event::default().event("error").data(json)));
            }
            evts
        }
    };

    let stream = futures::stream::iter(events);

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

/// GET /api/v1/ai/chat/sessions — list all active chat sessions.
async fn list_chat_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, (StatusCode, String)> {
    let chat_service = match &state.chat_service {
        Some(svc) => svc,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Chat service is not available".to_string(),
            ));
        }
    };

    let sessions = chat_service.list_sessions().await;
    Ok(Json(sessions))
}

/// Response for session deletion.
#[derive(Debug, Serialize)]
struct DeleteSessionResponse {
    deleted: bool,
    session_id: String,
}

/// DELETE /api/v1/ai/chat/sessions/:id — delete a chat session.
async fn delete_chat_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<DeleteSessionResponse>, (StatusCode, String)> {
    let chat_service = match &state.chat_service {
        Some(svc) => svc,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Chat service is not available".to_string(),
            ));
        }
    };

    let deleted = chat_service.delete_session(&session_id).await;

    Ok(Json(DeleteSessionResponse {
        deleted,
        session_id,
    }))
}
