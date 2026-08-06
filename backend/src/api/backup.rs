//! Backup API endpoints (ADR-003: S1-04).
//!
//! - POST /api/v1/backup/trigger  — trigger manual backup of all vectors
//! - GET  /api/v1/backup/stats    — get backup statistics
//! - POST /api/v1/backup/restore  — restore vectors from SQLite backup
//!
//! The stats query is single-code-path SeaORM (ADR-036); the one raw statement
//! left is documented at its call site.

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::NaiveDateTime;
use sea_orm::sea_query::{Asterisk, Expr, Func};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QuerySelect, Statement};
use serde::Serialize;

use crate::db::entities::vector_backups;
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

// --- Queries ---
//
// Taking a `&DatabaseConnection` rather than `AppState` lets the tests at the
// bottom of this file drive them directly; the handler passes `&state.orm`.

/// Row count and the most recent `updated_at` over `vector_backups`.
///
/// `MAX(updated_at)` decodes as `NaiveDateTime` rather than the pre-port
/// `String`: the column is a plain `TIMESTAMP` (ADR-036's shape rule), so on
/// PostgreSQL the aggregate comes back as a timestamp and the `String` decode
/// was a runtime error waiting there. `COUNT(*)` is 8-byte on both backends.
async fn fetch_count_and_latest(
    conn: &DatabaseConnection,
) -> Result<(i64, Option<NaiveDateTime>), DbErr> {
    let row: Option<(i64, Option<NaiveDateTime>)> = vector_backups::Entity::find()
        .select_only()
        .expr_as(Expr::from(Func::count(Expr::col(Asterisk))), "backup_count")
        .expr_as(
            Func::max(Expr::col(vector_backups::Column::UpdatedAt)),
            "last_backup_at",
        )
        .into_tuple()
        .one(conn)
        .await?;
    // An unqualified aggregate always yields exactly one row; the fallback only
    // guards against a future GROUP BY sneaking in.
    Ok(row.unwrap_or((0, None)))
}

/// Total stored vector bytes, or `None` when the table is empty.
///
/// One of ADR-036's narrow raw-SQL escape hatches, for the aggregate class
/// ADR-035 catalogued. `vector_data` is BLOB on SQLite and BYTEA on PostgreSQL,
/// and `LENGTH()` counts bytes for both — but sea-query has no builder for
/// `LENGTH` over a binary column, so the statement is written out and dispatched
/// through `conn.get_database_backend()`. The result is explicitly aliased:
/// SQLite would otherwise name the column `SUM(LENGTH(vector_data))` and
/// PostgreSQL would name it `sum`, and the decode is by name.
///
/// `LENGTH` returns INTEGER/int4, so `SUM` widens to BIGINT on PostgreSQL —
/// `i64` is the right decode (`NUMERIC`, which would need a cast, only arises
/// from `SUM` over a bigint). `SUM` over zero rows is NULL, hence the `Option`.
async fn fetch_total_bytes(conn: &DatabaseConnection) -> Result<Option<i64>, DbErr> {
    let stmt = Statement::from_sql_and_values(
        conn.get_database_backend(),
        "SELECT SUM(LENGTH(vector_data)) AS total_bytes FROM vector_backups",
        [],
    );
    match conn.query_one_raw(stmt).await? {
        Some(row) => row.try_get::<Option<i64>>("", "total_bytes"),
        None => Ok(None),
    }
}

/// Aggregate statistics over the `vector_backups` table.
async fn fetch_backup_stats(conn: &DatabaseConnection) -> Result<BackupStatsResponse, DbErr> {
    let (backup_count, last_backup_at) = fetch_count_and_latest(conn).await?;
    let total_bytes = fetch_total_bytes(conn).await?;

    Ok(BackupStatsResponse {
        backup_count: backup_count as u64,
        // RFC3339 UTC, replacing the raw timestamp string the pre-port query
        // echoed — that shape was whatever the backend happened to store.
        last_backup_at: last_backup_at.map(|ts| ts.and_utc().to_rfc3339()),
        total_size_bytes: total_bytes.map(|s| s as u64),
    })
}

/// GET /api/v1/backup/stats
///
/// Queries the `vector_backups` table for aggregate statistics.
async fn backup_stats(
    State(state): State<AppState>,
) -> Result<Json<BackupStatsResponse>, (StatusCode, String)> {
    fetch_backup_stats(&state.orm)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory SQLite with migration 001, which creates both `vector_backups`
    /// and the `emails` table its `email_id` foreign key points at.
    async fn fresh_conn() -> DatabaseConnection {
        let conn = crate::db::test_sqlite_database().await.sea_orm();
        crate::db::apply_sqlite_migrations(
            &conn,
            &[include_str!(
                "../../migrations/sqlite/001_initial_schema.sql"
            )],
        )
        .await
        .expect("migrate");
        conn
    }

    /// One backup row holding `bytes` bytes of vector data, with `updated_at`
    /// written as the given literal so a test can pin both stored timestamp
    /// shapes. The owning email is seeded first for the foreign key.
    async fn seed_backup(conn: &DatabaseConnection, id: &str, bytes: usize, updated_at: &str) {
        conn.execute_unprepared(&format!(
            "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
             received_at) VALUES ('{id}', 'a1', 'gmail', 'subject {id}', 's@example.com', \
             'r@example.com', '2026-07-01T08:00:00+00:00')"
        ))
        .await
        .expect("seed email");

        let blob = "ab".repeat(bytes);
        conn.execute_unprepared(&format!(
            "INSERT INTO vector_backups (vector_id, email_id, collection, dimensions, \
             vector_data, updated_at) \
             VALUES ('v-{id}', '{id}', 'emails', 384, X'{blob}', '{updated_at}')"
        ))
        .await
        .expect("seed backup");
    }

    /// COUNT, MAX and the summed byte length over seeded rows.
    #[tokio::test]
    async fn stats_aggregate_seeded_backups() {
        let conn = fresh_conn().await;
        seed_backup(&conn, "e1", 4, "2026-07-01T08:00:00+00:00").await;
        seed_backup(&conn, "e2", 6, "2026-07-02T09:30:00+00:00").await;

        let stats = fetch_backup_stats(&conn).await.expect("stats");

        assert_eq!(stats.backup_count, 2);
        assert_eq!(stats.total_size_bytes, Some(10));
        assert_eq!(
            stats.last_backup_at.as_deref(),
            Some("2026-07-02T09:30:00+00:00")
        );
    }

    /// An empty table reports zero rows and no aggregates rather than erroring —
    /// `SUM` over zero rows is NULL, and the count query still yields one row.
    #[tokio::test]
    async fn stats_on_empty_table_report_zero_and_none() {
        let conn = fresh_conn().await;

        let stats = fetch_backup_stats(&conn).await.expect("stats");

        assert_eq!(stats.backup_count, 0);
        assert_eq!(stats.total_size_bytes, None);
        assert_eq!(stats.last_backup_at, None);
    }

    /// `MAX(updated_at)` decodes across both stored shapes and is emitted as
    /// RFC3339 UTC — the pre-port code echoed whichever shape was stored.
    #[tokio::test]
    async fn latest_backup_timestamp_normalizes_to_rfc3339() {
        let conn = fresh_conn().await;
        seed_backup(&conn, "e1", 2, "2026-07-01T08:00:00+00:00").await;
        seed_backup(&conn, "e2", 2, "2026-07-02 09:30:00").await;

        let stats = fetch_backup_stats(&conn).await.expect("stats");

        assert_eq!(
            stats.last_backup_at.as_deref(),
            Some("2026-07-02T09:30:00+00:00")
        );
    }
}
