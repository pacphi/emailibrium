//! Offline operation queue for buffering email operations (R-02).
//!
//! When the network is unavailable, operations (archive, label, delete,
//! etc.) are enqueued locally and replayed when connectivity returns.
//! Operations are processed FIFO with configurable retry limits.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{audited_sql, Database};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The kind of email operation being queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Archive,
    Label,
    Delete,
    MarkRead,
    Move,
    Unsubscribe,
}

impl OperationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Label => "label",
            Self::Delete => "delete",
            Self::MarkRead => "mark_read",
            Self::Move => "move",
            Self::Unsubscribe => "unsubscribe",
        }
    }
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for OperationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "archive" => Ok(Self::Archive),
            "label" => Ok(Self::Label),
            "delete" => Ok(Self::Delete),
            "mark_read" => Ok(Self::MarkRead),
            "move" => Ok(Self::Move),
            "unsubscribe" => Ok(Self::Unsubscribe),
            other => Err(format!("Unknown operation type: {other}")),
        }
    }
}

/// Status of a queued operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Conflict,
}

impl QueueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Conflict => "conflict",
        }
    }
}

impl std::str::FromStr for QueueStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "conflict" => Ok(Self::Conflict),
            other => Err(format!("Unknown queue status: {other}")),
        }
    }
}

/// An operation buffered in the offline queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedOperation {
    pub id: String,
    pub account_id: String,
    pub operation_type: OperationType,
    pub target_id: String,
    pub payload: Option<serde_json::Value>,
    pub status: QueueStatus,
    pub retry_count: u32,
    pub max_retries: u32,
    pub created_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

impl QueuedOperation {
    /// Create a new pending operation with a generated UUID.
    pub fn new(
        account_id: String,
        operation_type: OperationType,
        target_id: String,
        payload: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            account_id,
            operation_type,
            target_id,
            payload,
            status: QueueStatus::Pending,
            retry_count: 0,
            max_retries: 3,
            created_at: Utc::now(),
            processed_at: None,
            error: None,
        }
    }
}

/// Row type for SQLite: `created_at`/`processed_at` are TEXT (SQLite has no native
/// temporal type; the app writes/reads RFC3339 strings by hand). `retry_count`/
/// `max_retries` decode as i32, matching the actual INTEGER/INT4 column (ADR-035).
type QueueRowSqlite = (
    String,         // id
    String,         // account_id
    String,         // operation_type
    String,         // target_id
    Option<String>, // payload (JSON)
    String,         // status
    i32,            // retry_count
    i32,            // max_retries
    String,         // created_at
    Option<String>, // processed_at
    Option<String>, // error
);

/// Row type for PostgreSQL: unlike SQLite, `created_at`/`processed_at` are real
/// TIMESTAMPTZ columns there (ADR-033's dialect port), so binding/decoding the
/// RFC3339-string values the SQLite path uses fails outright — Postgres has no
/// text->timestamptz assignment cast. This arm binds/decodes `DateTime<Utc>`
/// natively instead (ADR-035).
type QueueRowPostgres = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    i32,
    i32,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
);

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Offline operation queue (SQLite or PostgreSQL).
pub struct OfflineQueue {
    db: Database,
}

impl OfflineQueue {
    /// Create a new offline queue using the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Enqueue an operation for later execution. Returns the operation ID.
    pub async fn enqueue(&self, op: &QueuedOperation) -> Result<String, sqlx::Error> {
        let op_type = op.operation_type.as_str();
        let status = op.status.as_str();
        let payload_str = op.payload.as_ref().map(|v| v.to_string());

        let sql = self.db.adapt(
            r#"INSERT INTO sync_queue
                   (id, account_id, operation_type, target_id, payload,
                    status, retry_count, max_retries, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        );
        match &self.db {
            Database::Sqlite(pool) => {
                let created = op.created_at.to_rfc3339();
                sqlx::query(audited_sql(&sql))
                    .bind(&op.id)
                    .bind(&op.account_id)
                    .bind(op_type)
                    .bind(&op.target_id)
                    .bind(&payload_str)
                    .bind(status)
                    .bind(op.retry_count as i32)
                    .bind(op.max_retries as i32)
                    .bind(&created)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(&op.id)
                    .bind(&op.account_id)
                    .bind(op_type)
                    .bind(&op.target_id)
                    .bind(&payload_str)
                    .bind(status)
                    .bind(op.retry_count as i32)
                    .bind(op.max_retries as i32)
                    .bind(op.created_at)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(op.id.clone())
    }

    /// Get next batch of pending operations (FIFO order, limited).
    ///
    /// Atomically marks fetched operations as `processing` to prevent
    /// double-dispatch by concurrent workers.
    pub async fn dequeue_batch(&self, limit: u32) -> Result<Vec<QueuedOperation>, sqlx::Error> {
        let sql = self.db.adapt(
            r#"SELECT id, account_id, operation_type, target_id, payload,
                      status, retry_count, max_retries, created_at,
                      processed_at, error
               FROM sync_queue
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT ?"#,
        );
        let mark_sql = self
            .db
            .adapt("UPDATE sync_queue SET status = 'processing' WHERE id = ?");
        let ops: Vec<QueuedOperation> = match &self.db {
            Database::Sqlite(pool) => {
                let rows: Vec<QueueRowSqlite> = sqlx::query_as(audited_sql(&sql))
                    .bind(limit as i32)
                    .fetch_all(pool)
                    .await?;
                let ops: Vec<QueuedOperation> = rows.into_iter().map(row_to_op_sqlite).collect();
                for op in &ops {
                    sqlx::query(audited_sql(&mark_sql))
                        .bind(&op.id)
                        .execute(pool)
                        .await?;
                }
                ops
            }
            Database::Postgres(pool) => {
                let rows: Vec<QueueRowPostgres> = sqlx::query_as(audited_sql(&sql))
                    .bind(limit as i32)
                    .fetch_all(pool)
                    .await?;
                let ops: Vec<QueuedOperation> = rows.into_iter().map(row_to_op_postgres).collect();
                for op in &ops {
                    sqlx::query(audited_sql(&mark_sql))
                        .bind(&op.id)
                        .execute(pool)
                        .await?;
                }
                ops
            }
        };

        Ok(ops)
    }

    /// Mark an operation as completed.
    pub async fn complete(&self, id: &str) -> Result<(), sqlx::Error> {
        let sql = self.db.adapt(
            r#"UPDATE sync_queue
               SET status = 'completed', processed_at = ?
               WHERE id = ?"#,
        );
        match &self.db {
            Database::Sqlite(pool) => {
                let now = Utc::now().to_rfc3339();
                sqlx::query(audited_sql(&sql))
                    .bind(&now)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(Utc::now())
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Mark as failed, increment retry count.
    ///
    /// If the retry count has not yet reached `max_retries`, the status
    /// is reset to `pending` so the operation will be retried. Otherwise
    /// it remains in `failed` state.
    pub async fn fail(&self, id: &str, error: &str) -> Result<(), sqlx::Error> {
        let select_sql = self
            .db
            .adapt("SELECT retry_count, max_retries FROM sync_queue WHERE id = ?");
        let update_sql = self.db.adapt(
            r#"UPDATE sync_queue
               SET status = ?, retry_count = ?, error = ?, processed_at = ?
               WHERE id = ?"#,
        );

        match &self.db {
            Database::Sqlite(pool) => {
                let row: Option<(i32, i32)> = sqlx::query_as(audited_sql(&select_sql))
                    .bind(id)
                    .fetch_optional(pool)
                    .await?;
                let (retry_count, max_retries) = row.unwrap_or((0, 3));
                let new_retry = retry_count + 1;
                let new_status = if new_retry >= max_retries {
                    "failed"
                } else {
                    "pending"
                };
                let now = Utc::now().to_rfc3339();
                sqlx::query(audited_sql(&update_sql))
                    .bind(new_status)
                    .bind(new_retry)
                    .bind(error)
                    .bind(&now)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                let row: Option<(i32, i32)> = sqlx::query_as(audited_sql(&select_sql))
                    .bind(id)
                    .fetch_optional(pool)
                    .await?;
                let (retry_count, max_retries) = row.unwrap_or((0, 3));
                let new_retry = retry_count + 1;
                let new_status = if new_retry >= max_retries {
                    "failed"
                } else {
                    "pending"
                };
                sqlx::query(audited_sql(&update_sql))
                    .bind(new_status)
                    .bind(new_retry)
                    .bind(error)
                    .bind(Utc::now())
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Mark an operation as having a conflict.
    pub async fn mark_conflict(&self, id: &str, error: &str) -> Result<(), sqlx::Error> {
        let sql = self.db.adapt(
            r#"UPDATE sync_queue
               SET status = 'conflict', error = ?, processed_at = ?
               WHERE id = ?"#,
        );
        match &self.db {
            Database::Sqlite(pool) => {
                let now = Utc::now().to_rfc3339();
                sqlx::query(audited_sql(&sql))
                    .bind(error)
                    .bind(&now)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(error)
                    .bind(Utc::now())
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Get pending count for an account.
    pub async fn pending_count(&self, account_id: &str) -> Result<u64, sqlx::Error> {
        let sql = self
            .db
            .adapt("SELECT COUNT(*) FROM sync_queue WHERE account_id = ? AND status = 'pending'");
        let row: (i64,) = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_one(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_one(pool)
                    .await?
            }
        };
        Ok(row.0.max(0) as u64)
    }

    /// Get all pending operations for display.
    pub async fn list_pending(
        &self,
        account_id: &str,
    ) -> Result<Vec<QueuedOperation>, sqlx::Error> {
        let sql = self.db.adapt(
            r#"SELECT id, account_id, operation_type, target_id, payload,
                      status, retry_count, max_retries, created_at,
                      processed_at, error
               FROM sync_queue
               WHERE account_id = ? AND status IN ('pending', 'processing')
               ORDER BY created_at ASC"#,
        );
        let ops = match &self.db {
            Database::Sqlite(pool) => {
                let rows: Vec<QueueRowSqlite> = sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_all(pool)
                    .await?;
                rows.into_iter().map(row_to_op_sqlite).collect()
            }
            Database::Postgres(pool) => {
                let rows: Vec<QueueRowPostgres> = sqlx::query_as(audited_sql(&sql))
                    .bind(account_id)
                    .fetch_all(pool)
                    .await?;
                rows.into_iter().map(row_to_op_postgres).collect()
            }
        };

        Ok(ops)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn row_to_op_sqlite(row: QueueRowSqlite) -> QueuedOperation {
    let operation_type = row
        .2
        .parse::<OperationType>()
        .unwrap_or(OperationType::Archive);
    let status = row.5.parse::<QueueStatus>().unwrap_or(QueueStatus::Pending);
    let payload = row.4.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let created_at = chrono::DateTime::parse_from_rfc3339(&row.8)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let processed_at = row
        .9
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    QueuedOperation {
        id: row.0,
        account_id: row.1,
        operation_type,
        target_id: row.3,
        payload,
        status,
        retry_count: row.6 as u32,
        max_retries: row.7 as u32,
        created_at,
        processed_at,
        error: row.10,
    }
}

fn row_to_op_postgres(row: QueueRowPostgres) -> QueuedOperation {
    let operation_type = row
        .2
        .parse::<OperationType>()
        .unwrap_or(OperationType::Archive);
    let status = row.5.parse::<QueueStatus>().unwrap_or(QueueStatus::Pending);
    let payload = row.4.as_deref().and_then(|s| serde_json::from_str(s).ok());

    QueuedOperation {
        id: row.0,
        account_id: row.1,
        operation_type,
        target_id: row.3,
        payload,
        status,
        retry_count: row.6 as u32,
        max_retries: row.7 as u32,
        created_at: row.8,
        processed_at: row.9,
        error: row.10,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"CREATE TABLE sync_queue (
                id              TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL,
                operation_type  TEXT NOT NULL,
                target_id       TEXT NOT NULL,
                payload         TEXT,
                status          TEXT DEFAULT 'pending',
                retry_count     INTEGER DEFAULT 0,
                max_retries     INTEGER DEFAULT 3,
                created_at      DATETIME DEFAULT (datetime('now')),
                processed_at    DATETIME,
                error           TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn make_op(target_id: &str) -> QueuedOperation {
        QueuedOperation::new(
            "acct-1".to_string(),
            OperationType::Archive,
            target_id.to_string(),
            None,
        )
    }

    #[tokio::test]
    async fn test_enqueue_and_dequeue() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        let op = make_op("msg-1");
        let id = queue.enqueue(&op).await.unwrap();
        assert_eq!(id, op.id);

        let batch = queue.dequeue_batch(10).await.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].target_id, "msg-1");
        assert_eq!(batch[0].operation_type, OperationType::Archive);
    }

    #[tokio::test]
    async fn test_fifo_ordering() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        // Enqueue three operations with slight time differences.
        for i in 0..3 {
            let mut op = make_op(&format!("msg-{i}"));
            // Override created_at to guarantee ordering.
            op.created_at = Utc::now() + chrono::Duration::milliseconds(i as i64 * 100);
            queue.enqueue(&op).await.unwrap();
        }

        let batch = queue.dequeue_batch(10).await.unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].target_id, "msg-0");
        assert_eq!(batch[1].target_id, "msg-1");
        assert_eq!(batch[2].target_id, "msg-2");
    }

    #[tokio::test]
    async fn test_dequeue_marks_processing() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        queue.enqueue(&make_op("msg-1")).await.unwrap();

        // First dequeue should return the item.
        let batch1 = queue.dequeue_batch(10).await.unwrap();
        assert_eq!(batch1.len(), 1);

        // Second dequeue should return nothing (it is now processing).
        let batch2 = queue.dequeue_batch(10).await.unwrap();
        assert!(batch2.is_empty());
    }

    #[tokio::test]
    async fn test_complete_operation() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        let op = make_op("msg-1");
        let id = queue.enqueue(&op).await.unwrap();
        queue.complete(&id).await.unwrap();

        // Should not appear in pending.
        let count = queue.pending_count("acct-1").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_fail_requeues_until_max_retries() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        let mut op = make_op("msg-1");
        op.max_retries = 2;
        let id = queue.enqueue(&op).await.unwrap();

        // First failure: retry_count 0 -> 1, re-queued as pending.
        queue.fail(&id, "timeout").await.unwrap();
        let count = queue.pending_count("acct-1").await.unwrap();
        assert_eq!(count, 1);

        // Second failure: retry_count 1 -> 2 >= max_retries, stays failed.
        queue.fail(&id, "timeout again").await.unwrap();
        let count = queue.pending_count("acct-1").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_mark_conflict() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        let op = make_op("msg-1");
        let id = queue.enqueue(&op).await.unwrap();
        queue
            .mark_conflict(&id, "message deleted on server")
            .await
            .unwrap();

        let pending = queue.list_pending("acct-1").await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_pending_count() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        assert_eq!(queue.pending_count("acct-1").await.unwrap(), 0);

        queue.enqueue(&make_op("msg-1")).await.unwrap();
        queue.enqueue(&make_op("msg-2")).await.unwrap();

        assert_eq!(queue.pending_count("acct-1").await.unwrap(), 2);
        assert_eq!(queue.pending_count("acct-other").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_list_pending() {
        let pool = test_pool().await;
        let queue = OfflineQueue::new(Database::Sqlite(pool));

        queue.enqueue(&make_op("msg-1")).await.unwrap();
        let op2 = make_op("msg-2");
        let id2 = queue.enqueue(&op2).await.unwrap();
        queue.complete(&id2).await.unwrap();

        let pending = queue.list_pending("acct-1").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target_id, "msg-1");
    }

    #[test]
    fn test_operation_type_roundtrip() {
        assert_eq!(
            "archive".parse::<OperationType>().unwrap(),
            OperationType::Archive
        );
        assert_eq!(OperationType::MarkRead.as_str(), "mark_read");
        assert_eq!(OperationType::Unsubscribe.to_string(), "unsubscribe");
        assert!("invalid".parse::<OperationType>().is_err());
    }

    #[test]
    fn test_queue_status_roundtrip() {
        assert_eq!(
            "conflict".parse::<QueueStatus>().unwrap(),
            QueueStatus::Conflict
        );
        assert_eq!(QueueStatus::Processing.as_str(), "processing");
        assert!("bogus".parse::<QueueStatus>().is_err());
    }

    #[test]
    fn test_queued_operation_serialization() {
        let op = QueuedOperation::new(
            "acct-1".to_string(),
            OperationType::Label,
            "msg-1".to_string(),
            Some(serde_json::json!({"labels": ["important"]})),
        );
        let json = serde_json::to_string(&op).unwrap();
        let deserialized: QueuedOperation = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.operation_type, OperationType::Label);
        assert_eq!(deserialized.target_id, "msg-1");
    }
}
