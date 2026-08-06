//! Background job types and queue for async content extraction (ADR-006, item #28).
//!
//! Defines job payloads and a SQLite-backed job queue for heavy extraction tasks
//! that should not block the ingestion pipeline:
//!
//! - `ContentExtractionJob` -- run the full content pipeline on a raw email
//! - `EmbeddingJob` -- generate vector embeddings for extracted text
//! - `ClipEmbeddingJob` -- generate CLIP embeddings for image attachments
//! - `SyncJob` -- sync emails from a connected account
//!
//! Jobs are enqueued by the ingestion pipeline and processed by background
//! workers. Results are written back to the database.
//!
//! The `JobQueue` provides a persistent job queue backed by the
//! `background_jobs` table (see migration 005). It uses apalis's in-memory
//! storage for fast dispatch and persists state to the connected database for
//! durability.

use chrono::Utc;
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::entities::background_jobs as jobs;
use crate::db::Database;

/// Job to extract content from a raw email asynchronously.
///
/// Dispatched when emails arrive; runs the full `ContentPipeline::extract_all`
/// and stores results in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentExtractionJob {
    /// Email ID to process.
    pub email_id: String,
    /// Account ID that owns the email.
    pub account_id: String,
    /// Priority level (lower = higher priority).
    pub priority: u32,
}

/// Job to generate text embeddings for an email.
///
/// Dispatched after content extraction completes. Runs the embedding pipeline
/// and stores the resulting vector in the vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingJob {
    /// Email ID to embed.
    pub email_id: String,
    /// Pre-extracted text to embed (avoids re-reading from DB).
    pub text: String,
    /// Embedding model to use (from config).
    pub model: String,
}

/// Job to generate CLIP embeddings for image attachments.
///
/// Dispatched when an email has image attachments and CLIP is enabled.
/// Reads image data from the attachment store and produces a vector embedding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipEmbeddingJob {
    /// Email ID containing the image.
    pub email_id: String,
    /// Attachment index within the email.
    pub attachment_index: usize,
    /// Content-ID for inline images.
    pub content_id: Option<String>,
}

/// Job to sync emails from a connected email account.
///
/// Dispatched on a schedule or manually via the API. Connects to the
/// provider (Gmail/Outlook) and downloads new emails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    /// Account ID to sync.
    pub account_id: String,
    /// Whether to perform a full re-sync (vs. incremental).
    pub full_sync: bool,
    /// Maximum number of emails to fetch in this batch.
    pub batch_limit: Option<u32>,
}

/// Status of a background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is queued and waiting for a worker.
    Pending,
    /// Job is currently being processed.
    Running,
    /// Job completed successfully.
    Completed,
    /// Job failed (may be retried).
    Failed,
    /// Job was cancelled.
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Known job type identifiers stored in the `job_type` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobType {
    ContentExtraction,
    Embedding,
    ClipEmbedding,
    Sync,
}

impl JobType {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobType::ContentExtraction => "content_extraction",
            JobType::Embedding => "embedding",
            JobType::ClipEmbedding => "clip_embedding",
            JobType::Sync => "sync",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "content_extraction" => Some(JobType::ContentExtraction),
            "embedding" => Some(JobType::Embedding),
            "clip_embedding" => Some(JobType::ClipEmbedding),
            "sync" => Some(JobType::Sync),
            _ => None,
        }
    }
}

impl std::fmt::Display for JobType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A persisted job record from the `background_jobs` table.
///
/// `priority`/`attempts`/`max_retries` are `i32` because the `background_jobs` entity declares
/// them that way, matching the `INTEGER` column type in both backends' migrations —
/// PostgreSQL's `INTEGER` is a real 4-byte type, unlike SQLite's dynamically-8-byte one
/// (ADR-035). The entity, not this struct, is what makes that width per-backend correct
/// (ADR-036).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub job_type: String,
    pub payload: String,
    pub status: String,
    pub priority: i32,
    pub attempts: i32,
    pub max_retries: i32,
    pub error_msg: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A UTC `'YYYY-MM-DD HH:MM:SS'` timestamp for the TEXT-typed temporal columns.
///
/// `updated_at`/`completed_at` are TEXT in both dialects (ADR-035 §2.5), so the value is
/// formatted in Rust and bound as a string — byte-equivalent to what the two hand-written
/// arms this port replaced produced (`datetime('now')` on SQLite,
/// `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')` on PostgreSQL).
fn now_text() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Job queue for background processing (ADR-006, item #28).
///
/// Provides enqueue/dequeue/update operations against the `background_jobs`
/// table created by migration 005. Workers poll this queue to process jobs.
///
/// This queue uses apalis's `MemoryStorage` pattern conceptually but persists
/// to the connected database for durability across restarts. Every query runs
/// through SeaORM, so SQLite and PostgreSQL share one code path (ADR-036).
#[derive(Clone)]
pub struct JobQueue {
    conn: DatabaseConnection,
}

impl JobQueue {
    /// Create a new job queue backed by the given database connection.
    pub fn new(db: Database) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Enqueue a content extraction job.
    pub async fn enqueue_content_extraction(
        &self,
        job: &ContentExtractionJob,
    ) -> Result<String, DbErr> {
        self.enqueue(JobType::ContentExtraction, job, job.priority as i32)
            .await
    }

    /// Enqueue an embedding job.
    pub async fn enqueue_embedding(&self, job: &EmbeddingJob) -> Result<String, DbErr> {
        self.enqueue(JobType::Embedding, job, 0).await
    }

    /// Enqueue a CLIP embedding job.
    pub async fn enqueue_clip_embedding(&self, job: &ClipEmbeddingJob) -> Result<String, DbErr> {
        self.enqueue(JobType::ClipEmbedding, job, 0).await
    }

    /// Enqueue a sync job.
    pub async fn enqueue_sync(&self, job: &SyncJob) -> Result<String, DbErr> {
        self.enqueue(JobType::Sync, job, 0).await
    }

    /// Generic enqueue: serialize the payload and insert into `background_jobs`.
    async fn enqueue<T: Serialize>(
        &self,
        job_type: JobType,
        payload: &T,
        priority: i32,
    ) -> Result<String, DbErr> {
        let id = Uuid::new_v4().to_string();
        let payload_json = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());

        // The unset columns stay out of the INSERT so the table's own DEFAULTs
        // (attempts, max_retries, created_at, updated_at) still apply.
        jobs::Entity::insert(jobs::ActiveModel {
            id: Set(id.clone()),
            job_type: Set(job_type.as_str().to_owned()),
            payload: Set(payload_json),
            status: Set("pending".to_owned()),
            priority: Set(priority),
            ..Default::default()
        })
        .exec(&self.conn)
        .await?;

        debug!(job_id = %id, job_type = %job_type, "Job enqueued");
        Ok(id)
    }

    /// Dequeue the next pending job of the given type, marking it as running.
    ///
    /// Returns `None` if no pending jobs are available. Uses `SELECT ... LIMIT 1`
    /// with an immediate `UPDATE` to prevent double-processing.
    pub async fn dequeue(&self, job_type: JobType) -> Result<Option<JobRecord>, DbErr> {
        // Fetch the highest-priority pending job.
        let row = jobs::Entity::find()
            .filter(jobs::Column::JobType.eq(job_type.as_str()))
            .filter(jobs::Column::Status.eq("pending"))
            .order_by_asc(jobs::Column::Priority)
            .order_by_asc(jobs::Column::CreatedAt)
            .one(&self.conn)
            .await?;

        let Some(r) = row else {
            return Ok(None);
        };

        // Mark as running. `updated_at` is TEXT in both dialects (ADR-035 §2.5), so the
        // timestamp is formatted in Rust — see `now_text()`.
        //
        // KNOWN CLAIM RACE, preserved deliberately by the SeaORM port: the SELECT above and
        // this guarded UPDATE are not atomic, and `rows_affected` is ignored. When another
        // claimer flips the row to 'running' in between, this UPDATE matches zero rows yet
        // the caller still receives `Some(JobRecord)` — with an `attempts` fabricated below
        // from the select-time value rather than read back from the row. Pinned by
        // `dequeue_returns_a_job_it_did_not_claim_when_another_claimer_won`; fixing it is
        // out of scope here.
        jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("running"))
            .col_expr(
                jobs::Column::Attempts,
                Expr::col(jobs::Column::Attempts).add(1),
            )
            .col_expr(jobs::Column::UpdatedAt, Expr::value(now_text()))
            .filter(jobs::Column::Id.eq(r.id.clone()))
            .filter(jobs::Column::Status.eq("pending"))
            .exec(&self.conn)
            .await?;

        Ok(Some(JobRecord {
            id: r.id,
            job_type: r.job_type,
            payload: r.payload,
            status: "running".to_string(),
            priority: r.priority,
            attempts: r.attempts + 1,
            max_retries: r.max_retries,
            error_msg: r.error_msg,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    /// Mark a job as completed.
    pub async fn mark_completed(&self, job_id: &str) -> Result<(), DbErr> {
        // completed_at/updated_at are TEXT in both dialects (ADR-035 §2.5) — see `now_text()`.
        // One value for both columns, matching what a single statement's two `datetime('now')`
        // calls already produced.
        let now = now_text();
        jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("completed"))
            .col_expr(jobs::Column::CompletedAt, Expr::value(now.clone()))
            .col_expr(jobs::Column::UpdatedAt, Expr::value(now))
            .filter(jobs::Column::Id.eq(job_id))
            .exec(&self.conn)
            .await?;

        debug!(job_id = %job_id, "Job completed");
        Ok(())
    }

    /// Mark a job as failed with an error message.
    ///
    /// If the job has not exceeded `max_retries`, it is reset to pending
    /// for automatic retry.
    pub async fn mark_failed(&self, job_id: &str, error: &str) -> Result<(), DbErr> {
        // Check if we should retry. The retry decision stays in Rust, on a plain read —
        // same read-then-decide shape (and same lost-update window) as before the port.
        let row: Option<(i32, i32)> = jobs::Entity::find()
            .filter(jobs::Column::Id.eq(job_id))
            .select_only()
            .column(jobs::Column::Attempts)
            .column(jobs::Column::MaxRetries)
            .into_tuple()
            .one(&self.conn)
            .await?;

        let final_status = match row {
            Some((attempts, max_retries)) if attempts < max_retries => {
                info!(job_id = %job_id, attempts, max_retries, "Job failed, will retry");
                "pending" // Reset to pending for retry.
            }
            _ => {
                warn!(job_id = %job_id, error = %error, "Job failed permanently");
                "failed"
            }
        };

        // updated_at is TEXT in both dialects (ADR-035 §2.5) — see `now_text()`.
        jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value(final_status))
            .col_expr(jobs::Column::ErrorMsg, Expr::value(error))
            .col_expr(jobs::Column::UpdatedAt, Expr::value(now_text()))
            .filter(jobs::Column::Id.eq(job_id))
            .exec(&self.conn)
            .await?;

        Ok(())
    }

    /// Cancel a pending or running job.
    pub async fn cancel(&self, job_id: &str) -> Result<bool, DbErr> {
        // updated_at is TEXT in both dialects (ADR-035 §2.5) — see `now_text()`.
        let affected = jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("cancelled"))
            .col_expr(jobs::Column::UpdatedAt, Expr::value(now_text()))
            .filter(jobs::Column::Id.eq(job_id))
            .filter(jobs::Column::Status.is_in(["pending", "running"]))
            .exec(&self.conn)
            .await?
            .rows_affected;

        Ok(affected > 0)
    }

    /// Count pending jobs, optionally filtered by type.
    pub async fn pending_count(&self, job_type: Option<JobType>) -> Result<i64, DbErr> {
        let mut query = jobs::Entity::find().filter(jobs::Column::Status.eq("pending"));
        if let Some(jt) = job_type {
            query = query.filter(jobs::Column::JobType.eq(jt.as_str()));
        }
        // SeaORM decodes COUNT(*) as u64; the caller's `i64` contract is unchanged.
        let count = query.count(&self.conn).await?;

        Ok(count as i64)
    }

    /// Resume abandoned jobs (status = 'running' with no active worker).
    ///
    /// Called on startup to reset jobs that were running when the process
    /// was interrupted.
    pub async fn resume_abandoned(&self) -> Result<u64, DbErr> {
        // updated_at is TEXT in both dialects (ADR-035 §2.5) — see `now_text()`.
        let count = jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("pending"))
            .col_expr(jobs::Column::UpdatedAt, Expr::value(now_text()))
            .filter(jobs::Column::Status.eq("running"))
            .exec(&self.conn)
            .await?
            .rows_affected;
        if count > 0 {
            info!(count, "Resumed abandoned jobs");
        }
        Ok(count)
    }
}

/// Background job worker that polls the `JobQueue` and processes jobs.
///
/// The worker runs in a spawned tokio task and continuously polls for
/// pending jobs. Job handlers are provided as closures at construction.
pub struct JobWorker {
    queue: JobQueue,
    poll_interval: std::time::Duration,
}

impl JobWorker {
    /// Create a new worker with the given queue and poll interval.
    pub fn new(queue: JobQueue, poll_interval: std::time::Duration) -> Self {
        Self {
            queue,
            poll_interval,
        }
    }

    /// Start the worker loop for a specific job type.
    ///
    /// The `handler` closure receives the deserialized payload JSON and
    /// should return `Ok(())` on success or `Err(message)` on failure.
    pub async fn run<F, Fut>(&self, job_type: JobType, handler: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send,
    {
        info!(job_type = %job_type, "Background job worker started");

        loop {
            match self.queue.dequeue(job_type).await {
                Ok(Some(record)) => {
                    debug!(job_id = %record.id, job_type = %record.job_type, "Processing job");

                    match handler(record.payload.clone()).await {
                        Ok(()) => {
                            if let Err(e) = self.queue.mark_completed(&record.id).await {
                                error!(job_id = %record.id, error = %e, "Failed to mark job completed");
                            }
                        }
                        Err(msg) => {
                            if let Err(e) = self.queue.mark_failed(&record.id, &msg).await {
                                error!(job_id = %record.id, error = %e, "Failed to mark job failed");
                            }
                        }
                    }
                }
                Ok(None) => {
                    // No pending jobs; sleep before polling again.
                    tokio::time::sleep(self.poll_interval).await;
                }
                Err(e) => {
                    error!(error = %e, "Failed to dequeue job");
                    tokio::time::sleep(self.poll_interval).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_extraction_job_serialization() {
        let job = ContentExtractionJob {
            email_id: "email-001".to_string(),
            account_id: "acct-001".to_string(),
            priority: 1,
        };
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: ContentExtractionJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email_id, "email-001");
        assert_eq!(deserialized.priority, 1);
    }

    #[test]
    fn test_embedding_job_serialization() {
        let job = EmbeddingJob {
            email_id: "email-002".to_string(),
            text: "Hello world".to_string(),
            model: "all-MiniLM-L6-v2".to_string(),
        };
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: EmbeddingJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email_id, "email-002");
        assert_eq!(deserialized.model, "all-MiniLM-L6-v2");
    }

    #[test]
    fn test_clip_embedding_job_serialization() {
        let job = ClipEmbeddingJob {
            email_id: "email-003".to_string(),
            attachment_index: 0,
            content_id: Some("cid:image001".to_string()),
        };
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: ClipEmbeddingJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.attachment_index, 0);
        assert_eq!(deserialized.content_id, Some("cid:image001".to_string()));
    }

    #[test]
    fn test_sync_job_serialization() {
        let job = SyncJob {
            account_id: "acct-001".to_string(),
            full_sync: false,
            batch_limit: Some(100),
        };
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: SyncJob = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.full_sync);
        assert_eq!(deserialized.batch_limit, Some(100));
    }

    #[test]
    fn test_job_status_display() {
        assert_eq!(JobStatus::Pending.to_string(), "pending");
        assert_eq!(JobStatus::Running.to_string(), "running");
        assert_eq!(JobStatus::Completed.to_string(), "completed");
        assert_eq!(JobStatus::Failed.to_string(), "failed");
        assert_eq!(JobStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_job_type_roundtrip() {
        for jt in [
            JobType::ContentExtraction,
            JobType::Embedding,
            JobType::ClipEmbedding,
            JobType::Sync,
        ] {
            let s = jt.as_str();
            assert_eq!(JobType::parse_str(s), Some(jt));
        }
        assert_eq!(JobType::parse_str("unknown"), None);
    }

    #[test]
    fn test_job_type_display() {
        assert_eq!(JobType::ContentExtraction.to_string(), "content_extraction");
        assert_eq!(JobType::Embedding.to_string(), "embedding");
        assert_eq!(JobType::ClipEmbedding.to_string(), "clip_embedding");
        assert_eq!(JobType::Sync.to_string(), "sync");
    }

    /// In-memory database carrying the `background_jobs` table.
    async fn test_db() -> Database {
        use sea_orm::ConnectionTrait;

        let db = crate::db::test_sqlite_database().await;
        db.sea_orm()
            .execute_unprepared(
                r#"CREATE TABLE IF NOT EXISTS background_jobs (
                id          TEXT PRIMARY KEY,
                job_type    TEXT NOT NULL,
                payload     TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'pending',
                priority    INTEGER NOT NULL DEFAULT 0,
                attempts    INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 3,
                error_msg   TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
                scheduled_at TEXT,
                completed_at TEXT
            )"#,
            )
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn test_job_queue_enqueue_and_dequeue() {
        let db = test_db().await;

        let queue = JobQueue::new(db);

        // Enqueue a content extraction job.
        let job = ContentExtractionJob {
            email_id: "e1".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let job_id = queue.enqueue_content_extraction(&job).await.unwrap();
        assert!(!job_id.is_empty());

        // Pending count should be 1.
        let count = queue
            .pending_count(Some(JobType::ContentExtraction))
            .await
            .unwrap();
        assert_eq!(count, 1);

        // Dequeue should return the job.
        let record = queue.dequeue(JobType::ContentExtraction).await.unwrap();
        assert!(record.is_some());
        let record = record.unwrap();
        assert_eq!(record.id, job_id);
        assert_eq!(record.status, "running");

        // Pending count should now be 0.
        let count = queue
            .pending_count(Some(JobType::ContentExtraction))
            .await
            .unwrap();
        assert_eq!(count, 0);

        // Mark completed.
        queue.mark_completed(&job_id).await.unwrap();

        // Dequeue again should return None.
        let record = queue.dequeue(JobType::ContentExtraction).await.unwrap();
        assert!(record.is_none());
    }

    #[tokio::test]
    async fn test_job_queue_retry_on_failure() {
        let db = test_db().await;

        let queue = JobQueue::new(db);

        let job = EmbeddingJob {
            email_id: "e2".to_string(),
            text: "test".to_string(),
            model: "test-model".to_string(),
        };
        let _job_id = queue.enqueue_embedding(&job).await.unwrap();

        // Dequeue and fail.
        let record = queue.dequeue(JobType::Embedding).await.unwrap().unwrap();
        queue
            .mark_failed(&record.id, "transient error")
            .await
            .unwrap();

        // Should be back in pending (attempt 1 < max_retries 3).
        let count = queue.pending_count(Some(JobType::Embedding)).await.unwrap();
        assert_eq!(count, 1);

        // Fail two more times to exhaust retries.
        for _ in 0..2 {
            let r = queue.dequeue(JobType::Embedding).await.unwrap().unwrap();
            queue.mark_failed(&r.id, "still failing").await.unwrap();
        }

        // After 3 attempts, should be permanently failed.
        let count = queue.pending_count(Some(JobType::Embedding)).await.unwrap();
        assert_eq!(count, 0);

        // Dequeue returns None.
        let record = queue.dequeue(JobType::Embedding).await.unwrap();
        assert!(record.is_none());
    }

    #[tokio::test]
    async fn test_job_queue_cancel() {
        let db = test_db().await;

        let queue = JobQueue::new(db);

        let job = SyncJob {
            account_id: "a1".to_string(),
            full_sync: false,
            batch_limit: None,
        };
        let job_id = queue.enqueue_sync(&job).await.unwrap();

        // Cancel the pending job.
        let cancelled = queue.cancel(&job_id).await.unwrap();
        assert!(cancelled);

        // Should not be dequeueable.
        let record = queue.dequeue(JobType::Sync).await.unwrap();
        assert!(record.is_none());

        // Cancelling again should return false (already cancelled).
        let cancelled = queue.cancel(&job_id).await.unwrap();
        assert!(!cancelled);
    }

    #[tokio::test]
    async fn test_job_queue_resume_abandoned() {
        let db = test_db().await;

        let queue = JobQueue::new(db);

        let job = ContentExtractionJob {
            email_id: "e3".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let _job_id = queue.enqueue_content_extraction(&job).await.unwrap();

        // Dequeue to set status to running.
        let _record = queue.dequeue(JobType::ContentExtraction).await.unwrap();

        // Simulate crash: resume abandoned.
        let count = queue.resume_abandoned().await.unwrap();
        assert_eq!(count, 1);

        // Job should be dequeueable again.
        let record = queue.dequeue(JobType::ContentExtraction).await.unwrap();
        assert!(record.is_some());
    }

    #[tokio::test]
    async fn test_job_queue_priority_ordering() {
        let db = test_db().await;

        let queue = JobQueue::new(db);

        // Enqueue low priority first.
        let low = ContentExtractionJob {
            email_id: "low".to_string(),
            account_id: "a1".to_string(),
            priority: 10,
        };
        let _low_id = queue.enqueue_content_extraction(&low).await.unwrap();

        // Enqueue high priority second.
        let high = ContentExtractionJob {
            email_id: "high".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let _high_id = queue.enqueue_content_extraction(&high).await.unwrap();

        // Dequeue should return high priority first (lower number = higher priority).
        let record = queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .unwrap();
        let payload: ContentExtractionJob = serde_json::from_str(&record.payload).unwrap();
        assert_eq!(payload.email_id, "high");

        // Next should be low priority.
        let record = queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .unwrap();
        let payload: ContentExtractionJob = serde_json::from_str(&record.payload).unwrap();
        assert_eq!(payload.email_id, "low");
    }

    /// Pin for the dequeue claim sequence the ADR-036 port carries over as-is:
    /// the claim UPDATE is guarded on `status = 'pending'` and the returned
    /// record's `attempts` is the select-time value + 1, which matches the row
    /// only when the claim is uncontended. The contended interleaving (a rival
    /// claim between SELECT and UPDATE, where `rows_affected == 0` is ignored
    /// and both callers get the job) is not deterministically reachable from a
    /// test; it stays recorded in pl-legacy-enum-file-classes and phase 7's fix
    /// will flip this pin's shape.
    #[tokio::test]
    async fn test_dequeue_claim_syncs_attempts_with_the_row_when_uncontended() {
        use sea_orm::ConnectionTrait;

        let db = test_db().await;
        let conn = db.sea_orm();
        let queue = JobQueue::new(db);

        let job = ContentExtractionJob {
            email_id: "claim-1".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let job_id = queue.enqueue_content_extraction(&job).await.unwrap();

        let record = queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            record.attempts, 1,
            "fabricated attempts = select-time 0 + 1"
        );
        let row = conn
            .query_one_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT status, attempts FROM background_jobs WHERE id = ?1",
                [job_id.as_str().into()],
            ))
            .await
            .unwrap()
            .unwrap();
        let db_status: String = row.try_get("", "status").unwrap();
        let db_attempts: i32 = row.try_get("", "attempts").unwrap();
        assert_eq!(db_status, "running");
        assert_eq!(
            db_attempts, record.attempts,
            "uncontended claim: record matches row"
        );

        // Re-arm the row with a nonzero attempt count: the next claim must
        // report select-time + 1 again, proving the increment rides the
        // guarded UPDATE, not the SELECT.
        conn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE background_jobs SET status = 'pending', attempts = 7 WHERE id = ?1",
            [job_id.as_str().into()],
        ))
        .await
        .unwrap();
        let record = queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.attempts, 8);
        let db_attempts: i32 = conn
            .query_one_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT attempts FROM background_jobs WHERE id = ?1",
                [job_id.as_str().into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "attempts")
            .unwrap();
        assert_eq!(db_attempts, 8);
    }

    /// Pin: dequeue's SELECT only considers `status = 'pending'` rows — a row
    /// already claimed (`running`) is invisible even when it is the oldest.
    #[tokio::test]
    async fn test_dequeue_skips_non_pending_rows() {
        use sea_orm::ConnectionTrait;

        let db = test_db().await;
        let conn = db.sea_orm();
        let queue = JobQueue::new(db);

        let older = ContentExtractionJob {
            email_id: "older".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let newer = ContentExtractionJob {
            email_id: "newer".to_string(),
            account_id: "a1".to_string(),
            priority: 0,
        };
        let older_id = queue.enqueue_content_extraction(&older).await.unwrap();
        let newer_id = queue.enqueue_content_extraction(&newer).await.unwrap();

        // Claim the older row out-of-band; dequeue must return the newer one.
        conn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE background_jobs SET status = 'running' WHERE id = ?1",
            [older_id.as_str().into()],
        ))
        .await
        .unwrap();
        let record = queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.id, newer_id);

        // Nothing pending remains.
        assert!(queue
            .dequeue(JobType::ContentExtraction)
            .await
            .unwrap()
            .is_none());
    }
}
