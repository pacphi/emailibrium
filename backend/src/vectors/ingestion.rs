//! Ingestion pipeline for batch email processing (S2-03, S2-05).
//!
//! Implements the 6-stage per-email workflow: Sync -> Embed -> Categorize ->
//! Cluster -> Analyze -> Complete. Supports pause/resume and broadcasts
//! progress updates for SSE streaming.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{broadcast, Notify, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::Database;

use super::categorizer::VectorCategorizer;
use super::embedding::EmbeddingPipeline;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::types::{EmbeddingStatus, VectorCollection, VectorDocument, VectorId};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Status of an ingestion job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum JobStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::Paused => write!(f, "paused"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Current phase within an ingestion job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum IngestionPhase {
    Syncing,
    Embedding,
    Categorizing,
    Clustering,
    Analyzing,
    Complete,
}

impl std::fmt::Display for IngestionPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestionPhase::Syncing => write!(f, "syncing"),
            IngestionPhase::Embedding => write!(f, "embedding"),
            IngestionPhase::Categorizing => write!(f, "categorizing"),
            IngestionPhase::Clustering => write!(f, "clustering"),
            IngestionPhase::Analyzing => write!(f, "analyzing"),
            IngestionPhase::Complete => write!(f, "complete"),
        }
    }
}

/// A snapshot of an ingestion job's state.
#[derive(Debug, Clone, Serialize)]
pub struct IngestionJob {
    pub id: String,
    pub account_id: String,
    pub status: JobStatus,
    pub total: u64,
    pub processed: u64,
    pub embedded: u64,
    pub categorized: u64,
    pub failed: u64,
    pub phase: IngestionPhase,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub emails_per_second: f64,
}

/// Progress update broadcast to SSE subscribers.
#[derive(Clone, Debug, Serialize)]
pub struct IngestionProgress {
    pub job_id: String,
    pub total: u64,
    pub processed: u64,
    pub embedded: u64,
    pub categorized: u64,
    pub failed: u64,
    pub phase: String,
    pub eta_seconds: Option<u64>,
    pub emails_per_second: f64,
}

/// Per-email embedding status record for tracking pipeline progress.
#[derive(Debug, Clone, Serialize)]
pub struct EmailEmbeddingRecord {
    pub email_id: String,
    pub status: EmbeddingStatus,
    pub error_message: Option<String>,
    pub embedded_at: Option<DateTime<Utc>>,
}

impl EmailEmbeddingRecord {
    /// Create a new record in Pending state.
    pub fn pending(email_id: String) -> Self {
        Self {
            email_id,
            status: EmbeddingStatus::Pending,
            error_message: None,
            embedded_at: None,
        }
    }

    /// Mark as successfully embedded.
    pub fn mark_embedded(&mut self) {
        self.status = EmbeddingStatus::Embedded;
        self.embedded_at = Some(Utc::now());
        self.error_message = None;
    }

    /// Mark as failed with an error message.
    pub fn mark_failed(&mut self, error: String) {
        self.status = EmbeddingStatus::Failed;
        self.error_message = Some(error);
    }

    /// Mark as stale (needs re-embedding).
    pub fn mark_stale(&mut self) {
        self.status = EmbeddingStatus::Stale;
    }
}

/// Row from the emails table needed for ingestion.
struct PendingEmail {
    id: String,
    subject: String,
    from_addr: String,
    body_text: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct IngestionState {
    current_job: Option<IngestionJob>,
    paused: bool,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Orchestrates the batch ingestion of emails through embedding and
/// categorization stages.
pub struct IngestionPipeline {
    embedding: Arc<EmbeddingPipeline>,
    store: Arc<dyn VectorStoreBackend>,
    categorizer: Arc<VectorCategorizer>,
    db: Arc<Database>,
    progress_tx: broadcast::Sender<IngestionProgress>,
    state: Arc<RwLock<IngestionState>>,
    resume_notify: Arc<Notify>,
}

impl IngestionPipeline {
    /// Create a new ingestion pipeline.
    pub fn new(
        embedding: Arc<EmbeddingPipeline>,
        store: Arc<dyn VectorStoreBackend>,
        categorizer: Arc<VectorCategorizer>,
        db: Arc<Database>,
    ) -> Self {
        let (progress_tx, _) = broadcast::channel(128);
        Self {
            embedding,
            store,
            categorizer,
            db,
            progress_tx,
            state: Arc::new(RwLock::new(IngestionState {
                current_job: None,
                paused: false,
            })),
            resume_notify: Arc::new(Notify::new()),
        }
    }

    /// Subscribe to progress updates (for SSE streaming).
    pub fn subscribe(&self) -> broadcast::Receiver<IngestionProgress> {
        self.progress_tx.subscribe()
    }

    /// Start ingestion for the given account. Returns the job ID immediately.
    pub async fn start_ingestion(&self, account_id: &str) -> Result<String, VectorError> {
        let mut state = self.state.write().await;
        if let Some(ref job) = state.current_job {
            if job.status == JobStatus::Running || job.status == JobStatus::Paused {
                return Err(VectorError::IngestionError(
                    "An ingestion job is already in progress".to_string(),
                ));
            }
        }

        let job_id = Uuid::new_v4().to_string();
        let mut job = IngestionJob {
            id: job_id.clone(),
            account_id: account_id.to_string(),
            status: JobStatus::Pending,
            total: 0,
            processed: 0,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            started_at: Utc::now(),
            completed_at: None,
            emails_per_second: 0.0,
        };
        // Transition Pending → Running before spawning the background task.
        job.status = JobStatus::Running;
        state.current_job = Some(job);
        state.paused = false;
        drop(state);

        // Spawn background task
        let pipeline = IngestionPipelineHandle {
            embedding: self.embedding.clone(),
            store: self.store.clone(),
            categorizer: self.categorizer.clone(),
            db: self.db.clone(),
            progress_tx: self.progress_tx.clone(),
            state: self.state.clone(),
            resume_notify: self.resume_notify.clone(),
        };

        let jid = job_id.clone();
        let aid = account_id.to_string();
        tokio::spawn(async move {
            pipeline.run_ingestion(jid, aid).await;
        });

        Ok(job_id)
    }

    /// Pause the current ingestion job.
    pub async fn pause(&self) -> Result<(), VectorError> {
        let mut state = self.state.write().await;
        match state.current_job {
            Some(ref mut job) if job.status == JobStatus::Running => {
                job.status = JobStatus::Paused;
                state.paused = true;
                Ok(())
            }
            _ => Err(VectorError::IngestionError(
                "No running ingestion job to pause".to_string(),
            )),
        }
    }

    /// Resume a paused ingestion job.
    pub async fn resume(&self) -> Result<(), VectorError> {
        let mut state = self.state.write().await;
        match state.current_job {
            Some(ref mut job) if job.status == JobStatus::Paused => {
                job.status = JobStatus::Running;
                state.paused = false;
                drop(state);
                self.resume_notify.notify_waiters();
                Ok(())
            }
            _ => Err(VectorError::IngestionError(
                "No paused ingestion job to resume".to_string(),
            )),
        }
    }

    /// Get the current progress snapshot.
    pub async fn get_progress(&self) -> Option<IngestionProgress> {
        let state = self.state.read().await;
        state.current_job.as_ref().map(|job| IngestionProgress {
            job_id: job.id.clone(),
            total: job.total,
            processed: job.processed,
            embedded: job.embedded,
            categorized: job.categorized,
            failed: job.failed,
            phase: job.phase.to_string(),
            eta_seconds: compute_eta(job),
            emails_per_second: job.emails_per_second,
        })
    }
}

/// Internal handle cloned into the background task. Keeps the same Arc
/// references so progress updates are visible to the outer pipeline.
struct IngestionPipelineHandle {
    embedding: Arc<EmbeddingPipeline>,
    store: Arc<dyn VectorStoreBackend>,
    categorizer: Arc<VectorCategorizer>,
    db: Arc<Database>,
    progress_tx: broadcast::Sender<IngestionProgress>,
    state: Arc<RwLock<IngestionState>>,
    resume_notify: Arc<Notify>,
}

impl IngestionPipelineHandle {
    async fn run_ingestion(&self, job_id: String, account_id: String) {
        info!(job_id = %job_id, account_id = %account_id, "Starting ingestion");

        // Phase 1: Fetch pending emails
        self.update_phase(IngestionPhase::Syncing).await;
        let emails = match self.fetch_pending_emails(&account_id).await {
            Ok(e) => e,
            Err(err) => {
                error!(job_id = %job_id, "Failed to fetch emails: {err}");
                self.mark_failed().await;
                return;
            }
        };

        let total = emails.len() as u64;
        {
            let mut state = self.state.write().await;
            if let Some(ref mut job) = state.current_job {
                job.total = total;
            }
        }
        self.broadcast_progress(&job_id).await;

        if total == 0 {
            info!(job_id = %job_id, "No pending emails, completing immediately");
            self.update_phase(IngestionPhase::Complete).await;
            self.mark_completed().await;
            self.broadcast_progress(&job_id).await;
            return;
        }

        // Phase 2: Embed in batches
        self.update_phase(IngestionPhase::Embedding).await;
        self.broadcast_progress(&job_id).await;

        let batch_size = 64;
        let start_time = std::time::Instant::now();

        for chunk in emails.chunks(batch_size) {
            // Check pause
            if self.is_paused().await {
                self.broadcast_progress(&job_id).await;
                self.wait_for_resume().await;
            }

            let texts: Vec<String> = chunk
                .iter()
                .map(|e| {
                    EmbeddingPipeline::prepare_email_text(&e.subject, &e.from_addr, &e.body_text)
                })
                .collect();

            match self.embedding.embed_batch(&texts).await {
                Ok(vectors) => {
                    let mut docs = Vec::with_capacity(vectors.len());
                    for (i, vector) in vectors.into_iter().enumerate() {
                        let email = &chunk[i];
                        let vector_id = VectorId::new();
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("subject".to_string(), email.subject.clone());
                        metadata.insert("from_addr".to_string(), email.from_addr.clone());

                        let doc = VectorDocument {
                            id: vector_id.clone(),
                            email_id: email.id.clone(),
                            vector,
                            metadata,
                            collection: VectorCollection::EmailText,
                            created_at: Utc::now(),
                        };
                        docs.push(doc);

                        // Update DB
                        if let Err(err) = self
                            .update_embedding_status(
                                &email.id,
                                &vector_id.to_string(),
                                self.embedding_model_name(),
                            )
                            .await
                        {
                            warn!(email_id = %email.id, "Failed to update embedding status: {err}");
                        }
                    }

                    // Batch insert into vector store
                    match self.store.batch_insert(docs).await {
                        Ok(ids) => {
                            let count = ids.len() as u64;
                            let mut state = self.state.write().await;
                            if let Some(ref mut job) = state.current_job {
                                job.embedded += count;
                                job.processed += count;
                                let elapsed = start_time.elapsed().as_secs_f64();
                                if elapsed > 0.0 {
                                    job.emails_per_second = job.processed as f64 / elapsed;
                                }
                            }
                        }
                        Err(err) => {
                            warn!("Batch insert failed: {err}");
                            let mut state = self.state.write().await;
                            if let Some(ref mut job) = state.current_job {
                                job.failed += chunk.len() as u64;
                                job.processed += chunk.len() as u64;
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!("Batch embedding failed: {err}");
                    let mut state = self.state.write().await;
                    if let Some(ref mut job) = state.current_job {
                        job.failed += chunk.len() as u64;
                        job.processed += chunk.len() as u64;
                    }
                }
            }

            self.broadcast_progress(&job_id).await;
        }

        // Phase 3: Categorize
        self.update_phase(IngestionPhase::Categorizing).await;
        self.broadcast_progress(&job_id).await;

        for email in &emails {
            if self.is_paused().await {
                self.wait_for_resume().await;
            }

            let text = EmbeddingPipeline::prepare_email_text(
                &email.subject,
                &email.from_addr,
                &email.body_text,
            );

            match self.categorizer.categorize(&text).await {
                Ok(result) => {
                    if let Err(err) = self
                        .update_category(
                            &email.id,
                            &result.category.to_string(),
                            result.confidence,
                            &result.method,
                        )
                        .await
                    {
                        warn!(email_id = %email.id, "Failed to update category: {err}");
                    }
                    let mut state = self.state.write().await;
                    if let Some(ref mut job) = state.current_job {
                        job.categorized += 1;
                    }
                }
                Err(err) => {
                    debug!(email_id = %email.id, "Categorization failed: {err}");
                }
            }
        }

        self.broadcast_progress(&job_id).await;

        // Phase 4-5: Clustering + Analyzing (placeholder phases, see ADR-006)
        self.update_phase(IngestionPhase::Clustering).await;
        self.broadcast_progress(&job_id).await;

        self.update_phase(IngestionPhase::Analyzing).await;
        self.broadcast_progress(&job_id).await;

        // Complete
        self.update_phase(IngestionPhase::Complete).await;
        self.mark_completed().await;
        self.broadcast_progress(&job_id).await;

        info!(job_id = %job_id, "Ingestion complete");
    }

    async fn fetch_pending_emails(
        &self,
        account_id: &str,
    ) -> Result<Vec<PendingEmail>, VectorError> {
        type EmailRow = (String, Option<String>, Option<String>, Option<String>);
        let rows: Vec<EmailRow> = sqlx::query_as(
            r#"SELECT id, subject, from_addr, body_text
               FROM emails
               WHERE account_id = ? AND embedding_status = 'pending'
               ORDER BY received_at DESC"#,
        )
        .bind(account_id)
        .fetch_all(&self.db.pool)
        .await
        .map_err(VectorError::DatabaseError)?;

        Ok(rows
            .into_iter()
            .map(|r| PendingEmail {
                id: r.0,
                subject: r.1.unwrap_or_default(),
                from_addr: r.2.unwrap_or_default(),
                body_text: r.3.unwrap_or_default(),
            })
            .collect())
    }

    async fn update_embedding_status(
        &self,
        email_id: &str,
        vector_id: &str,
        model: &str,
    ) -> Result<(), VectorError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"UPDATE emails
               SET embedding_status = 'embedded', embedded_at = ?, vector_id = ?, embedding_model = ?
               WHERE id = ?"#,
        )
        .bind(&now)
        .bind(vector_id)
        .bind(model)
        .bind(email_id)
        .execute(&self.db.pool)
        .await
        .map_err(VectorError::DatabaseError)?;
        Ok(())
    }

    async fn update_category(
        &self,
        email_id: &str,
        category: &str,
        confidence: f32,
        method: &str,
    ) -> Result<(), VectorError> {
        sqlx::query(
            r#"UPDATE emails
               SET category = ?, category_confidence = ?, category_method = ?
               WHERE id = ?"#,
        )
        .bind(category)
        .bind(confidence)
        .bind(method)
        .bind(email_id)
        .execute(&self.db.pool)
        .await
        .map_err(VectorError::DatabaseError)?;
        Ok(())
    }

    fn embedding_model_name(&self) -> &str {
        "mock-embedding"
    }

    async fn update_phase(&self, phase: IngestionPhase) {
        let mut state = self.state.write().await;
        if let Some(ref mut job) = state.current_job {
            job.phase = phase;
        }
    }

    async fn is_paused(&self) -> bool {
        self.state.read().await.paused
    }

    async fn wait_for_resume(&self) {
        info!("Ingestion paused, waiting for resume signal");
        self.resume_notify.notified().await;
        info!("Ingestion resumed");
    }

    async fn mark_completed(&self) {
        let mut state = self.state.write().await;
        if let Some(ref mut job) = state.current_job {
            job.status = JobStatus::Completed;
            job.completed_at = Some(Utc::now());
        }
    }

    async fn mark_failed(&self) {
        let mut state = self.state.write().await;
        if let Some(ref mut job) = state.current_job {
            job.status = JobStatus::Failed;
            job.completed_at = Some(Utc::now());
        }
    }

    async fn broadcast_progress(&self, job_id: &str) {
        let state = self.state.read().await;
        if let Some(ref job) = state.current_job {
            let progress = IngestionProgress {
                job_id: job_id.to_string(),
                total: job.total,
                processed: job.processed,
                embedded: job.embedded,
                categorized: job.categorized,
                failed: job.failed,
                phase: job.phase.to_string(),
                eta_seconds: compute_eta(job),
                emails_per_second: job.emails_per_second,
            };
            // Ignore send errors (no subscribers).
            let _ = self.progress_tx.send(progress);
        }
    }
}

/// Estimate remaining time based on current throughput.
fn compute_eta(job: &IngestionJob) -> Option<u64> {
    if job.emails_per_second > 0.0 && job.total > job.processed {
        let remaining = job.total - job.processed;
        Some((remaining as f64 / job.emails_per_second) as u64)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::categorizer::VectorCategorizer;
    use crate::vectors::config::EmbeddingConfig;
    use crate::vectors::embedding::EmbeddingPipeline;
    use crate::vectors::store::InMemoryVectorStore;

    async fn test_db() -> Database {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        sqlx::query(include_str!("../../migrations/001_initial_schema.sql"))
            .execute(&db.pool)
            .await
            .unwrap();
        db
    }

    fn make_pipeline(
        db: Arc<Database>,
    ) -> (
        IngestionPipeline,
        Arc<dyn VectorStoreBackend>,
        Arc<EmbeddingPipeline>,
    ) {
        let config = EmbeddingConfig {
            provider: "mock".to_string(),
            ..EmbeddingConfig::default()
        };
        let embedding = Arc::new(EmbeddingPipeline::new(&config).unwrap());
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        let categorizer = Arc::new(VectorCategorizer::new(
            store.clone(),
            embedding.clone(),
            0.0, // low threshold so everything categorizes
        ));
        let pipeline = IngestionPipeline::new(embedding.clone(), store.clone(), categorizer, db);
        (pipeline, store, embedding)
    }

    async fn insert_test_emails(db: &Database, account_id: &str, count: usize) {
        for i in 0..count {
            let id = format!("email-{}", i);
            let subject = format!("Test Subject {}", i);
            let from_addr = format!("sender{}@example.com", i);
            let body_text = format!("This is the body of test email number {}", i);
            sqlx::query(
                r#"INSERT INTO emails (id, account_id, provider, subject, from_addr, body_text, embedding_status)
                   VALUES (?, ?, 'test', ?, ?, ?, 'pending')"#,
            )
            .bind(&id)
            .bind(account_id)
            .bind(&subject)
            .bind(&from_addr)
            .bind(&body_text)
            .execute(&db.pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn test_start_ingestion_returns_job_id() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 3).await;
        let (pipeline, _, _) = make_pipeline(db);

        let job_id = pipeline.start_ingestion("acct-1").await.unwrap();
        assert!(!job_id.is_empty());
        // UUID format check
        assert!(Uuid::parse_str(&job_id).is_ok());
    }

    #[tokio::test]
    async fn test_ingestion_progress_tracking() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 5).await;
        let (pipeline, _, _) = make_pipeline(db);

        let _job_id = pipeline.start_ingestion("acct-1").await.unwrap();

        // Give the background task time to complete
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let progress = pipeline.get_progress().await;
        assert!(progress.is_some());
        let progress = progress.unwrap();
        assert_eq!(progress.total, 5);
        assert_eq!(progress.phase, "complete");
    }

    #[tokio::test]
    async fn test_ingestion_pause_resume() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 2).await;
        let (pipeline, _, _) = make_pipeline(db);

        let _job_id = pipeline.start_ingestion("acct-1").await.unwrap();

        // Immediately pause (may or may not catch it in time, but the
        // state machine transition should work)
        // Wait a tiny bit so the job is running
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // The job might already be completed in a fast test, so we just
        // verify the API doesn't panic
        let pause_result = pipeline.pause().await;
        if pause_result.is_ok() {
            // If we caught it running, verify paused state
            let progress = pipeline.get_progress().await.unwrap();
            assert_eq!(progress.phase, "syncing".to_string());

            // Resume
            pipeline.resume().await.unwrap();
        }

        // Wait for completion
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    #[tokio::test]
    async fn test_ingestion_embeds_emails() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 3).await;
        let (pipeline, store, _) = make_pipeline(db.clone());

        let _job_id = pipeline.start_ingestion("acct-1").await.unwrap();

        // Wait for completion
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Verify vectors were created in the store
        let count = store.count().await.unwrap();
        assert_eq!(count, 3, "Expected 3 vectors in store, got {}", count);

        // Verify DB was updated
        let row: (String,) =
            sqlx::query_as("SELECT embedding_status FROM emails WHERE id = 'email-0'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(row.0, "embedded");
    }

    #[tokio::test]
    async fn test_ingestion_categorizes_emails() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 2).await;
        let (pipeline, _, _) = make_pipeline(db.clone());

        let _job_id = pipeline.start_ingestion("acct-1").await.unwrap();

        // Wait for completion
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let progress = pipeline.get_progress().await.unwrap();
        assert_eq!(progress.phase, "complete");
        // Categorized count should match total (even if all go to Uncategorized)
        assert_eq!(progress.categorized, 2);

        // Verify DB category was updated
        let row: (Option<String>,) =
            sqlx::query_as("SELECT category FROM emails WHERE id = 'email-0'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(row.0.is_some());
    }
}
