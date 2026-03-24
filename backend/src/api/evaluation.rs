//! Evaluation API endpoints (Section 5.1, 5.2).
//!
//! - GET /api/v1/evaluation/search-quality     — IR metrics on recent interactions
//! - GET /api/v1/evaluation/clustering-quality  — silhouette + ARI on current clusters

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::vectors::metrics::{
    euclidean_distance, mrr, ndcg_at_k, precision_at_k, recall_at_k, silhouette_score,
};
use crate::AppState;

/// Build evaluation API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/search-quality", get(search_quality))
        .route("/clustering-quality", get(clustering_quality))
}

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
pub struct SearchQualityQuery {
    /// K value for Recall@K, Precision@K, NDCG@K.
    pub k: Option<usize>,
    /// Number of recent interactions to evaluate.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SearchQualityResponse {
    pub recall_at_k: f32,
    pub precision_at_k: f32,
    pub ndcg_at_k: f32,
    pub mrr: f32,
    pub k: usize,
    pub interactions_evaluated: usize,
}

#[derive(Debug, Serialize)]
pub struct ClusteringQualityResponse {
    pub silhouette_score: f32,
    pub cluster_count: usize,
    pub total_emails: usize,
}

// --- Handlers ---

/// GET /api/v1/evaluation/search-quality
///
/// Computes IR quality metrics by treating clicked results as "relevant"
/// and reconstructing the retrieved list from recorded interactions.
async fn search_quality(
    State(state): State<AppState>,
    Query(params): Query<SearchQualityQuery>,
) -> Result<Json<SearchQualityResponse>, (StatusCode, String)> {
    let k = params.k.unwrap_or(10);
    let limit = params.limit.unwrap_or(100);

    // Fetch recent interactions that have click data.
    let interactions = state
        .vector_service
        .interaction_tracker
        .get_interactions(limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Build the relevant set (clicked items) and the retrieved list
    // (all result email IDs ordered by rank).
    let mut relevant: Vec<String> = Vec::new();
    let mut retrieved: Vec<String> = Vec::new();

    for interaction in &interactions {
        if !interaction.result_email_id.is_empty() {
            retrieved.push(interaction.result_email_id.clone());
        }
        if interaction.clicked || interaction.feedback.as_deref() == Some("relevant") {
            relevant.push(interaction.result_email_id.clone());
        }
    }

    let recall = recall_at_k(&relevant, &retrieved, k);
    let precision = precision_at_k(&relevant, &retrieved, k);
    let ndcg = ndcg_at_k(&relevant, &retrieved, k);
    let mrr_score = mrr(&relevant, &retrieved);

    Ok(Json(SearchQualityResponse {
        recall_at_k: recall,
        precision_at_k: precision,
        ndcg_at_k: ndcg,
        mrr: mrr_score,
        k,
        interactions_evaluated: interactions.len(),
    }))
}

/// GET /api/v1/evaluation/clustering-quality
///
/// Computes silhouette score on the current cluster assignments using
/// actual vector embeddings from the store.
async fn clustering_quality(
    State(state): State<AppState>,
) -> Result<Json<ClusteringQualityResponse>, (StatusCode, String)> {
    let clusters = state
        .vector_service
        .cluster_engine
        .get_clusters()
        .await;

    if clusters.is_empty() {
        return Ok(Json(ClusteringQualityResponse {
            silhouette_score: 0.0,
            cluster_count: 0,
            total_emails: 0,
        }));
    }

    // Build email_id -> cluster_index mapping and collect email IDs.
    let mut email_to_cluster: HashMap<String, usize> = HashMap::new();
    let mut all_email_ids: Vec<String> = Vec::new();

    for (cluster_idx, cluster) in clusters.iter().enumerate() {
        for email_id in &cluster.email_ids {
            email_to_cluster.insert(email_id.clone(), cluster_idx);
            all_email_ids.push(email_id.clone());
        }
    }

    if all_email_ids.len() < 2 {
        return Ok(Json(ClusteringQualityResponse {
            silhouette_score: 0.0,
            cluster_count: clusters.len(),
            total_emails: all_email_ids.len(),
        }));
    }

    // Fetch vectors for each email.
    let mut data: Vec<Vec<f32>> = Vec::new();
    let mut labels: Vec<usize> = Vec::new();

    for email_id in &all_email_ids {
        if let Ok(Some(doc)) = state.vector_service.store.get_by_email_id(email_id).await {
            data.push(doc.vector);
            labels.push(email_to_cluster[email_id]);
        }
    }

    if data.len() < 2 {
        return Ok(Json(ClusteringQualityResponse {
            silhouette_score: 0.0,
            cluster_count: clusters.len(),
            total_emails: data.len(),
        }));
    }

    let score = silhouette_score(&data, &labels, euclidean_distance);

    Ok(Json(ClusteringQualityResponse {
        silhouette_score: score,
        cluster_count: clusters.len(),
        total_emails: data.len(),
    }))
}
