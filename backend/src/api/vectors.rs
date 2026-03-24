//! Vector API endpoints (S1-05, S1-07).
//!
//! - POST /api/v1/vectors/search/semantic — pure vector search
//! - POST /api/v1/vectors/search/similar/:email_id — find similar emails
//! - POST /api/v1/vectors/classify — classify an email
//! - GET  /api/v1/vectors/health — health check
//! - GET  /api/v1/vectors/stats — vector store statistics

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::vectors::types::VectorCollection;
use crate::AppState;

/// Build vector API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/search/semantic", post(semantic_search))
        .route("/search/similar/{email_id}", post(find_similar))
        .route("/classify", post(classify_email))
        .route("/health", get(health))
        .route("/stats", get(stats))
}

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct SemanticSearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub collection: Option<String>,
    pub filters: Option<HashMap<String, String>>,
    pub min_score: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total: usize,
    pub latency_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub email_id: String,
    pub score: f32,
    pub collection: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct ClassifyRequest {
    pub email_id: String,
    pub subject: String,
    pub from_addr: String,
    pub body_text: String,
}

#[derive(Debug, Serialize)]
pub struct ClassifyResponse {
    pub email_id: String,
    pub category: String,
    pub confidence: f32,
    pub method: String,
}

// --- Handlers ---

/// POST /api/v1/vectors/search/semantic
async fn semantic_search(
    State(state): State<AppState>,
    Json(req): Json<SemanticSearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let start = std::time::Instant::now();

    let collection = match req.collection.as_deref() {
        Some("image_text") => VectorCollection::ImageText,
        Some("image_visual") => VectorCollection::ImageVisual,
        Some("attachment_text") => VectorCollection::AttachmentText,
        _ => VectorCollection::EmailText,
    };

    // Embed the query
    let query_vector = state
        .vector_service
        .embedding
        .embed(&req.query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let limit = req
        .limit
        .unwrap_or(state.vector_service.config.search.default_limit);
    let min_score = req
        .min_score
        .unwrap_or(state.vector_service.config.search.similarity_threshold);

    let params = crate::vectors::types::SearchParams {
        vector: query_vector,
        limit,
        collection,
        filters: req.filters,
        min_score: Some(min_score),
    };

    let results = state
        .vector_service
        .store
        .search(&params)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<SearchResultItem> = results
        .iter()
        .map(|r| SearchResultItem {
            email_id: r.document.email_id.clone(),
            score: r.score,
            collection: r.document.collection.to_string(),
            metadata: r.document.metadata.clone(),
        })
        .collect();

    let total = items.len();
    let latency_ms = start.elapsed().as_millis() as u64;

    Ok(Json(SearchResponse {
        results: items,
        total,
        latency_ms,
    }))
}

/// POST /api/v1/vectors/search/similar/:email_id
async fn find_similar(
    State(state): State<AppState>,
    Path(email_id): Path<String>,
    Json(req): Json<FindSimilarRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let start = std::time::Instant::now();

    // Look up the email's existing vector
    let existing = state
        .vector_service
        .store
        .get_by_email_id(&email_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("No vector found for email_id: {}", email_id),
            )
        })?;

    let limit = req.limit.unwrap_or(10);
    let params = crate::vectors::types::SearchParams {
        vector: existing.vector,
        limit,
        collection: existing.collection,
        filters: None,
        min_score: Some(0.5),
    };

    let results = state
        .vector_service
        .store
        .search(&params)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Exclude the source email itself
    let items: Vec<SearchResultItem> = results
        .iter()
        .filter(|r| r.document.email_id != email_id)
        .map(|r| SearchResultItem {
            email_id: r.document.email_id.clone(),
            score: r.score,
            collection: r.document.collection.to_string(),
            metadata: r.document.metadata.clone(),
        })
        .collect();

    let total = items.len();
    let latency_ms = start.elapsed().as_millis() as u64;

    Ok(Json(SearchResponse {
        results: items,
        total,
        latency_ms,
    }))
}

#[derive(Debug, Deserialize)]
pub struct FindSimilarRequest {
    pub limit: Option<usize>,
}

/// POST /api/v1/vectors/classify
async fn classify_email(
    State(state): State<AppState>,
    Json(req): Json<ClassifyRequest>,
) -> Result<Json<ClassifyResponse>, (StatusCode, String)> {
    let text = crate::vectors::embedding::EmbeddingPipeline::prepare_email_text(
        &req.subject,
        &req.from_addr,
        &req.body_text,
    );

    let result = state
        .vector_service
        .categorizer
        .categorize(&text)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ClassifyResponse {
        email_id: req.email_id,
        category: result.category.to_string(),
        confidence: result.confidence,
        method: result.method,
    }))
}

/// GET /api/v1/vectors/health
async fn health(State(state): State<AppState>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let health = state
        .vector_service
        .health()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let status_code = if health.status == "healthy" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((status_code, Json(health)))
}

/// GET /api/v1/vectors/stats
async fn stats(
    State(state): State<AppState>,
) -> Result<Json<crate::vectors::types::VectorStats>, (StatusCode, String)> {
    let stats = state
        .vector_service
        .stats()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(stats))
}
