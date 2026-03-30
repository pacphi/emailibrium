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

    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    debug!(
        session_id = %session_id,
        message_len = req.message.len(),
        "Chat SSE stream request"
    );

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

/// Response for re-embed trigger.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReembedResponse {
    emails_reset: u64,
    message: String,
}

/// POST /api/v1/ai/reembed — mark all emails for re-embedding.
///
/// Resets all email embedding_status to 'pending', clears the vector store,
/// and lets the next poll cycle re-embed everything with the current model.
async fn trigger_reembed(
    State(state): State<AppState>,
) -> Result<Json<ReembedResponse>, (StatusCode, String)> {
    // Reset all embeddings to pending.
    let reset = sqlx::query("UPDATE emails SET embedding_status = 'pending', vector_id = NULL")
        .execute(&state.db.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tracing::info!(
        emails_reset = reset,
        "Re-embed triggered: all emails marked pending"
    );

    Ok(Json(ReembedResponse {
        emails_reset: reset,
        message: format!("{reset} emails queued for re-embedding. This will happen automatically on the next sync cycle."),
    }))
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
