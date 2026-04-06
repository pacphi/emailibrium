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

use uuid::Uuid;

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

use crate::vectors::chat::{ChatMessage, ChatResponse, ChatRole, SessionSummary};
use crate::vectors::chat_orchestrator::{
    ChatOrchestrator, OrchestrationResult, OrchestratorConfig,
};
use crate::vectors::generative_router::{GenerativeRouterService, ProviderStatus};
use crate::vectors::model_registry::ProviderType;
use crate::vectors::models::{self, ModelStatus};
use crate::vectors::reindex::ReindexStatus;
use crate::vectors::tool_calling::{ToolDefinition, ToolMessage, ToolMessageRole};
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
        .route("/chat/confirm", post(confirm_tool_call))
        .route("/chat/sessions", get(list_chat_sessions))
        .route("/chat/sessions/{id}", delete(delete_chat_session))
        .route("/model-catalog", get(model_catalog))
        .route("/embedding-catalog", get(embedding_catalog))
        .route("/system-info", get(system_info))
        .route("/switch-model", post(switch_model))
        .route("/model-status/{model_id}", get(model_status))
        .route("/reembed", post(trigger_reembed))
        .route("/config/prompts", get(config_prompts))
        .route("/config/classification", get(config_classification))
        .route("/config/tuning", get(config_tuning))
        .route("/config/app", get(config_app))
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
#[serde(rename_all = "camelCase")]
struct ChatRequest {
    /// Session ID (created if not found; auto-generated when absent).
    #[serde(default)]
    session_id: Option<String>,
    /// The user's message.
    message: String,
    /// Optional conversation history from the frontend (accepted but ignored
    /// server-side; the backend maintains its own session history).
    #[serde(default)]
    #[allow(dead_code)]
    history: Option<serde_json::Value>,
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

    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    debug!(
        session_id = %session_id,
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

    // RAG: derive context budget from model's context window.
    // Reserve tokens for: system prompt + history + response (overhead from YAML tuning config).
    let ctx_window = state.vector_service.config.generative.builtin.context_size as usize;
    let overhead = state.yaml_config.tuning.rag.overhead_tokens;
    let rag_budget = ctx_window.saturating_sub(overhead);

    debug!(rag_budget, ctx_window, "RAG context budget calculated");
    let email_context = if let Some(ref rag) = state.rag_pipeline {
        match rag.retrieve_context(&req.message, Some(rag_budget)).await {
            Ok(ctx) if ctx.result_count > 0 => {
                debug!(
                    result_count = ctx.result_count,
                    "RAG: injecting email context"
                );
                Some(ctx.formatted_context)
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("RAG retrieval failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let result = chat_service
        .chat(&session_id, &req.message, email_context)
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
/// When a tool-calling provider is available (ADR-028), the orchestrator path
/// is used instead of the plain generate() path. The orchestrator can emit
/// additional SSE event types: `tool_call`, `tool_result`, and `confirmation`.
///
/// When no tool-calling provider exists, this falls back to the current RAG-only
/// path using the generative router.
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

    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    debug!(
        session_id = %session_id,
        message_len = req.message.len(),
        "Chat SSE stream request"
    );

    // ── Branch: orchestrator path (tool-calling provider available) ────
    if let Some(ref tool_provider) = state.tool_calling_provider {
        debug!(session_id = %session_id, "Using orchestrator path (tool-calling provider)");

        // Ensure the session exists and record the user message.
        let session = chat_service.get_or_create_session(&session_id).await;

        // Build system prompt with email context from RAG.
        let ctx_window = state.vector_service.config.generative.builtin.context_size as usize;
        let overhead = state.yaml_config.tuning.rag.overhead_tokens;
        let rag_budget = ctx_window.saturating_sub(overhead);

        let email_context = if let Some(ref rag) = state.rag_pipeline {
            match rag.retrieve_context(&req.message, Some(rag_budget)).await {
                Ok(ctx) if ctx.result_count > 0 => {
                    debug!(
                        result_count = ctx.result_count,
                        "RAG: injecting email context"
                    );
                    Some(ctx.formatted_context)
                }
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!("RAG retrieval failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        let system_prompt = {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M %Z");
            let base = &state.yaml_config.prompts.chat_assistant;
            let mut prompt = format!("The current date and time is: {now}\n\n{base}");
            if let Some(ref ctx) = email_context {
                prompt.push_str("\n\n[Email Context]\n");
                prompt.push_str(ctx);
            }
            prompt
        };

        // Convert session history + new user message into ToolMessage array.
        let mut messages = session_history_to_tool_messages(&session.messages, &system_prompt);
        messages.push(ToolMessage {
            role: ToolMessageRole::User,
            content: req.message.clone(),
            tool_calls: None,
            tool_call_id: None,
        });

        // Build orchestrator with tools and executor.
        let orchestrator = ChatOrchestrator::new(OrchestratorConfig::default())
            .with_tools(build_tool_definitions())
            .with_executor(build_tool_executor(&state));

        let max_tokens = state.yaml_config.tuning.llm.chat_max_tokens as u32;
        let result = orchestrator
            .orchestrate(messages, tool_provider.as_ref(), 0.7, max_tokens)
            .await;

        let pending_confirmations = state.pending_confirmations.clone();
        let sid = session_id.clone();

        let events: Vec<Result<Event, Infallible>> = match result {
            Ok(OrchestrationResult::Response(text)) => {
                // Record the turn in the chat session via the normal chat path.
                let _ = chat_service.chat(&sid, &req.message, email_context).await;

                let chunk_size = 80;
                let mut evts: Vec<Result<Event, Infallible>> = if text.is_empty() {
                    vec![]
                } else {
                    text.as_bytes()
                        .chunks(chunk_size)
                        .filter_map(|c| {
                            let chunk = String::from_utf8_lossy(c).into_owned();
                            let payload = serde_json::json!({ "type": "token", "content": chunk });
                            serde_json::to_string(&payload)
                                .ok()
                                .map(|json| Ok(Event::default().data(json)))
                        })
                        .collect()
                };

                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "done",
                    "sessionId": sid,
                })) {
                    evts.push(Ok(Event::default().data(json)));
                }
                evts
            }
            Ok(OrchestrationResult::ConfirmationRequired {
                confirmation_id,
                tool_name,
                tool_args,
                description,
            }) => {
                // Store pending confirmation for the confirm endpoint.
                {
                    let mut pending = pending_confirmations.lock().await;
                    pending.insert(
                        confirmation_id.clone(),
                        PendingConfirmation {
                            confirmation_id: confirmation_id.clone(),
                            session_id: sid.clone(),
                            tool_name: tool_name.clone(),
                            tool_args: tool_args.clone(),
                            description: description.clone(),
                        },
                    );
                }

                let mut evts = Vec::new();
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "confirmation",
                    "confirmationId": confirmation_id,
                    "toolName": tool_name,
                    "toolArgs": tool_args,
                    "description": description,
                    "sessionId": sid,
                })) {
                    evts.push(Ok(Event::default().event("confirmation").data(json)));
                }
                evts
            }
            Ok(OrchestrationResult::MaxIterationsReached(msg)) => {
                let mut evts = Vec::new();
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "token",
                    "content": msg,
                })) {
                    evts.push(Ok(Event::default().data(json)));
                }
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "done",
                    "sessionId": sid,
                })) {
                    evts.push(Ok(Event::default().data(json)));
                }
                evts
            }
            Err(e) => {
                let mut evts = Vec::new();
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "error": e.to_string(),
                })) {
                    evts.push(Ok(Event::default().data(json)));
                }
                evts
            }
        };

        let stream = futures::stream::iter(events);
        return Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        ));
    }

    // ── Fallback: RAG-only path (no tool-calling provider) ────────────
    // RAG: derive context budget from model's context window.
    let ctx_window = state.vector_service.config.generative.builtin.context_size as usize;
    let overhead = state.yaml_config.tuning.rag.overhead_tokens;
    let rag_budget = ctx_window.saturating_sub(overhead);

    let email_context = if let Some(ref rag) = state.rag_pipeline {
        match rag.retrieve_context(&req.message, Some(rag_budget)).await {
            Ok(ctx) if ctx.result_count > 0 => {
                debug!(
                    result_count = ctx.result_count,
                    "RAG: injecting email context"
                );
                Some(ctx.formatted_context)
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("RAG retrieval failed: {e}");
                None
            }
        }
    } else {
        None
    };

    // Generate the complete response up-front, then split into SSE events.
    let result = chat_service
        .chat(&session_id, &req.message, email_context)
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

            // Emit chunks in the format the frontend expects:
            // data: {"type":"token","content":"..."}
            let mut evts: Vec<Result<Event, Infallible>> = raw_chunks
                .into_iter()
                .filter_map(|chunk| {
                    let payload = serde_json::json!({
                        "type": "token",
                        "content": chunk,
                    });
                    serde_json::to_string(&payload)
                        .ok()
                        .map(|json| Ok(Event::default().data(json)))
                })
                .collect();

            // Final "done" event with the session ID.
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "type": "done",
                "sessionId": response.session_id,
            })) {
                evts.push(Ok(Event::default().data(json)));
            }

            evts
        }
        Err(e) => {
            let mut evts = Vec::new();
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "type": "error",
                "error": e.to_string(),
            })) {
                evts.push(Ok(Event::default().data(json)));
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

// ---------------------------------------------------------------------------
// Model catalog & system info
// ---------------------------------------------------------------------------

/// GET /api/v1/ai/model-catalog — list available models with hardware recommendations.
///
/// Prefers models from YAML config (`models-llm.yaml`); falls back to hardcoded catalog.
async fn model_catalog(
    State(state): State<AppState>,
) -> Json<Vec<crate::vectors::model_catalog::ModelInfo>> {
    let yaml = &state.yaml_config;
    let cache_dir = &state.vector_service.config.generative.builtin.cache_dir;

    // If YAML has builtin provider models, serve those.
    if let Some(builtin) = yaml.llm_catalog.providers.get("builtin") {
        if !builtin.models.is_empty() {
            let os_overhead = yaml.app.hardware.os_overhead_mb as u64;
            let sys = crate::vectors::model_catalog::get_system_info_with_overhead(os_overhead);
            let available = sys.available_for_model_mb;
            let hf_cache = dirs::home_dir()
                .map(|h| h.join(".cache/huggingface/hub"))
                .unwrap_or_default();

            // Apply memory_safety_margin from tuning.yaml when checking model fit.
            let safety_margin = yaml.tuning.llm.memory_safety_margin;

            let models: Vec<crate::vectors::model_catalog::ModelInfo> = builtin
                .models
                .iter()
                .map(|m| {
                    let ram_mb = m.min_ram_mb.unwrap_or(500);
                    let disk_mb = m.disk_mb.unwrap_or(0);
                    let required_with_margin = (ram_mb as f32 * safety_margin) as u64;
                    let fits = required_with_margin <= available;
                    let repo = m.repo_id.as_deref().unwrap_or("");
                    let file = m.filename.as_deref().unwrap_or("");
                    let cache_key = repo.replace('/', "--");
                    let cached = hf_cache.join(format!("models--{cache_key}")).exists()
                        || std::path::Path::new(cache_dir).join(file).exists();

                    crate::vectors::model_catalog::ModelInfo {
                        id: m.id.clone(),
                        name: m.name.clone(),
                        params: m.params.clone().unwrap_or_default(),
                        disk_mb,
                        ram_mb,
                        context_size: m.context_size,
                        quality: m.quality.clone().unwrap_or_else(|| "good".to_string()),
                        recommended: fits,
                        cached,
                        repo_id: repo.to_string(),
                        filename: file.to_string(),
                        family: m.family.clone(),
                        chat_template: m.chat_template.clone(),
                        rag_capable: m.rag_capable,
                        tool_calling: m.tool_calling,
                        default_for_ram_mb: m.default_for_ram_mb,
                        notes: m.notes.clone(),
                        cost_per_1m_input: m.cost_per_1m_input,
                        cost_per_1m_output: m.cost_per_1m_output,
                    }
                })
                .collect();

            return Json(models);
        }
    }

    // No YAML models found — return empty with a log warning.
    tracing::warn!(
        "No models found in config/models-llm.yaml under providers.builtin. \
         Add models to the YAML file or check the config/ directory path."
    );
    Json(Vec::new())
}

// ---------------------------------------------------------------------------
// Embedding catalog
// ---------------------------------------------------------------------------

/// A single embedding model entry returned by the API.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddingCatalogEntry {
    id: String,
    name: String,
    provider: String,
    dimensions: u32,
    max_tokens: u32,
    quality: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    disk_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_ram_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_per_1m_tokens: Option<f64>,
    download_required: bool,
    /// Provider-level description (e.g., "In-process ONNX embeddings via fastembed").
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_description: Option<String>,
    /// fastembed model variant name (for ONNX providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    fastembed_variant: Option<String>,
    /// fastembed quantization mode (e.g., "true", "false").
    #[serde(skip_serializing_if = "Option::is_none")]
    fastembed_quantized: Option<String>,
    /// Whether this model is the default for its provider.
    is_default: bool,
    /// Ollama model tag for pulling (for Ollama embedding providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    ollama_tag: Option<String>,
    /// Provider base URL (for non-ONNX providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    /// Environment variable name for the API key (for cloud providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_env: Option<String>,
}

/// GET /api/v1/ai/embedding-catalog — list available embedding models grouped by provider.
async fn embedding_catalog(State(state): State<AppState>) -> Json<Vec<EmbeddingCatalogEntry>> {
    let catalog = &state.yaml_config.embedding_catalog;

    let os_overhead = state.yaml_config.app.hardware.os_overhead_mb as u64;
    let sys = crate::vectors::model_catalog::get_system_info_with_overhead(os_overhead);
    let available_mb = sys.available_for_model_mb;

    let mut entries = Vec::new();
    for (provider_name, provider) in &catalog.providers {
        for model in &provider.models {
            let ram_mb = model.min_ram_mb.unwrap_or(0);
            // Filter out models that exceed available RAM (hardware filtering).
            if ram_mb > 0 && (ram_mb as u64) > available_mb {
                continue;
            }
            let provider_desc = if provider.description.is_empty() {
                None
            } else {
                Some(provider.description.clone())
            };
            entries.push(EmbeddingCatalogEntry {
                id: model.id.clone(),
                name: model.name.clone(),
                provider: provider_name.clone(),
                dimensions: model.dimensions,
                max_tokens: model.max_tokens,
                quality: model.quality.clone().unwrap_or_else(|| "good".to_string()),
                description: model.description.clone().unwrap_or_default(),
                disk_mb: model.disk_mb,
                min_ram_mb: model.min_ram_mb,
                cost_per_1m_tokens: model.cost_per_1m_tokens,
                download_required: provider.download_required,
                provider_description: provider_desc,
                fastembed_variant: model.fastembed_variant.clone(),
                fastembed_quantized: model.fastembed_quantized.clone(),
                is_default: model.is_default,
                ollama_tag: model.ollama_tag.clone(),
                base_url: provider.base_url.clone(),
                api_key_env: provider.api_key_env.clone(),
            });
        }
    }

    Json(entries)
}

/// GET /api/v1/ai/system-info — system hardware info for model selection.
async fn system_info(
    State(state): State<AppState>,
) -> Json<crate::vectors::model_catalog::SystemInfo> {
    let os_overhead = state.yaml_config.app.hardware.os_overhead_mb as u64;
    Json(crate::vectors::model_catalog::get_system_info_with_overhead(os_overhead))
}

// ---------------------------------------------------------------------------
// Model switching & re-embedding
// ---------------------------------------------------------------------------

/// Request body for POST /api/v1/ai/switch-model.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchModelRequest {
    /// New model ID (e.g., "qwen2.5-3b-q4km").
    #[allow(dead_code)] // Read when `builtin-llm` feature is enabled
    model_id: String,
}

/// Response for model switch.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SwitchModelResponse {
    model_id: String,
    /// "ready" | "downloading" | "error"
    status: String,
    message: String,
}

/// Response for model status check.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStatusResponse {
    model_id: String,
    /// "cached" | "downloading" | "not_cached"
    status: String,
    cached: bool,
}

/// Track which models are currently being downloaded.
static DOWNLOADING: std::sync::LazyLock<tokio::sync::RwLock<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| tokio::sync::RwLock::new(std::collections::HashSet::new()));

/// POST /api/v1/ai/switch-model — switch the active built-in LLM at runtime.
///
/// If the model is cached, activates it immediately (status: "ready").
/// If not cached, starts a background download (status: "downloading").
/// Poll GET /api/v1/ai/model-status/:model_id to track progress.
#[cfg(feature = "builtin-llm")]
async fn switch_model(
    State(state): State<AppState>,
    Json(req): Json<SwitchModelRequest>,
) -> Result<Json<SwitchModelResponse>, (StatusCode, String)> {
    use crate::vectors::generative::GenerationParams;
    use crate::vectors::generative_builtin::BuiltInGenerativeModel;
    use crate::vectors::model_registry::ProviderType;

    let mut config = state.vector_service.config.generative.builtin.clone();
    config.model_id = req.model_id.clone();

    // Look up context_size from the catalog (use pre-loaded config to avoid re-reading YAML).
    let catalog = crate::vectors::model_catalog::get_model_catalog_with_config(
        &config.cache_dir,
        &state.yaml_config,
    );
    if let Some(entry) = catalog.iter().find(|m| m.id == req.model_id) {
        config.context_size = entry.context_size;
    }

    // Check if already cached — try to initialize (fast if cached).
    let is_cached = catalog.iter().any(|m| m.id == req.model_id && m.cached);

    if is_cached {
        // Model is cached — activate immediately.
        tracing::info!(model_id = %req.model_id, "Switching to cached model");
        let model = BuiltInGenerativeModel::with_params_and_prompts(
            &config,
            GenerationParams::default(),
            state.yaml_config.prompts.clone(),
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Model init failed: {e}"),
            )
        })?;
        state
            .vector_service
            .generative_router
            .register(ProviderType::BuiltIn, std::sync::Arc::new(model), 1)
            .await;

        return Ok(Json(SwitchModelResponse {
            model_id: req.model_id,
            status: "ready".to_string(),
            message: "Model activated. Ready for chat.".to_string(),
        }));
    }

    // Not cached — start background download.
    let model_id = req.model_id.clone();
    {
        let mut downloading = DOWNLOADING.write().await;
        if downloading.contains(&model_id) {
            return Ok(Json(SwitchModelResponse {
                model_id,
                status: "downloading".to_string(),
                message: "Download already in progress.".to_string(),
            }));
        }
        downloading.insert(model_id.clone());
    }

    tracing::info!(model_id = %model_id, "Starting background model download");
    let router = state.vector_service.generative_router.clone();
    let prompts_for_spawn = state.yaml_config.prompts.clone();
    tokio::spawn(async move {
        match BuiltInGenerativeModel::with_params_and_prompts(
            &config,
            GenerationParams::default(),
            prompts_for_spawn,
        ) {
            Ok(model) => {
                router
                    .register(ProviderType::BuiltIn, std::sync::Arc::new(model), 1)
                    .await;
                tracing::info!(model_id = %model_id, "Model downloaded and activated");
            }
            Err(e) => {
                tracing::error!(model_id = %model_id, "Model download failed: {e}");
            }
        }
        DOWNLOADING.write().await.remove(&model_id);
    });

    Ok(Json(SwitchModelResponse {
        model_id: req.model_id,
        status: "downloading".to_string(),
        message: "Download started. Poll /ai/model-status/{id} for progress.".to_string(),
    }))
}

#[cfg(not(feature = "builtin-llm"))]
async fn switch_model(
    Json(_req): Json<SwitchModelRequest>,
) -> Result<Json<SwitchModelResponse>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Built-in LLM not enabled. Build with --features builtin-llm".to_string(),
    ))
}

/// GET /api/v1/ai/model-status/:model_id — check if a model is cached/downloading.
async fn model_status(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Json<ModelStatusResponse> {
    let cache_dir = &state.vector_service.config.generative.builtin.cache_dir;
    let catalog =
        crate::vectors::model_catalog::get_model_catalog_with_config(cache_dir, &state.yaml_config);
    let is_cached = catalog.iter().any(|m| m.id == model_id && m.cached);
    let is_downloading = DOWNLOADING.read().await.contains(&model_id);

    let status = if is_cached {
        "cached"
    } else if is_downloading {
        "downloading"
    } else {
        "not_cached"
    };

    Json(ModelStatusResponse {
        model_id,
        status: status.to_string(),
        cached: is_cached,
    })
}

/// Request body for selective re-embed.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReembedRequest {
    #[serde(default = "default_reembed_mode")]
    mode: String,
}

fn default_reembed_mode() -> String {
    "all".to_string()
}

/// Response for re-embed trigger.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReembedResponse {
    emails_reset: u64,
    mode: String,
    message: String,
    ingestion_triggered: bool,
}

/// POST /api/v1/ai/reembed — mark emails for re-embedding.
///
/// Accepts optional `{ "mode": "all" | "failed" | "stale" }`.
/// After resetting, auto-triggers ingestion for the first account.
async fn trigger_reembed(
    State(state): State<AppState>,
    body: Option<Json<ReembedRequest>>,
) -> Result<Json<ReembedResponse>, (StatusCode, String)> {
    let mode = body.map(|b| b.0.mode).unwrap_or_else(default_reembed_mode);

    // For "all" mode, clear the vector store and clusters to start fresh.
    if mode == "all" {
        match state.vector_service.store.clear_all().await {
            Ok(cleared) => {
                tracing::info!(cleared, "Cleared vector store for full re-embed");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to clear vector store (continuing with re-embed)");
            }
        }
        // Also clear stale clusters so they don't show inflated counts.
        if let Err(e) = state.vector_service.cluster_engine.clear_clusters().await {
            tracing::warn!(error = %e, "Failed to clear clusters (continuing with re-embed)");
        }
    }

    let sql = match mode.as_str() {
        "failed" => {
            "UPDATE emails SET embedding_status = 'pending' WHERE embedding_status = 'failed'"
        }
        "stale" => {
            "UPDATE emails SET embedding_status = 'pending' WHERE embedding_status = 'stale'"
        }
        _ => "UPDATE emails SET embedding_status = 'pending', vector_id = NULL",
    };

    let reset = sqlx::query(sql)
        .execute(&state.db.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tracing::info!(emails_reset = reset, mode = %mode, "Re-embed triggered");

    // Auto-trigger ingestion for the first account with pending emails.
    let mut ingestion_triggered = false;
    if reset > 0 {
        let account_row: Option<(String,)> = sqlx::query_as(
            "SELECT DISTINCT account_id FROM emails WHERE embedding_status = 'pending' LIMIT 1",
        )
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Some((account_id,)) = account_row {
            match state
                .vector_service
                .ingestion_pipeline
                .start_ingestion(&account_id)
                .await
            {
                Ok(job_id) => {
                    tracing::info!(job_id = %job_id, account_id = %account_id, "Auto-triggered ingestion after reembed");
                    ingestion_triggered = true;
                }
                Err(e) => {
                    tracing::info!(error = %e, "Ingestion already running; reset emails will be processed in current or next cycle");
                }
            }
        }
    }

    let msg = if ingestion_triggered {
        format!("{reset} emails queued for re-embedding. Ingestion started.")
    } else if reset > 0 {
        format!("{reset} emails queued for re-embedding. Will process on next sync cycle.")
    } else {
        "No emails to re-embed.".to_string()
    };

    Ok(Json(ReembedResponse {
        emails_reset: reset,
        mode,
        message: msg,
        ingestion_triggered,
    }))
}

// ---------------------------------------------------------------------------
// Orchestrator support types (ADR-028)
// ---------------------------------------------------------------------------

/// A tool call awaiting user confirmation before execution.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingConfirmation {
    pub confirmation_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub description: String,
}

/// Request body for POST /api/v1/ai/chat/confirm.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmToolCallRequest {
    session_id: String,
    confirmation_id: String,
    approved: bool,
}

/// Response for confirm endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmToolCallResponse {
    confirmation_id: String,
    status: String,
}

/// POST /api/v1/ai/chat/confirm -- approve or reject a pending tool call.
async fn confirm_tool_call(
    State(state): State<AppState>,
    Json(req): Json<ConfirmToolCallRequest>,
) -> Result<Json<ConfirmToolCallResponse>, (StatusCode, String)> {
    let mut pending = state.pending_confirmations.lock().await;
    let entry = pending.remove(&req.confirmation_id);

    match entry {
        Some(confirmation) => {
            let status = if req.approved {
                debug!(
                    confirmation_id = %req.confirmation_id,
                    tool = %confirmation.tool_name,
                    session_id = %req.session_id,
                    "Tool call confirmed by user"
                );
                "approved"
            } else {
                debug!(
                    confirmation_id = %req.confirmation_id,
                    tool = %confirmation.tool_name,
                    session_id = %req.session_id,
                    "Tool call rejected by user"
                );
                "rejected"
            };

            Ok(Json(ConfirmToolCallResponse {
                confirmation_id: req.confirmation_id,
                status: status.to_string(),
            }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("No pending confirmation with id: {}", req.confirmation_id),
        )),
    }
}

/// Build the canonical tool definitions matching the 7 MCP server tools.
fn build_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search_emails".into(),
            description: "Search the user's emails by query text. Returns matching emails with sender, subject, date, and relevance score.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query text" },
                    "limit": { "type": "integer", "description": "Maximum results (default: 20)", "default": 20 }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "get_email".into(),
            description: "Get full email content including headers, body, and metadata by email ID.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "email_id": { "type": "string", "description": "Unique email identifier" }
                },
                "required": ["email_id"]
            }),
        },
        ToolDefinition {
            name: "list_recent_emails".into(),
            description: "List the most recent emails across all connected accounts.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Maximum emails (default: 20, max: 100)" }
                }
            }),
        },
        ToolDefinition {
            name: "count_emails".into(),
            description: "Count emails matching optional filters. Supports filtering by sender, category, and date range.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from_filter": { "type": "string", "description": "Filter by sender (partial match)" },
                    "category": { "type": "string", "description": "Filter by category" },
                    "after": { "type": "string", "description": "Only count emails after this ISO 8601 date" },
                    "before": { "type": "string", "description": "Only count emails before this ISO 8601 date" }
                }
            }),
        },
        ToolDefinition {
            name: "get_insights".into(),
            description: "Get email analytics: counts by category, top senders, and daily volume for the last 7 days.".into(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        ToolDefinition {
            name: "list_rules".into(),
            description: "List all email rules including their conditions, actions, and status.".into(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        },
        ToolDefinition {
            name: "get_email_thread".into(),
            description: "Get all emails in the same conversation thread as the specified email.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "email_id": { "type": "string", "description": "Email ID whose thread to retrieve" }
                },
                "required": ["email_id"]
            }),
        },
    ]
}

/// Build a `ToolExecutor` that dispatches tool calls to in-process service functions.
///
/// This bridges the orchestrator's tool executor interface to the same database
/// and search services used by the MCP server, without going over the network.
fn build_tool_executor(state: &AppState) -> crate::vectors::chat_orchestrator::ToolExecutor {
    use crate::vectors::search::{HybridSearchQuery, SearchMode};

    let db = state.db.clone();
    let hybrid_search = state.vector_service.hybrid_search.clone();

    std::sync::Arc::new(move |name: &str, args: serde_json::Value| {
        let db = db.clone();
        let hybrid_search = hybrid_search.clone();
        let name = name.to_string();

        Box::pin(async move {
            match name.as_str() {
                "search_emails" => {
                    let query_text = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

                    let query = HybridSearchQuery {
                        text: query_text.to_string(),
                        mode: SearchMode::Hybrid,
                        filters: None,
                        limit: Some(limit),
                        vector_weight: 1.0,
                        fts_weight: 1.0,
                    };

                    match hybrid_search.search(&query).await {
                        Ok(result) => {
                            let items: Vec<serde_json::Value> = result.results.iter().map(|r| {
                                serde_json::json!({
                                    "email_id": r.email_id,
                                    "score": r.score,
                                    "match_type": r.match_type,
                                    "subject": r.metadata.get("subject").unwrap_or(&String::new()),
                                    "from": r.metadata.get("from_addr").unwrap_or(&String::new()),
                                    "date": r.metadata.get("received_at").unwrap_or(&String::new()),
                                })
                            }).collect();
                            Ok(
                                serde_json::json!({ "total": result.total, "results": items })
                                    .to_string(),
                            )
                        }
                        Err(e) => Err(format!("Search failed: {e}")),
                    }
                }
                "get_email" => {
                    let email_id = args.get("email_id").and_then(|v| v.as_str()).unwrap_or("");
                    let row = sqlx::query_as::<_, (String, String, Option<String>, String, String, Option<String>, Option<String>)>(
                        "SELECT id, subject, from_name, from_addr, received_at, body_text, category FROM emails WHERE id = ?1"
                    )
                    .bind(email_id)
                    .fetch_optional(&db.pool)
                    .await;

                    match row {
                        Ok(Some((
                            id,
                            subject,
                            from_name,
                            from_addr,
                            received_at,
                            body_text,
                            category,
                        ))) => {
                            let sender = match &from_name {
                                Some(name) if !name.is_empty() => format!("{name} <{from_addr}>"),
                                _ => from_addr,
                            };
                            Ok(serde_json::json!({
                                "id": id, "subject": subject, "from": sender,
                                "date": received_at, "category": category,
                                "body": body_text.unwrap_or_default(),
                            })
                            .to_string())
                        }
                        Ok(None) => {
                            Ok(serde_json::json!({ "error": "Email not found" }).to_string())
                        }
                        Err(e) => Err(format!("Database error: {e}")),
                    }
                }
                "list_recent_emails" => {
                    let limit = args
                        .get("limit")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(20)
                        .min(100);
                    let rows = sqlx::query_as::<_, (String, String, Option<String>, String, String, Option<String>)>(
                        "SELECT id, subject, from_name, from_addr, received_at, category FROM emails ORDER BY received_at DESC LIMIT ?1"
                    )
                    .bind(limit)
                    .fetch_all(&db.pool)
                    .await;

                    match rows {
                        Ok(rows) => {
                            let items: Vec<serde_json::Value> = rows.iter().map(|(id, subject, from_name, from_addr, date, category)| {
                                let sender = match from_name {
                                    Some(name) if !name.is_empty() => format!("{name} <{from_addr}>"),
                                    _ => from_addr.clone(),
                                };
                                serde_json::json!({ "id": id, "subject": subject, "from": sender, "date": date, "category": category })
                            }).collect();
                            Ok(serde_json::json!({ "count": items.len(), "emails": items })
                                .to_string())
                        }
                        Err(e) => Err(format!("Database error: {e}")),
                    }
                }
                "count_emails" => {
                    let mut sql = String::from("SELECT COUNT(*) FROM emails WHERE 1=1");
                    let mut binds: Vec<String> = Vec::new();

                    if let Some(from) = args.get("from_filter").and_then(|v| v.as_str()) {
                        sql.push_str(&format!(" AND from_addr LIKE ?{}", binds.len() + 1));
                        binds.push(format!("%{from}%"));
                    }
                    if let Some(cat) = args.get("category").and_then(|v| v.as_str()) {
                        sql.push_str(&format!(" AND category = ?{}", binds.len() + 1));
                        binds.push(cat.to_string());
                    }
                    if let Some(after) = args.get("after").and_then(|v| v.as_str()) {
                        sql.push_str(&format!(" AND received_at >= ?{}", binds.len() + 1));
                        binds.push(after.to_string());
                    }
                    if let Some(before) = args.get("before").and_then(|v| v.as_str()) {
                        sql.push_str(&format!(" AND received_at <= ?{}", binds.len() + 1));
                        binds.push(before.to_string());
                    }

                    let mut query = sqlx::query_scalar::<_, i64>(&sql);
                    for b in &binds {
                        query = query.bind(b);
                    }

                    match query.fetch_one(&db.pool).await {
                        Ok(count) => Ok(serde_json::json!({ "count": count }).to_string()),
                        Err(e) => Err(format!("Database error: {e}")),
                    }
                }
                "get_insights" => {
                    let pool = &db.pool;
                    let categories: Vec<(String, i64)> = sqlx::query_as(
                        "SELECT category, COUNT(*) FROM emails GROUP BY category ORDER BY COUNT(*) DESC"
                    ).fetch_all(pool).await.unwrap_or_default();

                    let senders: Vec<(String, i64)> = sqlx::query_as(
                        "SELECT COALESCE(from_name, from_addr), COUNT(*) FROM emails GROUP BY 1 ORDER BY 2 DESC LIMIT 10"
                    ).fetch_all(pool).await.unwrap_or_default();

                    let daily: Vec<(String, i64)> = sqlx::query_as(
                        "SELECT DATE(received_at), COUNT(*) FROM emails WHERE received_at >= datetime('now', '-7 days') GROUP BY 1 ORDER BY 1 DESC"
                    ).fetch_all(pool).await.unwrap_or_default();

                    Ok(serde_json::json!({
                        "categories": categories.iter().map(|(c, n)| serde_json::json!({"category": c, "count": n})).collect::<Vec<_>>(),
                        "top_senders": senders.iter().map(|(s, n)| serde_json::json!({"sender": s, "count": n})).collect::<Vec<_>>(),
                        "daily_volume": daily.iter().map(|(d, n)| serde_json::json!({"date": d, "count": n})).collect::<Vec<_>>(),
                    }).to_string())
                }
                "list_rules" => {
                    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
                        "SELECT id, name, conditions_json, actions_json, enabled FROM rules ORDER BY name"
                    ).fetch_all(&db.pool).await.unwrap_or_default();

                    let items: Vec<serde_json::Value> = rows.iter().map(|(id, name, conds, acts, enabled)| {
                        serde_json::json!({ "id": id, "name": name, "conditions": conds, "actions": acts, "is_active": *enabled != 0 })
                    }).collect();

                    Ok(serde_json::json!({ "count": items.len(), "rules": items }).to_string())
                }
                "get_email_thread" => {
                    let email_id = args.get("email_id").and_then(|v| v.as_str()).unwrap_or("");
                    let thread_key: Option<String> =
                        sqlx::query_scalar("SELECT thread_key FROM emails WHERE id = ?1")
                            .bind(email_id)
                            .fetch_optional(&db.pool)
                            .await
                            .unwrap_or(None);

                    match thread_key {
                        Some(key) => {
                            #[derive(sqlx::FromRow)]
                            struct ThreadRow {
                                id: String,
                                subject: String,
                                from_name: Option<String>,
                                from_addr: String,
                                received_at: String,
                                category: Option<String>,
                            }
                            let rows: Vec<ThreadRow> = sqlx::query_as(
                                "SELECT id, subject, from_name, from_addr, received_at, category FROM emails WHERE thread_key = ?1 ORDER BY received_at ASC"
                            ).bind(&key).fetch_all(&db.pool).await.unwrap_or_default();

                            let items: Vec<serde_json::Value> = rows.iter().map(|r| {
                                let sender = match &r.from_name {
                                    Some(name) if !name.is_empty() => format!("{name} <{}>", r.from_addr),
                                    _ => r.from_addr.clone(),
                                };
                                serde_json::json!({ "id": r.id, "subject": r.subject, "from": sender, "date": r.received_at, "category": r.category })
                            }).collect();

                            Ok(serde_json::json!({ "thread_key": key, "count": items.len(), "emails": items }).to_string())
                        }
                        None => Ok(serde_json::json!({ "error": "Email not found" }).to_string()),
                    }
                }
                other => Err(format!("Unknown tool: {other}")),
            }
        })
    })
}

/// Convert chat session history into `ToolMessage` array for the orchestrator.
fn session_history_to_tool_messages(
    messages: &[ChatMessage],
    system_prompt: &str,
) -> Vec<ToolMessage> {
    let mut result = vec![ToolMessage {
        role: ToolMessageRole::System,
        content: system_prompt.to_string(),
        tool_calls: None,
        tool_call_id: None,
    }];

    for msg in messages {
        let role = match msg.role {
            ChatRole::User => ToolMessageRole::User,
            ChatRole::Assistant => ToolMessageRole::Assistant,
            ChatRole::System => ToolMessageRole::System,
        };
        result.push(ToolMessage {
            role,
            content: msg.content.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    result
}

// ---------------------------------------------------------------------------
// Handlers — YAML config endpoints (Task 3)
// ---------------------------------------------------------------------------

/// GET /api/v1/ai/config/prompts — return prompts from YAML config.
async fn config_prompts(
    State(state): State<AppState>,
) -> Json<crate::vectors::yaml_config::PromptsConfig> {
    Json(state.yaml_config.prompts.clone())
}

/// GET /api/v1/ai/config/classification — return categories and rules from YAML config.
async fn config_classification(
    State(state): State<AppState>,
) -> Json<crate::vectors::yaml_config::ClassificationConfig> {
    Json(state.yaml_config.classification.clone())
}

/// GET /api/v1/ai/config/tuning — return tuning parameters from YAML config.
async fn config_tuning(
    State(state): State<AppState>,
) -> Json<crate::vectors::yaml_config::TuningConfig> {
    Json(state.yaml_config.tuning.clone())
}

/// GET /api/v1/ai/config/app — return application settings from YAML config.
///
/// Exposes sync, cache, network, defaults, hardware, security, and paths
/// configuration to the frontend so it can read configurable values
/// (e.g. polling intervals, stale times, theme defaults) instead of
/// hardcoding them.
async fn config_app(State(state): State<AppState>) -> Json<crate::vectors::yaml_config::AppConfig> {
    Json(state.yaml_config.app.clone())
}
