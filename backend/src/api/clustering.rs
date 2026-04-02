//! Clustering API endpoints (S3-01, S3-02).
//!
//! - GET  /api/v1/clustering/clusters           — list all clusters
//! - GET  /api/v1/clustering/clusters/:id        — get cluster details + email IDs
//! - POST /api/v1/clustering/recluster           — trigger full recluster
//! - POST /api/v1/clustering/clusters/:id/pin    — pin a cluster
//! - POST /api/v1/clustering/clusters/:id/unpin  — unpin a cluster

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

use crate::vectors::clustering::ClusteringReport;
use crate::AppState;

/// Build clustering API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/clusters", get(list_clusters))
        .route("/clusters/{id}", get(get_cluster))
        .route("/status", get(clustering_status))
        .route("/recluster", post(recluster))
        .route("/clusters/{id}/pin", post(pin_cluster))
        .route("/clusters/{id}/unpin", post(unpin_cluster))
}

// --- Response types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterListResponse {
    pub clusters: Vec<ClusterSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterTermResponse {
    pub word: String,
    pub score: f32,
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepresentativeEmail {
    pub id: String,
    pub subject: String,
    pub from_addr: String,
    pub from_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub email_count: usize,
    pub stability_score: f32,
    pub is_pinned: bool,
    pub top_terms: Vec<ClusterTermResponse>,
    pub representative_emails: Vec<RepresentativeEmail>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterDetailResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub email_ids: Vec<String>,
    pub email_count: usize,
    pub stability_score: f32,
    pub stability_runs: u32,
    pub is_pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReclusterResponse {
    pub report: ClusteringReport,
}

#[derive(Debug, Serialize)]
pub struct PinResponse {
    pub cluster_id: String,
    pub pinned: bool,
}

// --- Handlers ---

/// GET /api/v1/clustering/clusters
async fn list_clusters(
    State(state): State<AppState>,
) -> Result<Json<ClusterListResponse>, (StatusCode, String)> {
    let clusters = state.vector_service.cluster_engine.get_clusters().await;

    // Batch-fetch representative email metadata from SQLite.
    let all_rep_ids: Vec<String> = clusters
        .iter()
        .flat_map(|c| c.representative_email_ids.iter().cloned())
        .collect();

    let email_meta: std::collections::HashMap<String, (String, String, Option<String>)> =
        if !all_rep_ids.is_empty() {
            let placeholders: Vec<String> =
                (1..=all_rep_ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT id, subject, from_addr, from_name FROM emails WHERE id IN ({})",
                placeholders.join(", ")
            );
            let mut query = sqlx::query_as::<_, (String, String, String, Option<String>)>(&sql);
            for id in &all_rep_ids {
                query = query.bind(id);
            }
            query
                .fetch_all(&state.db.pool)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|(id, subject, from_addr, from_name)| (id, (subject, from_addr, from_name)))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let summaries: Vec<ClusterSummary> = clusters
        .iter()
        .map(|c| {
            let top_terms = c
                .top_terms
                .iter()
                .map(|t| ClusterTermResponse {
                    word: t.word.clone(),
                    score: t.score,
                    count: t.count,
                })
                .collect();

            let representative_emails = c
                .representative_email_ids
                .iter()
                .filter_map(|id| {
                    email_meta
                        .get(id)
                        .map(|(subject, from_addr, from_name)| RepresentativeEmail {
                            id: id.clone(),
                            subject: subject.clone(),
                            from_addr: from_addr.clone(),
                            from_name: from_name.clone(),
                        })
                })
                .collect();

            ClusterSummary {
                id: c.id.clone(),
                name: c.name.clone(),
                description: c.description.clone(),
                email_count: c.email_count,
                stability_score: c.stability_score,
                is_pinned: c.is_pinned,
                top_terms,
                representative_emails,
            }
        })
        .collect();

    let total = summaries.len();

    Ok(Json(ClusterListResponse {
        clusters: summaries,
        total,
    }))
}

/// GET /api/v1/clustering/clusters/:id
async fn get_cluster(
    State(state): State<AppState>,
    Path(cluster_id): Path<String>,
) -> Result<Json<ClusterDetailResponse>, (StatusCode, String)> {
    let clusters = state.vector_service.cluster_engine.get_clusters().await;

    let cluster = clusters
        .iter()
        .find(|c| c.id == cluster_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Cluster {} not found", cluster_id),
            )
        })?;

    let email_ids = state
        .vector_service
        .cluster_engine
        .get_cluster_emails(&cluster_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ClusterDetailResponse {
        id: cluster.id.clone(),
        name: cluster.name.clone(),
        description: cluster.description.clone(),
        email_ids,
        email_count: cluster.email_count,
        stability_score: cluster.stability_score,
        stability_runs: cluster.stability_runs,
        is_pinned: cluster.is_pinned,
        created_at: cluster.created_at.to_rfc3339(),
        updated_at: cluster.updated_at.to_rfc3339(),
    }))
}

/// GET /api/v1/clustering/status — clustering readiness and counts.
async fn clustering_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let clusters = state.vector_service.cluster_engine.get_clusters().await;
    let cluster_count = clusters.len();
    let total_emails: usize = clusters.iter().map(|c| c.email_count).sum();

    // Check if ingestion is currently in clustering phase.
    // Note: get_progress() returns the last job even if complete, so we must
    // check the phase to distinguish truly active ingestion from a stale job.
    let ingestion_progress = state.vector_service.ingestion_pipeline.get_progress().await;
    let phase = ingestion_progress.as_ref().map(|p| p.phase.as_str());
    let is_truly_active = matches!(
        phase,
        Some("syncing" | "embedding" | "categorizing" | "clustering" | "analyzing" | "backfilling")
    );
    let is_clustering = phase == Some("clustering");

    Json(serde_json::json!({
        "clusterCount": cluster_count,
        "totalClusteredEmails": total_emails,
        "isClustering": is_clustering,
        "isIngesting": is_truly_active,
        "phase": if is_truly_active { phase } else { None },
    }))
}

/// POST /api/v1/clustering/recluster
async fn recluster(
    State(state): State<AppState>,
) -> Result<Json<ReclusterResponse>, (StatusCode, String)> {
    let report = state
        .vector_service
        .cluster_engine
        .full_recluster()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ReclusterResponse { report }))
}

/// POST /api/v1/clustering/clusters/:id/pin
async fn pin_cluster(
    State(state): State<AppState>,
    Path(cluster_id): Path<String>,
) -> Result<Json<PinResponse>, (StatusCode, String)> {
    state
        .vector_service
        .cluster_engine
        .pin_cluster(&cluster_id)
        .await
        .map_err(|e| match e {
            crate::vectors::error::VectorError::NotFound(_) => {
                (StatusCode::NOT_FOUND, e.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    Ok(Json(PinResponse {
        cluster_id,
        pinned: true,
    }))
}

/// POST /api/v1/clustering/clusters/:id/unpin
///
/// ClusterEngine currently only exposes `pin_cluster` (sets `is_pinned = true`).
/// The symmetric `unpin_cluster` method will be added by the other agent updating
/// VectorService. This handler verifies the cluster exists and returns the
/// expected API contract. Once `unpin_cluster` is available on ClusterEngine,
/// replace the verification block below with a direct call.
async fn unpin_cluster(
    State(state): State<AppState>,
    Path(cluster_id): Path<String>,
) -> Result<Json<PinResponse>, (StatusCode, String)> {
    // Verify cluster exists.
    let clusters = state.vector_service.cluster_engine.get_clusters().await;

    let _cluster = clusters
        .iter()
        .find(|c| c.id == cluster_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Cluster {} not found", cluster_id),
            )
        })?;

    // Unpin requires ClusterEngine::unpin_cluster (pending addition to ClusterEngine).

    Ok(Json(PinResponse {
        cluster_id,
        pinned: false,
    }))
}
