//! Processing checkpoint service for crash recovery (R-06).
//!
//! Tracks the state of long-running processing jobs so they can resume
//! from the last successfully processed item after a crash, restart, or
//! transient failure. Complements the ingestion-level checkpoints in
//! `vectors::ingestion` with a provider-sync–scoped checkpoint lifecycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::warn;

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

/// Row type returned by SQLite queries.
type CheckpointRow = (
    String,         // job_id
    String,         // provider
    String,         // account_id
    Option<String>, // last_processed_id
    Option<i64>,    // total_count
    i64,            // processed_count
    String,         // state
    Option<String>, // error_message
    String,         // updated_at
);

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Checkpoint service backed by SQLite for crash-recovery state tracking.
pub struct CheckpointService {
    pool: SqlitePool,
}

impl CheckpointService {
    /// Create a new checkpoint service using the provided connection pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create or update a checkpoint for a job.
    ///
    /// Uses `INSERT ... ON CONFLICT DO UPDATE` (upsert) so callers can
    /// simply call `save_checkpoint` on every progress tick without
    /// worrying about whether the row already exists.
    pub async fn save_checkpoint(
        &self,
        checkpoint: &ProcessingCheckpoint,
    ) -> Result<(), sqlx::Error> {
        let state_str = checkpoint.state.as_str();
        let updated = Utc::now().to_rfc3339();

        sqlx::query(
            r#"INSERT INTO processing_checkpoints
                   (job_id, provider, account_id, last_processed_id,
                    total_count, processed_count, state, error_message, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(job_id) DO UPDATE SET
                   last_processed_id = excluded.last_processed_id,
                   total_count       = excluded.total_count,
                   processed_count   = excluded.processed_count,
                   state             = excluded.state,
                   error_message     = excluded.error_message,
                   updated_at        = excluded.updated_at"#,
        )
        .bind(&checkpoint.job_id)
        .bind(&checkpoint.provider)
        .bind(&checkpoint.account_id)
        .bind(&checkpoint.last_processed_id)
        .bind(checkpoint.total_count)
        .bind(checkpoint.processed_count)
        .bind(state_str)
        .bind(&checkpoint.error_message)
        .bind(&updated)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get the latest checkpoint for a job (for resume).
    pub async fn get_checkpoint(
        &self,
        job_id: &str,
    ) -> Result<Option<ProcessingCheckpoint>, sqlx::Error> {
        let row: Option<CheckpointRow> = sqlx::query_as(
            r#"SELECT job_id, provider, account_id, last_processed_id,
                      total_count, processed_count, state, error_message, updated_at
               FROM processing_checkpoints
               WHERE job_id = ?"#,
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_checkpoint))
    }

    /// Get all incomplete checkpoints (for startup resume).
    ///
    /// Returns checkpoints in states `running`, `paused`, `resuming`, or
    /// `failed` — anything that was not cleanly completed.
    pub async fn get_resumable(&self) -> Result<Vec<ProcessingCheckpoint>, sqlx::Error> {
        let rows: Vec<CheckpointRow> = sqlx::query_as(
            r#"SELECT job_id, provider, account_id, last_processed_id,
                      total_count, processed_count, state, error_message, updated_at
               FROM processing_checkpoints
               WHERE state IN ('running', 'paused', 'resuming', 'failed')
               ORDER BY updated_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_checkpoint).collect())
    }

    /// Mark a job as completed.
    pub async fn complete(&self, job_id: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"UPDATE processing_checkpoints
               SET state = 'completed', updated_at = ?
               WHERE job_id = ?"#,
        )
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark a job as failed with error info.
    pub async fn fail(&self, job_id: &str, error: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"UPDATE processing_checkpoints
               SET state = 'failed', error_message = ?, updated_at = ?
               WHERE job_id = ?"#,
        )
        .bind(error)
        .bind(&now)
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Clean up old completed checkpoints.
    ///
    /// Removes completed checkpoints older than `retention_days` days.
    /// Returns the number of rows deleted.
    pub async fn cleanup_old(&self, retention_days: u32) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"DELETE FROM processing_checkpoints
               WHERE state = 'completed'
                 AND updated_at < datetime('now', ? || ' days')"#,
        )
        .bind(-(retention_days as i64))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn row_to_checkpoint(row: CheckpointRow) -> ProcessingCheckpoint {
    let state = row.6.parse::<CheckpointState>().unwrap_or_else(|e| {
        warn!("Invalid checkpoint state '{}': {e}", row.6);
        CheckpointState::Failed
    });

    let updated_at = chrono::DateTime::parse_from_rfc3339(&row.8)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    ProcessingCheckpoint {
        job_id: row.0,
        provider: row.1,
        account_id: row.2,
        last_processed_id: row.3,
        total_count: row.4,
        processed_count: row.5,
        state,
        error_message: row.7,
        updated_at,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an in-memory SQLite database with the processing_checkpoints table.
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();
        pool
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
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

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
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

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
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

        let loaded = svc.get_checkpoint("no-such-job").await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_complete_marks_state() {
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

        let cp = make_checkpoint("job-3", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();
        svc.complete("job-3").await.unwrap();

        let loaded = svc.get_checkpoint("job-3").await.unwrap().unwrap();
        assert_eq!(loaded.state, CheckpointState::Completed);
    }

    #[tokio::test]
    async fn test_fail_stores_error() {
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

        let cp = make_checkpoint("job-4", CheckpointState::Running);
        svc.save_checkpoint(&cp).await.unwrap();
        svc.fail("job-4", "connection timeout").await.unwrap();

        let loaded = svc.get_checkpoint("job-4").await.unwrap().unwrap();
        assert_eq!(loaded.state, CheckpointState::Failed);
        assert_eq!(loaded.error_message.as_deref(), Some("connection timeout"));
    }

    #[tokio::test]
    async fn test_get_resumable_excludes_completed() {
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool);

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
        let pool = test_pool().await;
        let svc = CheckpointService::new(pool.clone());

        // Insert a completed checkpoint with an old timestamp.
        sqlx::query(
            r#"INSERT INTO processing_checkpoints
                   (job_id, provider, account_id, processed_count, state, updated_at)
               VALUES ('old-job', 'gmail', 'acct', 0, 'completed', '2020-01-01T00:00:00+00:00')"#,
        )
        .execute(&pool)
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
