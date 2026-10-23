//! Processing checkpoint service for crash recovery (R-06).
//!
//! Tracks the state of long-running processing jobs so they can resume
//! from the last successfully processed item after a crash, restart, or
//! transient failure. Complements the ingestion-level checkpoints in
//! `vectors::ingestion` with a provider-sync–scoped checkpoint lifecycle.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::db::entities::processing_checkpoints as checkpoints;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// State of a processing checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointState {
    Running,
    Paused,
    Completed,
    Failed,
    Resuming,
}

impl CheckpointState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Resuming => "resuming",
        }
    }
}

impl std::fmt::Display for CheckpointState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for CheckpointState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "resuming" => Ok(Self::Resuming),
            other => Err(format!("Unknown checkpoint state: {other}")),
        }
    }
}

/// A processing checkpoint snapshot persisted to SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingCheckpoint {
    pub job_id: String,
    pub provider: String,
    pub account_id: String,
    pub last_processed_id: Option<String>,
    pub total_count: Option<i64>,
    pub processed_count: i64,
    pub state: CheckpointState,
    pub error_message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Checkpoint service for crash-recovery state tracking (SQLite or PostgreSQL).
pub struct CheckpointService {
    conn: DatabaseConnection,
}

impl CheckpointService {
    /// Create a new checkpoint service using the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Create or update a checkpoint for a job.
    ///
    /// Uses `INSERT ... ON CONFLICT DO UPDATE` (upsert) so callers can
    /// simply call `save_checkpoint` on every progress tick without
    /// worrying about whether the row already exists. `provider` and
    /// `account_id` are deliberately absent from the conflict branch: an
    /// existing row keeps the values it was created with.
    ///
    /// `updated_at` is written as RFC3339 even though the column's DDL default
    /// produces `YYYY-MM-DD HH:MM:SS` — a pre-existing format mix preserved
    /// bug-for-bug; see [`CheckpointService::cleanup_old`] for why it matters.
    pub async fn save_checkpoint(
        &self,
        checkpoint: &ProcessingCheckpoint,
    ) -> Result<(), sea_orm::DbErr> {
        let row = checkpoints::ActiveModel {
            job_id: Set(checkpoint.job_id.clone()),
            provider: Set(checkpoint.provider.clone()),
            account_id: Set(checkpoint.account_id.clone()),
            last_processed_id: Set(checkpoint.last_processed_id.clone()),
            total_count: Set(checkpoint.total_count.map(|v| v as i32)),
            processed_count: Set(checkpoint.processed_count as i32),
            state: Set(checkpoint.state.as_str().to_owned()),
            error_message: Set(checkpoint.error_message.clone()),
            updated_at: Set(Utc::now().to_rfc3339()),
        };

        checkpoints::Entity::insert(row)
            .on_conflict(
                OnConflict::column(checkpoints::Column::JobId)
                    .update_columns([
                        checkpoints::Column::LastProcessedId,
                        checkpoints::Column::TotalCount,
                        checkpoints::Column::ProcessedCount,
                        checkpoints::Column::State,
                        checkpoints::Column::ErrorMessage,
                        checkpoints::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(&self.conn)
            .await?;

        Ok(())
    }

    /// Get the latest checkpoint for a job (for resume).
    pub async fn get_checkpoint(
        &self,
        job_id: &str,
    ) -> Result<Option<ProcessingCheckpoint>, sea_orm::DbErr> {
        let row = checkpoints::Entity::find_by_id(job_id.to_owned())
            .one(&self.conn)
            .await?;

        Ok(row.map(map_checkpoint_model))
    }

    /// Get all incomplete checkpoints (for startup resume).
    ///
    /// Returns checkpoints in states `running`, `paused`, `resuming`, or
    /// `failed` — anything that was not cleanly completed.
    pub async fn get_resumable(&self) -> Result<Vec<ProcessingCheckpoint>, sea_orm::DbErr> {
        let rows = checkpoints::Entity::find()
            .filter(checkpoints::Column::State.is_in(["running", "paused", "resuming", "failed"]))
            .order_by_desc(checkpoints::Column::UpdatedAt)
            .all(&self.conn)
            .await?;

        Ok(rows.into_iter().map(map_checkpoint_model).collect())
    }

    /// Mark a job as completed.
    pub async fn complete(&self, job_id: &str) -> Result<(), sea_orm::DbErr> {
        // update_many (not ActiveModel::update) so an unknown job stays a
        // silent zero-rows no-op, exactly as the old UPDATE behaved.
        checkpoints::Entity::update_many()
            .col_expr(checkpoints::Column::State, Expr::value("completed"))
            .col_expr(
                checkpoints::Column::UpdatedAt,
                Expr::value(Utc::now().to_rfc3339()),
            )
            .filter(checkpoints::Column::JobId.eq(job_id))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    /// Mark a job as failed with error info.
    pub async fn fail(&self, job_id: &str, error: &str) -> Result<(), sea_orm::DbErr> {
        checkpoints::Entity::update_many()
            .col_expr(checkpoints::Column::State, Expr::value("failed"))
            .col_expr(checkpoints::Column::ErrorMessage, Expr::value(error))
            .col_expr(
                checkpoints::Column::UpdatedAt,
                Expr::value(Utc::now().to_rfc3339()),
            )
            .filter(checkpoints::Column::JobId.eq(job_id))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    /// Clean up old completed checkpoints.
    ///
    /// Removes completed checkpoints older than `retention_days` days.
    /// Returns the number of rows deleted.
    ///
    /// The cutoff is computed in Rust and compared as a string, because
    /// `updated_at` is TEXT storing a `'YYYY-MM-DD HH:MM:SS'`-shaped string
    /// (no 'T', no offset — matching SQLite's own `datetime()` output, NOT the
    /// RFC3339 strings the app itself writes via `save_checkpoint`), so the
    /// cutoff is formatted identically rather than in the app's usual RFC3339
    /// shape. That is precisely the value both hand-written dialect variants
    /// used to compute — `datetime('now', ? || ' days')` on SQLite,
    /// `to_char(now() - make_interval(days => ?), ...)` on PostgreSQL (ADR-035
    /// §2.3) — so this deliberately reproduces SQLite's existing format quirk
    /// (and its narrow same-day lexicographic edge case) rather than silently
    /// fixing unrelated pre-existing behavior during a dialect port.
    pub async fn cleanup_old(&self, retention_days: u32) -> Result<u64, sea_orm::DbErr> {
        let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let result = checkpoints::Entity::delete_many()
            .filter(checkpoints::Column::State.eq("completed"))
            .filter(checkpoints::Column::UpdatedAt.lt(cutoff))
            .exec(&self.conn)
            .await?;

        Ok(result.rows_affected)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a `processing_checkpoints` entity row into a [`ProcessingCheckpoint`].
///
/// The entity's declared types (not a backend-concrete `Row`) are what make this
/// portable across SQLite and PostgreSQL — see ADR-036. `total_count`/
/// `processed_count` are `i32` there because the column is INTEGER/INT4 in both
/// dialects; the public struct's `i64` fields are a safe widen on the way out.
fn map_checkpoint_model(row: checkpoints::Model) -> ProcessingCheckpoint {
    let state = row.state.parse::<CheckpointState>().unwrap_or_else(|e| {
        warn!("Invalid checkpoint state '{}': {e}", row.state);
        CheckpointState::Failed
    });

    let updated_at = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    ProcessingCheckpoint {
        job_id: row.job_id,
        provider: row.provider,
        account_id: row.account_id,
        last_processed_id: row.last_processed_id,
        total_count: row.total_count.map(|v| v as i64),
        processed_count: row.processed_count as i64,
        state,
        error_message: row.error_message,
        updated_at,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    /// Create an in-memory SQLite database with the processing_checkpoints table.
    async fn test_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        db.sea_orm()
            .execute_unprepared(
                r#"CREATE TABLE processing_checkpoints (
                job_id           TEXT PRIMARY KEY,
                provider         TEXT NOT NULL,
                account_id       TEXT NOT NULL,
                last_processed_id TEXT,
                total_count      INTEGER,
                processed_count  INTEGER NOT NULL DEFAULT 0,
                state            TEXT NOT NULL DEFAULT 'running',
                error_message    TEXT,
                updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
            )
            .await
            .unwrap();
        db
    }

    fn make_checkpoint(job_id: &str, state: CheckpointState) -> ProcessingCheckpoint {
        ProcessingCheckpoint {
            job_id: job_id.to_string(),
            provider: "gmail".to_string(),
            account_id: "acct-1".to_string(),
            last_processed_id: None,
            total_count: Some(100),
            processed_count: 0,
            state,
            error_message: None,
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_save_and_get_checkpoint() {
        let svc = CheckpointService::new(test_db().await);

        let cp = make_checkpoint("job-1", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();

        let loaded = svc.get_checkpoint("job-1").await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.job_id, "job-1");
        assert_eq!(loaded.state, CheckpointState::Running);
        assert_eq!(loaded.total_count, Some(100));
    }

    #[tokio::test]
    async fn test_upsert_updates_existing() {
        let svc = CheckpointService::new(test_db().await);

        let mut cp = make_checkpoint("job-2", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();

        cp.processed_count = 50;
        cp.last_processed_id = Some("email-49".to_string());
        svc.save_checkpoint(&cp).await.unwrap();

        let loaded = svc.get_checkpoint("job-2").await.unwrap().unwrap();
        assert_eq!(loaded.processed_count, 50);
        assert_eq!(loaded.last_processed_id.as_deref(), Some("email-49"));
    }

    #[tokio::test]
    async fn test_get_nonexistent_returns_none() {
        let svc = CheckpointService::new(test_db().await);

        let loaded = svc.get_checkpoint("no-such-job").await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_operations_scope_to_job_id() {
        let svc = CheckpointService::new(test_db().await);

        let mut job_a = make_checkpoint("job-acct-a", CheckpointState::Running);
        job_a.account_id = "acct-a".to_string();
        job_a.processed_count = 7;
        svc.save_checkpoint(&job_a).await.unwrap();

        let mut job_b = make_checkpoint("job-acct-b", CheckpointState::Paused);
        job_b.account_id = "acct-b".to_string();
        job_b.processed_count = 11;
        svc.save_checkpoint(&job_b).await.unwrap();

        // A read returns only its own job's checkpoint.
        let loaded = svc.get_checkpoint("job-acct-a").await.unwrap().unwrap();
        assert_eq!(loaded.account_id, "acct-a");
        assert_eq!(loaded.processed_count, 7);

        // ...and a write touches only its own job's row.
        svc.complete("job-acct-a").await.unwrap();
        let other = svc.get_checkpoint("job-acct-b").await.unwrap().unwrap();
        assert_eq!(other.state, CheckpointState::Paused);
        assert_eq!(other.account_id, "acct-b");
        assert_eq!(other.processed_count, 11);
    }

    #[tokio::test]
    async fn test_complete_marks_state() {
        let svc = CheckpointService::new(test_db().await);

        let cp = make_checkpoint("job-3", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();
        svc.complete("job-3").await.unwrap();

        let loaded = svc.get_checkpoint("job-3").await.unwrap().unwrap();
        assert_eq!(loaded.state, CheckpointState::Completed);
    }

    #[tokio::test]
    async fn test_fail_stores_error() {
        let svc = CheckpointService::new(test_db().await);

        let cp = make_checkpoint("job-4", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();
        svc.fail("job-4", "connection timeout").await.unwrap();

        let loaded = svc.get_checkpoint("job-4").await.unwrap().unwrap();
        assert_eq!(loaded.state, CheckpointState::Failed);
        assert_eq!(loaded.error_message.as_deref(), Some("connection timeout"));
    }

    #[tokio::test]
    async fn test_get_resumable_excludes_completed() {
        let svc = CheckpointService::new(test_db().await);

        svc.save_checkpoint(&make_checkpoint("running-1", CheckpointState::Running))
            .await
            .unwrap();
        svc.save_checkpoint(&make_checkpoint("paused-1", CheckpointState::Paused))
            .await
            .unwrap();
        svc.save_checkpoint(&make_checkpoint("done-1", CheckpointState::Completed))
            .await
            .unwrap();
        svc.save_checkpoint(&make_checkpoint("failed-1", CheckpointState::Failed))
            .await
            .unwrap();

        let resumable = svc.get_resumable().await.unwrap();
        assert_eq!(resumable.len(), 3);
        let ids: Vec<&str> = resumable.iter().map(|c| c.job_id.as_str()).collect();
        assert!(!ids.contains(&"done-1"));
        assert!(ids.contains(&"running-1"));
        assert!(ids.contains(&"paused-1"));
        assert!(ids.contains(&"failed-1"));
    }

    #[tokio::test]
    async fn test_cleanup_old_removes_completed() {
        let db = test_db().await;
        let svc = CheckpointService::new(db.clone());

        // Insert a completed checkpoint with an old timestamp.
        db.sea_orm()
            .execute_unprepared(
                r#"INSERT INTO processing_checkpoints
                   (job_id, provider, account_id, processed_count, state, updated_at)
               VALUES ('old-job', 'gmail', 'acct', 0, 'completed', '2020-01-01T00:00:00+00:00')"#,
            )
            .await
            .unwrap();

        // Insert a recent completed checkpoint.
        svc.save_checkpoint(&make_checkpoint("new-job", CheckpointState::Completed))
            .await
            .unwrap();

        let deleted = svc.cleanup_old(30).await.unwrap();
        assert_eq!(deleted, 1);

        // The old one should be gone.
        assert!(svc.get_checkpoint("old-job").await.unwrap().is_none());
        // The recent one should remain.
        assert!(svc.get_checkpoint("new-job").await.unwrap().is_some());
    }

    #[test]
    fn test_checkpoint_state_roundtrip() {
        assert_eq!(
            "running".parse::<CheckpointState>().unwrap(),
            CheckpointState::Running
        );
        assert_eq!(CheckpointState::Paused.as_str(), "paused");
        assert_eq!(CheckpointState::Resuming.to_string(), "resuming");
        assert!("invalid".parse::<CheckpointState>().is_err());
    }

    #[test]
    fn test_checkpoint_serialization() {
        let cp = make_checkpoint("ser-1", CheckpointState::Running);
        let json = serde_json::to_string(&cp).unwrap();
        let deserialized: ProcessingCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.job_id, "ser-1");
        assert_eq!(deserialized.state, CheckpointState::Running);
    }
}
