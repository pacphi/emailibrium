//! Offline operation queue for buffering email operations (R-02).
//!
//! When the network is unavailable, operations (archive, label, delete,
//! etc.) are enqueued locally and replayed when connectivity returns.
//! Operations are processed FIFO with configurable retry limits.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::entities::sync_queue;
use crate::db::Database;

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

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Offline operation queue (SQLite or PostgreSQL).
///
/// One code path for both backends: the `sync_queue` entity declares the Rust
/// types and SeaORM owns encode/decode per backend (ADR-036), replacing the
/// pre-port `QueueRowSqlite`/`QueueRowPostgres` split and its per-backend
/// SQL-adaptation plumbing.
pub struct OfflineQueue {
    conn: DatabaseConnection,
}

impl OfflineQueue {
    /// Create a new offline queue using the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Enqueue an operation for later execution. Returns the operation ID.
    pub async fn enqueue(&self, op: &QueuedOperation) -> Result<String, sea_orm::DbErr> {
        let op_type = op.operation_type.as_str();
        let status = op.status.as_str();
        let payload_str = op.payload.as_ref().map(|v| v.to_string());

        // `processed_at`/`error` stay `NotSet` so they are omitted from the
        // INSERT and take their column defaults, exactly as the pre-port
        // 9-column INSERT did.
        sync_queue::Entity::insert(sync_queue::ActiveModel {
            id: Set(op.id.clone()),
            account_id: Set(op.account_id.clone()),
            operation_type: Set(op_type.to_owned()),
            target_id: Set(op.target_id.clone()),
            payload: Set(payload_str),
            status: Set(Some(status.to_owned())),
            retry_count: Set(Some(op.retry_count as i32)),
            max_retries: Set(Some(op.max_retries as i32)),
            created_at: Set(Some(op.created_at)),
            processed_at: NotSet,
            error: NotSet,
        })
        .exec(&self.conn)
        .await?;

        Ok(op.id.clone())
    }

    /// Get next batch of pending operations (FIFO order, limited).
    ///
    /// Atomically marks fetched operations as `processing` to prevent
    /// double-dispatch by concurrent workers.
    ///
    /// That atomicity claim is aspirational, not implemented: the SELECT and the
    /// per-row UPDATEs are separate unguarded statements, so two concurrent
    /// workers can select the same rows. The SeaORM port preserves the race
    /// byte-for-byte rather than silently changing semantics; the fix is tracked
    /// as `pl-legacy-enum-file-classes` / phase 7.
    pub async fn dequeue_batch(&self, limit: u32) -> Result<Vec<QueuedOperation>, sea_orm::DbErr> {
        let rows = sync_queue::Entity::find()
            .filter(sync_queue::Column::Status.eq("pending"))
            .order_by_asc(sync_queue::Column::CreatedAt)
            .limit(limit as u64)
            .all(&self.conn)
            .await?;
        let ops: Vec<QueuedOperation> = rows.into_iter().map(model_to_op).collect();
        for op in &ops {
            sync_queue::Entity::update_many()
                .col_expr(sync_queue::Column::Status, Expr::value("processing"))
                .filter(sync_queue::Column::Id.eq(op.id.as_str()))
                .exec(&self.conn)
                .await?;
        }

        Ok(ops)
    }

    /// Mark an operation as completed.
    pub async fn complete(&self, id: &str) -> Result<(), sea_orm::DbErr> {
        sync_queue::Entity::update_many()
            .col_expr(sync_queue::Column::Status, Expr::value("completed"))
            .col_expr(sync_queue::Column::ProcessedAt, Expr::value(Utc::now()))
            .filter(sync_queue::Column::Id.eq(id))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    /// Mark as failed, increment retry count.
    ///
    /// If the retry count has not yet reached `max_retries`, the status
    /// is reset to `pending` so the operation will be retried. Otherwise
    /// it remains in `failed` state.
    ///
    /// Same read-compute-write shape as [`Self::dequeue_batch`], and the same
    /// unguarded race: concurrent `fail` calls for one id can both read the
    /// pre-increment `retry_count` and lose one increment. Ported as-is; see
    /// `pl-legacy-enum-file-classes` / phase 7.
    pub async fn fail(&self, id: &str, error: &str) -> Result<(), sea_orm::DbErr> {
        let row: Option<(Option<i32>, Option<i32>)> = sync_queue::Entity::find()
            .select_only()
            .column(sync_queue::Column::RetryCount)
            .column(sync_queue::Column::MaxRetries)
            .filter(sync_queue::Column::Id.eq(id))
            .into_tuple()
            .one(&self.conn)
            .await?;
        // Absent row falls back to (0, 3) as before; a present row with NULL
        // counters takes the same column defaults the schema declares.
        let (retry_count, max_retries) = row
            .map(|(retry, max)| (retry.unwrap_or(0), max.unwrap_or(3)))
            .unwrap_or((0, 3));
        let new_retry = retry_count + 1;
        let new_status = if new_retry >= max_retries {
            "failed"
        } else {
            "pending"
        };
        sync_queue::Entity::update_many()
            .col_expr(sync_queue::Column::Status, Expr::value(new_status))
            .col_expr(sync_queue::Column::RetryCount, Expr::value(new_retry))
            .col_expr(sync_queue::Column::Error, Expr::value(error))
            .col_expr(sync_queue::Column::ProcessedAt, Expr::value(Utc::now()))
            .filter(sync_queue::Column::Id.eq(id))
            .exec(&self.conn)
            .await?;

        Ok(())
    }

    /// Mark an operation as having a conflict.
    pub async fn mark_conflict(&self, id: &str, error: &str) -> Result<(), sea_orm::DbErr> {
        sync_queue::Entity::update_many()
            .col_expr(sync_queue::Column::Status, Expr::value("conflict"))
            .col_expr(sync_queue::Column::Error, Expr::value(error))
            .col_expr(sync_queue::Column::ProcessedAt, Expr::value(Utc::now()))
            .filter(sync_queue::Column::Id.eq(id))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    /// Get pending count for an account.
    pub async fn pending_count(&self, account_id: &str) -> Result<u64, sea_orm::DbErr> {
        sync_queue::Entity::find()
            .filter(sync_queue::Column::AccountId.eq(account_id))
            .filter(sync_queue::Column::Status.eq("pending"))
            .count(&self.conn)
            .await
    }

    /// Get all pending operations for display.
    pub async fn list_pending(
        &self,
        account_id: &str,
    ) -> Result<Vec<QueuedOperation>, sea_orm::DbErr> {
        let rows = sync_queue::Entity::find()
            .filter(sync_queue::Column::AccountId.eq(account_id))
            .filter(sync_queue::Column::Status.is_in(["pending", "processing"]))
            .order_by_asc(sync_queue::Column::CreatedAt)
            .all(&self.conn)
            .await?;

        Ok(rows.into_iter().map(model_to_op).collect())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a `sync_queue` entity row into a `QueuedOperation`.
///
/// The entity's declared types (not a backend-concrete row tuple) are what make
/// this portable across SQLite and PostgreSQL — see ADR-036. Every unparseable
/// or absent value falls back exactly as the pre-port converters did: unknown
/// operation type -> `Archive`, unknown status -> `Pending`, unreadable
/// `created_at` -> `Utc::now()`.
fn model_to_op(row: sync_queue::Model) -> QueuedOperation {
    let operation_type = row
        .operation_type
        .parse::<OperationType>()
        .unwrap_or(OperationType::Archive);
    let status = row
        .status
        .as_deref()
        .and_then(|s| s.parse::<QueueStatus>().ok())
        .unwrap_or(QueueStatus::Pending);
    let payload = row
        .payload
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    QueuedOperation {
        id: row.id,
        account_id: row.account_id,
        operation_type,
        target_id: row.target_id,
        payload,
        status,
        retry_count: row.retry_count.unwrap_or(0) as u32,
        max_retries: row.max_retries.unwrap_or(3) as u32,
        created_at: row.created_at.unwrap_or_else(Utc::now),
        processed_at: row.processed_at,
        error: row.error,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    async fn test_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        db.sea_orm()
            .execute_unprepared(
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
            .await
            .unwrap();
        db
    }

    fn make_op(target_id: &str) -> QueuedOperation {
        QueuedOperation::new(
            "acct-1".to_string(),
            OperationType::Archive,
            target_id.to_string(),
            None,
        )
    }

    fn make_op_for(account_id: &str, target_id: &str) -> QueuedOperation {
        QueuedOperation::new(
            account_id.to_string(),
            OperationType::Archive,
            target_id.to_string(),
            None,
        )
    }

    #[tokio::test]
    async fn test_enqueue_and_dequeue() {
        let queue = OfflineQueue::new(test_db().await);

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
        let queue = OfflineQueue::new(test_db().await);

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
        let queue = OfflineQueue::new(test_db().await);

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
        let queue = OfflineQueue::new(test_db().await);

        let op = make_op("msg-1");
        let id = queue.enqueue(&op).await.unwrap();
        queue.complete(&id).await.unwrap();

        // Should not appear in pending.
        let count = queue.pending_count("acct-1").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_fail_requeues_until_max_retries() {
        let queue = OfflineQueue::new(test_db().await);

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
        let queue = OfflineQueue::new(test_db().await);

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
        let queue = OfflineQueue::new(test_db().await);

        assert_eq!(queue.pending_count("acct-1").await.unwrap(), 0);

        queue.enqueue(&make_op("msg-1")).await.unwrap();
        queue.enqueue(&make_op("msg-2")).await.unwrap();

        assert_eq!(queue.pending_count("acct-1").await.unwrap(), 2);
        assert_eq!(queue.pending_count("acct-other").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_list_pending() {
        let queue = OfflineQueue::new(test_db().await);

        queue.enqueue(&make_op("msg-1")).await.unwrap();
        let op2 = make_op("msg-2");
        let id2 = queue.enqueue(&op2).await.unwrap();
        queue.complete(&id2).await.unwrap();

        let pending = queue.list_pending("acct-1").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target_id, "msg-1");
    }

    /// Two-account scoping: `list_pending`'s `account_id` filter is the only
    /// thing keeping one account's queue out of another's view, and a dropped
    /// filter would otherwise survive the whole suite (phase-2 mutation-court
    /// finding). Covers both queue-visible statuses — `processing` rows are
    /// listed too, so the scoping must hold after a dequeue.
    #[tokio::test]
    async fn test_list_pending_scopes_to_account() {
        let queue = OfflineQueue::new(test_db().await);

        queue.enqueue(&make_op_for("acct-a", "a-1")).await.unwrap();
        queue.enqueue(&make_op_for("acct-a", "a-2")).await.unwrap();
        queue.enqueue(&make_op_for("acct-b", "b-1")).await.unwrap();

        let a = queue.list_pending("acct-a").await.unwrap();
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|op| op.account_id == "acct-a"));

        let b = queue.list_pending("acct-b").await.unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].target_id, "b-1");

        // After a dequeue flips every row to `processing`, the rows stay
        // listed and stay scoped.
        queue.dequeue_batch(10).await.unwrap();
        let a = queue.list_pending("acct-a").await.unwrap();
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|op| op.account_id == "acct-a"));
        assert_eq!(queue.list_pending("acct-b").await.unwrap().len(), 1);
    }

    /// Characterization pin: `dequeue_batch` takes no account and filters only
    /// on `status = 'pending'`, so it drains every account's operations
    /// together and one account's backlog can crowd out another's within the
    /// `limit`. Ported as-is (ADR-036 behavior preservation); recorded here so
    /// that whoever adds per-account scoping does so deliberately rather than
    /// by accident.
    #[tokio::test]
    async fn test_dequeue_batch_is_not_account_scoped() {
        let queue = OfflineQueue::new(test_db().await);

        queue.enqueue(&make_op_for("acct-a", "a-1")).await.unwrap();
        queue.enqueue(&make_op_for("acct-b", "b-1")).await.unwrap();

        let batch = queue.dequeue_batch(10).await.unwrap();
        assert_eq!(batch.len(), 2);
        assert!(batch.iter().any(|op| op.account_id == "acct-a"));
        assert!(batch.iter().any(|op| op.account_id == "acct-b"));

        // Both accounts' rows were marked `processing` by the one call.
        assert_eq!(queue.pending_count("acct-a").await.unwrap(), 0);
        assert_eq!(queue.pending_count("acct-b").await.unwrap(), 0);
    }

    /// `complete`, `fail` and `mark_conflict` are keyed by operation id; a
    /// dropped `WHERE id = ?` would flip every other account's rows too.
    #[tokio::test]
    async fn test_terminal_transitions_touch_only_their_own_row() {
        let queue = OfflineQueue::new(test_db().await);

        let a = make_op_for("acct-a", "a-1");
        let b = make_op_for("acct-b", "b-1");
        let a_id = queue.enqueue(&a).await.unwrap();
        queue.enqueue(&b).await.unwrap();

        queue.complete(&a_id).await.unwrap();
        assert_eq!(queue.pending_count("acct-a").await.unwrap(), 0);
        assert_eq!(queue.pending_count("acct-b").await.unwrap(), 1);

        let c = make_op_for("acct-a", "a-2");
        let c_id = queue.enqueue(&c).await.unwrap();
        queue.mark_conflict(&c_id, "gone upstream").await.unwrap();
        assert!(queue.list_pending("acct-a").await.unwrap().is_empty());
        assert_eq!(queue.list_pending("acct-b").await.unwrap().len(), 1);
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
