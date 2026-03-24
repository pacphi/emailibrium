//! Backup API endpoints (ADR-003: S1-04).
//!
//! - POST /api/v1/backup/trigger  — trigger manual backup of all vectors
//! - GET  /api/v1/backup/stats    — get backup statistics
//! - POST /api/v1/backup/restore  — restore vectors from SQLite backup

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

use crate::AppState;

/// Build backup API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/trigger", post(trigger_backup))
        .route("/stats", get(backup_stats))
        .route("/restore", post(restore_backup))
}

// --- Response types ---

#[derive(Debug, Serialize)]
pub struct BackupTriggerResponse {
    pub vectors_backed_up: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct BackupStatsResponse {
    pub backup_count: u64,
    pub last_backup_at: Option<String>,
    pub total_size_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RestoreResponse {
    pub vectors_restored: u64,
    pub message: String,
}

// --- Handlers ---

/// POST /api/v1/backup/trigger
async fn trigger_backup(
    State(state): State<AppState>,
) -> Result<Json<BackupTriggerResponse>, (StatusCode, String)> {
    let count = state
        .vector_service
        .backup_service
        .backup_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(BackupTriggerResponse {
        vectors_backed_up: count,
        message: format!("Successfully backed up {} vectors", count),
    }))
}

/// GET /api/v1/backup/stats
///
/// Queries the `vector_backups` table for aggregate statistics.
async fn backup_stats(
    State(state): State<AppState>,
) -> Result<Json<BackupStatsResponse>, (StatusCode, String)> {
    // Query backup count and latest timestamp directly from the database.
    let row: (i64, Option<String>) =
        sqlx::query_as("SELECT COUNT(*), MAX(updated_at) FROM vector_backups")
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Estimate total size from vector_data column.
    let size_row: (Option<i64>,) =
        sqlx::query_as("SELECT SUM(LENGTH(vector_data)) FROM vector_backups")
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(BackupStatsResponse {
        backup_count: row.0 as u64,
        last_backup_at: row.1,
        total_size_bytes: size_row.0.map(|s| s as u64),
    }))
}

/// POST /api/v1/backup/restore
async fn restore_backup(
    State(state): State<AppState>,
) -> Result<Json<RestoreResponse>, (StatusCode, String)> {
    let docs = state
        .vector_service
        .backup_service
        .restore_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let count = docs.len() as u64;

    // Re-insert restored documents into the vector store.
    for doc in docs {
        state
            .vector_service
            .store
            .insert(doc)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(RestoreResponse {
        vectors_restored: count,
        message: format!("Successfully restored {} vectors from backup", count),
    }))
}
