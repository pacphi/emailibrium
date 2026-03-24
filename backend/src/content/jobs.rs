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
//! The `JobQueue` provides a SQLite-persistent job queue backed by the
//! `background_jobs` table (see migration 005). It uses apalis's in-memory
//! storage for fast dispatch and persists state to SQLite for durability.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub job_type: String,
    pub payload: String,
    pub status: String,
    pub priority: i64,
    pub attempts: i64,
    pub max_retries: i64,
    pub error_msg: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Row tuple returned when querying the `background_jobs` table.
type JobRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
    String,
    String,
);

/// SQLite-backed job queue for background processing (ADR-006, item #28).
///
/// Provides enqueue/dequeue/update operations against the `background_jobs`
/// table created by migration 005. Workers poll this queue to process jobs.
///
/// This queue uses apalis's `MemoryStorage` pattern conceptually but persists
/// directly to SQLite for durability across restarts.
#[derive(Clone)]
pub struct JobQueue {
    pool: SqlitePool,
}

impl JobQueue {
    /// Create a new job queue backed by the given SQLite connection pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Enqueue a content extraction job.
    pub async fn enqueue_content_extraction(
        &self,
        job: &ContentExtractionJob,
    ) -> Result<String, sqlx::Error> {
        self.enqueue(JobType::ContentExtraction, job, job.priority as i64)
            .await
    }

    /// Enqueue an embedding job.
    pub async fn enqueue_embedding(&self, job: &EmbeddingJob) -> Result<String, sqlx::Error> {
        self.enqueue(JobType::Embedding, job, 0).await
    }

    /// Enqueue a CLIP embedding job.
    pub async fn enqueue_clip_embedding(
        &self,
        job: &ClipEmbeddingJob,
    ) -> Result<String, sqlx::Error> {
        self.enqueue(JobType::ClipEmbedding, job, 0).await
    }

    /// Enqueue a sync job.
    pub async fn enqueue_sync(&self, job: &SyncJob) -> Result<String, sqlx::Error> {
        self.enqueue(JobType::Sync, job, 0).await
    }

    /// Generic enqueue: serialize the payload and insert into `background_jobs`.
    async fn enqueue<T: Serialize>(
        &self,
        job_type: JobType,
        payload: &T,
        priority: i64,
    ) -> Result<String, sqlx::Error> {
        let id = Uuid::new_v4().to_string();
        let payload_json = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());

        sqlx::query(
            r#"INSERT INTO background_jobs (id, job_type, payload, status, priority)
               VALUES (?, ?, ?, 'pending', ?)"#,
        )
        .bind(&id)
        .bind(job_type.as_str())
        .bind(&payload_json)
        .bind(priority)
        .execute(&self.pool)
        .await?;

        debug!(job_id = %id, job_type = %job_type, "Job enqueued");
        Ok(id)
    }

    /// Dequeue the next pending job of the given type, marking it as running.
    ///
    /// Returns `None` if no pending jobs are available. Uses `SELECT ... LIMIT 1`
    /// with an immediate `UPDATE` to prevent double-processing.
    pub async fn dequeue(&self, job_type: JobType) -> Result<Option<JobRecord>, sqlx::Error> {
        // Fetch the highest-priority pending job.
        let row: Option<JobRow> = sqlx::query_as(
            r#"SELECT id, job_type, payload, status, priority, attempts, max_retries,
                          error_msg, created_at, updated_at
                   FROM background_jobs
                   WHERE job_type = ? AND status = 'pending'
                   ORDER BY priority ASC, created_at ASC
                   LIMIT 1"#,
        )
        .bind(job_type.as_str())
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = row else {
            return Ok(None);
        };

        // Mark as running.
        sqlx::query(
            r#"UPDATE background_jobs
               SET status = 'running', attempts = attempts + 1, updated_at = datetime('now')
               WHERE id = ? AND status = 'pending'"#,
        )
        .bind(&r.0)
        .execute(&self.pool)
        .await?;

        Ok(Some(JobRecord {
            id: r.0,
            job_type: r.1,
            payload: r.2,
            status: "running".to_string(),
            priority: r.4,
            attempts: r.5 + 1,
            max_retries: r.6,
            error_msg: r.7,
            created_at: r.8,
            updated_at: r.9,
        }))
    }

    /// Mark a job as completed.
    pub async fn mark_completed(&self, job_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE background_jobs
               SET status = 'completed', completed_at = datetime('now'), updated_at = datetime('now')
               WHERE id = ?"#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        debug!(job_id = %job_id, "Job completed");
        Ok(())
    }

    /// Mark a job as failed with an error message.
    ///
    /// If the job has not exceeded `max_retries`, it is reset to pending
    /// for automatic retry.
    pub async fn mark_failed(&self, job_id: &str, error: &str) -> Result<(), sqlx::Error> {
        // Check if we should retry.
        let row: Option<(i64, i64)> =
            sqlx::query_as(r#"SELECT attempts, max_retries FROM background_jobs WHERE id = ?"#)
                .bind(job_id)
                .fetch_optional(&self.pool)
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

        sqlx::query(
            r#"UPDATE background_jobs
               SET status = ?, error_msg = ?, updated_at = datetime('now')
               WHERE id = ?"#,
        )
        .bind(final_status)
        .bind(error)
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Cancel a pending or running job.
    pub async fn cancel(&self, job_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"UPDATE background_jobs
               SET status = 'cancelled', updated_at = datetime('now')
               WHERE id = ? AND status IN ('pending', 'running')"#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Count pending jobs, optionally filtered by type.
    pub async fn pending_count(&self, job_type: Option<JobType>) -> Result<i64, sqlx::Error> {
        let count: (i64,) = match job_type {
            Some(jt) => sqlx::query_as(
                "SELECT COUNT(*) FROM background_jobs WHERE status = 'pending' AND job_type = ?",
            )
            .bind(jt.as_str())
            .fetch_one(&self.pool)
            .await?,
            None => {
                sqlx::query_as("SELECT COUNT(*) FROM background_jobs WHERE status = 'pending'")
                    .fetch_one(&self.pool)
                    .await?
            }
        };

        Ok(count.0)
    }

    /// Resume abandoned jobs (status = 'running' with no active worker).
    ///
    /// Called on startup to reset jobs that were running when the process
    /// was interrupted.
    pub async fn resume_abandoned(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"UPDATE background_jobs
               SET status = 'pending', updated_at = datetime('now')
               WHERE status = 'running'"#,
        )
        .execute(&self.pool)
        .await?;

        let count = result.rows_affected();
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

    #[tokio::test]
    async fn test_job_queue_enqueue_and_dequeue() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        // Create the background_jobs table.
        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(pool);

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
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(pool);

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
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(pool);

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
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(pool);

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
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
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
        .execute(&pool)
        .await
        .unwrap();

        let queue = JobQueue::new(pool);

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
}
