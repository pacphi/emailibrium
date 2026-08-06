//! Ingestion pipeline for batch email processing (S2-03, S2-05).
//!
//! Implements the 6-stage per-email workflow: Sync -> Embed -> Categorize ->
//! Cluster -> Analyze -> Complete. Supports pause/resume and broadcasts
//! progress updates for SSE streaming.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Notify, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::entities::{emails, ingestion_checkpoints};
use crate::db::Database;

use super::categorizer::VectorCategorizer;
use super::embedding::EmbeddingPipeline;
use super::error::VectorError;
use super::store::VectorStoreBackend;
use super::thread;
use super::types::{EmbeddingStatus, VectorCollection, VectorDocument, VectorId};
use super::yaml_config::ClassificationConfig;

/// Row tuple for ingestion checkpoint queries (counters are INTEGER — `i32`).
type CheckpointRow = (String, String, String, i32, i32, i32, Option<String>);

/// Row tuple for full ingestion checkpoint queries (with account_id, status, etc.).
type FullCheckpointRow = (
    String,
    String,
    String,
    String,
    String,
    i32,
    i32,
    i32,
    Option<String>,
    Option<String>,
);

/// `datetime('now')` replacement for the checkpoint TEXT timestamp columns
/// (ADR-035 §2.5): the application owns the `YYYY-MM-DD HH:MM:SS` shape.
fn now_text() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

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
    Backfilling,
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
            IngestionPhase::Backfilling => write!(f, "backfilling"),
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

// ---------------------------------------------------------------------------
// Sync-phase progress broadcast (moved from `api::ingestion`)
// ---------------------------------------------------------------------------
//
// These live here rather than in the API layer so library-side consumers — the
// MCP tool registry among them — can read sync progress without depending on
// the binary crate. `api::ingestion` re-exports them under their original
// names, so existing call sites are unaffected.
//
// Deliberately distinct from `IngestionPhase` / `IngestionProgress` above:
// those describe a *pipeline job* and carry an already-stringified phase, while
// these describe the *sync broadcast* and keep the phase typed. Collapsing the
// two would change the wire format, since the JSON phase strings come from
// `Display` (lowercase), not from `Serialize`.

/// Phase of the ingestion pipeline, as broadcast to SSE subscribers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    Syncing,
    Embedding,
    Categorizing,
    Clustering,
    Analyzing,
    Complete,
}

impl std::fmt::Display for SyncPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncPhase::Syncing => write!(f, "syncing"),
            SyncPhase::Embedding => write!(f, "embedding"),
            SyncPhase::Categorizing => write!(f, "categorizing"),
            SyncPhase::Clustering => write!(f, "clustering"),
            SyncPhase::Analyzing => write!(f, "analyzing"),
            SyncPhase::Complete => write!(f, "complete"),
        }
    }
}

/// Real-time progress update for an ingestion job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    pub job_id: String,
    pub total: u64,
    pub processed: u64,
    pub embedded: u64,
    pub categorized: u64,
    pub failed: u64,
    pub phase: SyncPhase,
    pub eta_seconds: Option<u64>,
    pub emails_per_second: f64,
}

/// Holds the broadcast sender for SSE progress events.
///
/// Shared in `AppState` so ingestion workers can publish updates and
/// SSE endpoints can subscribe.
#[derive(Clone)]
pub struct IngestionBroadcast {
    sender: broadcast::Sender<SyncProgress>,
    /// Cache of the most recent progress snapshot so that polling endpoints
    /// (e.g. `/api/v1/ingestion/progress`) can return sync-phase progress
    /// even when the pipeline has not yet started a job.
    last_progress: std::sync::Arc<tokio::sync::RwLock<Option<SyncProgress>>>,
}

impl IngestionBroadcast {
    /// Create a new broadcast channel with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            last_progress: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Publish a progress event. Returns the number of active receivers.
    ///
    /// Also caches the progress so polling endpoints can retrieve it.
    pub fn send(
        &self,
        progress: SyncProgress,
    ) -> Result<usize, broadcast::error::SendError<SyncProgress>> {
        // Cache the latest snapshot for polling.
        let cached = self.last_progress.clone();
        let snapshot = progress.clone();
        tokio::spawn(async move {
            *cached.write().await = Some(snapshot);
        });
        self.sender.send(progress)
    }

    /// Subscribe to progress events.
    pub fn subscribe(&self) -> broadcast::Receiver<SyncProgress> {
        self.sender.subscribe()
    }

    /// Get the most recently broadcast progress snapshot.
    pub async fn last_progress(&self) -> Option<SyncProgress> {
        self.last_progress.read().await.clone()
    }

    /// Clear the cached progress (e.g. when a pipeline run completes).
    pub async fn clear_last_progress(&self) {
        *self.last_progress.write().await = None;
    }
}

impl Default for IngestionBroadcast {
    fn default() -> Self {
        Self::new(256)
    }
}

#[allow(dead_code)]
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
    /// Sender display name (ADR-029: richer embedding text).
    from_name: Option<String>,
    /// ISO-8601 received timestamp (ADR-029).
    received_at: Option<String>,
    /// Classification category (ADR-029).
    category: Option<String>,
    /// RFC 2822 Message-ID header for thread derivation.
    message_id: Option<String>,
    /// Provider-assigned thread/conversation ID (e.g. Gmail X-GM-THRID).
    provider_thread_id: Option<String>,
}

/// Persisted checkpoint for resume-from-failure (audit item #26).
#[derive(Debug, Clone, Serialize)]
pub struct IngestionCheckpoint {
    pub id: String,
    pub batch_id: String,
    pub account_id: String,
    pub stage: String,
    pub status: String,
    pub total: u64,
    pub processed: u64,
    pub failed: u64,
    pub last_processed_id: Option<String>,
    pub error_msg: Option<String>,
}

// ---------------------------------------------------------------------------
// Backfill progress
// ---------------------------------------------------------------------------

/// Shared state for tracking LLM backfill progress, accessible via polling.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BackfillProgress {
    pub active: bool,
    pub total: u64,
    pub categorized: u64,
    pub failed: u64,
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
    conn: DatabaseConnection,
    generative: Option<Arc<dyn crate::vectors::generative::GenerativeModel>>,
    cluster_engine: Option<Arc<crate::vectors::clustering::ClusterEngine>>,
    classification_config: ClassificationConfig,
    progress_tx: broadcast::Sender<IngestionProgress>,
    state: Arc<RwLock<IngestionState>>,
    resume_notify: Arc<Notify>,
    /// Tuning parameters from `config/tuning.yaml`.
    ingestion_tuning: super::yaml_config::IngestionTuning,
    error_recovery_tuning: super::yaml_config::ErrorRecoveryTuning,
    #[allow(dead_code)]
    backfill_state: Arc<RwLock<BackfillProgress>>,
}

impl IngestionPipeline {
    /// Create a new ingestion pipeline.
    pub fn new(
        embedding: Arc<EmbeddingPipeline>,
        store: Arc<dyn VectorStoreBackend>,
        categorizer: Arc<VectorCategorizer>,
        db: Arc<Database>,
        ingestion_tuning: super::yaml_config::IngestionTuning,
        error_recovery_tuning: super::yaml_config::ErrorRecoveryTuning,
    ) -> Self {
        let (progress_tx, _) = broadcast::channel(128);
        Self {
            embedding,
            store,
            categorizer,
            conn: db.sea_orm(),
            generative: None,
            cluster_engine: None,
            classification_config: ClassificationConfig::default(),
            progress_tx,
            state: Arc::new(RwLock::new(IngestionState {
                current_job: None,
                paused: false,
            })),
            resume_notify: Arc::new(Notify::new()),
            ingestion_tuning,
            error_recovery_tuning,
            backfill_state: Arc::new(RwLock::new(BackfillProgress::default())),
        }
    }

    /// Set the generative model for classification fallback (ADR-012).
    pub fn set_generative(
        &mut self,
        gen: Option<Arc<dyn crate::vectors::generative::GenerativeModel>>,
    ) {
        self.generative = gen;
    }

    /// Set the classification config for rule-based and LLM classification.
    pub fn set_classification_config(&mut self, config: ClassificationConfig) {
        self.classification_config = config;
    }

    /// Inject the cluster engine for the clustering phase of ingestion.
    pub fn set_cluster_engine(&mut self, engine: Arc<crate::vectors::clustering::ClusterEngine>) {
        self.cluster_engine = Some(engine);
    }

    /// Subscribe to progress updates (for SSE streaming).
    pub fn subscribe(&self) -> broadcast::Receiver<IngestionProgress> {
        self.progress_tx.subscribe()
    }

    /// Return a snapshot of the current LLM backfill progress.
    pub async fn get_backfill_progress(&self) -> BackfillProgress {
        self.backfill_state.read().await.clone()
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
            conn: self.conn.clone(),
            generative: self.generative.clone(),
            cluster_engine: self.cluster_engine.clone(),
            classification_config: self.classification_config.clone(),
            progress_tx: self.progress_tx.clone(),
            state: self.state.clone(),
            resume_notify: self.resume_notify.clone(),
            ingestion_tuning: self.ingestion_tuning.clone(),
            error_recovery_tuning: self.error_recovery_tuning.clone(),
            backfill_state: self.backfill_state.clone(),
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

    /// Resume a previously failed ingestion job from its last checkpoint
    /// (audit item #26).
    ///
    /// Looks up the most recent incomplete checkpoint for the given account
    /// and resumes processing from the last successfully processed email.
    pub async fn resume_from_checkpoint(
        &self,
        account_id: &str,
    ) -> Result<Option<String>, VectorError> {
        // Find the most recent incomplete checkpoint for this account.
        let checkpoint: Option<CheckpointRow> = ingestion_checkpoints::Entity::find()
            .select_only()
            .column(ingestion_checkpoints::Column::Id)
            .column(ingestion_checkpoints::Column::BatchId)
            .column(ingestion_checkpoints::Column::Stage)
            .column(ingestion_checkpoints::Column::Total)
            .column(ingestion_checkpoints::Column::Processed)
            .column(ingestion_checkpoints::Column::Failed)
            .column(ingestion_checkpoints::Column::LastProcessedId)
            .filter(ingestion_checkpoints::Column::AccountId.eq(account_id))
            .filter(ingestion_checkpoints::Column::Status.is_in(["running", "failed", "paused"]))
            .order_by_desc(ingestion_checkpoints::Column::CreatedAt)
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let Some((cp_id, batch_id, stage, total, processed, failed, last_id)) = checkpoint else {
            return Ok(None);
        };

        info!(
            batch_id = %batch_id,
            stage = %stage,
            processed = processed,
            total = total,
            last_id = ?last_id,
            "Resuming ingestion from checkpoint"
        );

        // Update checkpoint status to running.
        ingestion_checkpoints::Entity::update_many()
            .col_expr(
                ingestion_checkpoints::Column::Status,
                Expr::value("running"),
            )
            .col_expr(
                ingestion_checkpoints::Column::UpdatedAt,
                Expr::value(now_text()),
            )
            .filter(ingestion_checkpoints::Column::Id.eq(&cp_id))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        // Create a new job that picks up where we left off.
        let mut state = self.state.write().await;
        if let Some(ref job) = state.current_job {
            if job.status == JobStatus::Running || job.status == JobStatus::Paused {
                return Err(VectorError::IngestionError(
                    "An ingestion job is already in progress".to_string(),
                ));
            }
        }

        let job = IngestionJob {
            id: batch_id.clone(),
            account_id: account_id.to_string(),
            status: JobStatus::Running,
            total: total as u64,
            processed: processed as u64,
            embedded: processed as u64,
            categorized: 0,
            failed: failed as u64,
            phase: match stage.as_str() {
                "embedding" => IngestionPhase::Embedding,
                "categorizing" => IngestionPhase::Categorizing,
                "clustering" => IngestionPhase::Clustering,
                "analyzing" => IngestionPhase::Analyzing,
                "backfilling" => IngestionPhase::Backfilling,
                _ => IngestionPhase::Syncing,
            },
            started_at: Utc::now(),
            completed_at: None,
            emails_per_second: 0.0,
        };
        state.current_job = Some(job);
        state.paused = false;
        drop(state);

        // Spawn background task with the resume offset.
        let pipeline = IngestionPipelineHandle {
            embedding: self.embedding.clone(),
            store: self.store.clone(),
            categorizer: self.categorizer.clone(),
            conn: self.conn.clone(),
            generative: self.generative.clone(),
            cluster_engine: self.cluster_engine.clone(),
            classification_config: self.classification_config.clone(),
            progress_tx: self.progress_tx.clone(),
            state: self.state.clone(),
            resume_notify: self.resume_notify.clone(),
            ingestion_tuning: self.ingestion_tuning.clone(),
            error_recovery_tuning: self.error_recovery_tuning.clone(),
            backfill_state: self.backfill_state.clone(),
        };

        let jid = batch_id.clone();
        let aid = account_id.to_string();
        let resume_after = last_id;
        tokio::spawn(async move {
            pipeline
                .run_ingestion_with_resume(jid, aid, resume_after)
                .await;
        });

        Ok(Some(batch_id))
    }

    /// Get the latest checkpoint for an account.
    pub async fn get_checkpoint(
        &self,
        account_id: &str,
    ) -> Result<Option<IngestionCheckpoint>, VectorError> {
        let row: Option<FullCheckpointRow> = ingestion_checkpoints::Entity::find()
            .select_only()
            .column(ingestion_checkpoints::Column::Id)
            .column(ingestion_checkpoints::Column::BatchId)
            .column(ingestion_checkpoints::Column::AccountId)
            .column(ingestion_checkpoints::Column::Stage)
            .column(ingestion_checkpoints::Column::Status)
            .column(ingestion_checkpoints::Column::Total)
            .column(ingestion_checkpoints::Column::Processed)
            .column(ingestion_checkpoints::Column::Failed)
            .column(ingestion_checkpoints::Column::LastProcessedId)
            .column(ingestion_checkpoints::Column::ErrorMsg)
            .filter(ingestion_checkpoints::Column::AccountId.eq(account_id))
            .order_by_desc(ingestion_checkpoints::Column::CreatedAt)
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        Ok(row.map(|r| IngestionCheckpoint {
            id: r.0,
            batch_id: r.1,
            account_id: r.2,
            stage: r.3,
            status: r.4,
            total: r.5 as u64,
            processed: r.6 as u64,
            failed: r.7 as u64,
            last_processed_id: r.8,
            error_msg: r.9,
        }))
    }
}

/// Internal handle cloned into the background task. Keeps the same Arc
/// references so progress updates are visible to the outer pipeline.
struct IngestionPipelineHandle {
    embedding: Arc<EmbeddingPipeline>,
    store: Arc<dyn VectorStoreBackend>,
    categorizer: Arc<VectorCategorizer>,
    conn: DatabaseConnection,
    generative: Option<Arc<dyn crate::vectors::generative::GenerativeModel>>,
    cluster_engine: Option<Arc<crate::vectors::clustering::ClusterEngine>>,
    classification_config: ClassificationConfig,
    progress_tx: broadcast::Sender<IngestionProgress>,
    state: Arc<RwLock<IngestionState>>,
    resume_notify: Arc<Notify>,
    ingestion_tuning: super::yaml_config::IngestionTuning,
    error_recovery_tuning: super::yaml_config::ErrorRecoveryTuning,
    backfill_state: Arc<RwLock<BackfillProgress>>,
}

impl IngestionPipelineHandle {
    /// Run ingestion resuming after a specific email ID (checkpoint resume).
    async fn run_ingestion_with_resume(
        &self,
        job_id: String,
        account_id: String,
        resume_after_id: Option<String>,
    ) {
        info!(
            job_id = %job_id,
            account_id = %account_id,
            resume_after = ?resume_after_id,
            "Resuming ingestion from checkpoint"
        );

        // Fetch pending emails, filtering to those after the checkpoint.
        self.update_phase(IngestionPhase::Syncing).await;
        let all_emails = match self.fetch_pending_emails(&account_id).await {
            Ok(e) => e,
            Err(err) => {
                error!(job_id = %job_id, "Failed to fetch emails on resume: {err}");
                self.mark_failed().await;
                self.save_checkpoint(
                    &job_id,
                    &account_id,
                    "syncing",
                    "failed",
                    None,
                    Some(&err.to_string()),
                )
                .await;
                return;
            }
        };

        // Skip emails up to and including the last processed ID.
        let emails: Vec<PendingEmail> = if let Some(ref last_id) = resume_after_id {
            let mut found = false;
            all_emails
                .into_iter()
                .filter(|e| {
                    if found {
                        return true;
                    }
                    if e.id == *last_id {
                        found = true;
                    }
                    false
                })
                .collect()
        } else {
            all_emails
        };

        // Run the normal ingestion flow on the remaining emails.
        self.run_ingestion_inner(job_id, account_id, emails).await;
    }

    async fn run_ingestion(&self, job_id: String, account_id: String) {
        info!(job_id = %job_id, account_id = %account_id, "Starting ingestion");

        // Phase 1: Fetch pending emails
        self.update_phase(IngestionPhase::Syncing).await;
        let emails = match self.fetch_pending_emails(&account_id).await {
            Ok(e) => e,
            Err(err) => {
                error!(job_id = %job_id, "Failed to fetch emails: {err}");
                self.mark_failed().await;
                self.save_checkpoint(
                    &job_id,
                    &account_id,
                    "syncing",
                    "failed",
                    None,
                    Some(&err.to_string()),
                )
                .await;
                return;
            }
        };

        // Save initial checkpoint.
        self.save_checkpoint(&job_id, &account_id, "syncing", "running", None, None)
            .await;

        self.run_ingestion_inner(job_id, account_id, emails).await;
    }

    /// Shared ingestion logic used by both fresh starts and checkpoint resumes.
    async fn run_ingestion_inner(
        &self,
        job_id: String,
        account_id: String,
        emails: Vec<PendingEmail>,
    ) {
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
            self.save_checkpoint(&job_id, &account_id, "complete", "completed", None, None)
                .await;
            self.broadcast_progress(&job_id).await;
            return;
        }

        // Phase 2: Embed in batches
        self.update_phase(IngestionPhase::Embedding).await;
        self.broadcast_progress(&job_id).await;

        let batch_size = self.ingestion_tuning.embedding_batch_size;
        let start_time = std::time::Instant::now();
        let mut last_processed_id: Option<String> = None;

        // --- Pipelined embedding + insertion via tokio channel ---
        // The producer embeds batch N+1 while the consumer inserts batch N.
        let channel_buffer = self.ingestion_tuning.pipeline_channel_buffer;

        // Message type: Ok((docs, last_email_id, batch_len)) or Err((batch_len, error_msg))
        type EmbedMsg = Result<(Vec<VectorDocument>, Option<String>, u64), (u64, String)>;
        let (tx, mut rx): (mpsc::Sender<EmbedMsg>, mpsc::Receiver<EmbedMsg>) =
            mpsc::channel(channel_buffer);

        // Shared counter so the consumer can read how many the producer has embedded.
        let producer_embedded = Arc::new(AtomicU64::new(0));
        let producer_embedded_consumer = producer_embedded.clone();

        // Clone Arcs for the producer task.
        let embedding = self.embedding.clone();
        let conn = self.conn.clone();
        let state_ref = self.state.clone();
        let resume_notify = self.resume_notify.clone();
        let max_retries = self.error_recovery_tuning.max_retries;
        let retry_delay_ms = self.error_recovery_tuning.retry_delay_ms;
        let model_name = self.embedding_model_name().to_string();

        // Build owned batch data for the producer (avoids borrowing `emails`).
        struct BatchInput {
            texts: Vec<String>,
            email_ids: Vec<String>,
            subjects: Vec<String>,
            from_addrs: Vec<String>,
            message_ids: Vec<Option<String>>,
            provider_thread_ids: Vec<Option<String>>,
            last_email_id: Option<String>,
            batch_len: u64,
        }

        let batches: Vec<BatchInput> = emails
            .chunks(batch_size)
            .map(|chunk| {
                let texts: Vec<String> = chunk
                    .iter()
                    .map(|e| {
                        EmbeddingPipeline::prepare_email_text(
                            &e.subject,
                            &e.from_addr,
                            &e.body_text,
                            e.from_name.as_deref(),
                            e.received_at.as_deref(),
                            e.category.as_deref(),
                        )
                    })
                    .collect();
                BatchInput {
                    texts,
                    email_ids: chunk.iter().map(|e| e.id.clone()).collect(),
                    subjects: chunk.iter().map(|e| e.subject.clone()).collect(),
                    from_addrs: chunk.iter().map(|e| e.from_addr.clone()).collect(),
                    message_ids: chunk.iter().map(|e| e.message_id.clone()).collect(),
                    provider_thread_ids: chunk
                        .iter()
                        .map(|e| e.provider_thread_id.clone())
                        .collect(),
                    last_email_id: chunk.last().map(|e| e.id.clone()),
                    batch_len: chunk.len() as u64,
                }
            })
            .collect();

        // Producer task: embed each batch and send docs through the channel.
        let producer = tokio::spawn(async move {
            for batch in batches {
                // Respect pause/cancel state.
                {
                    let is_paused = state_ref.read().await.paused;
                    if is_paused {
                        info!("Embedding producer paused, waiting for resume signal");
                        resume_notify.notified().await;
                        info!("Embedding producer resumed");
                    }
                }

                // Retry embedding with error_recovery tuning parameters.
                let retry_delay = std::time::Duration::from_millis(retry_delay_ms);
                let mut embed_result = embedding.embed_batch(&batch.texts).await;
                for attempt in 1..=max_retries {
                    if embed_result.is_ok() {
                        break;
                    }
                    warn!(
                        attempt,
                        max_retries, "Embedding batch failed, retrying after delay"
                    );
                    tokio::time::sleep(retry_delay).await;
                    embed_result = embedding.embed_batch(&batch.texts).await;
                }

                match embed_result {
                    Ok(vectors) => {
                        let mut docs = Vec::with_capacity(vectors.len());
                        for (i, vector) in vectors.into_iter().enumerate() {
                            let vector_id = VectorId::new();
                            let mut metadata = std::collections::HashMap::new();
                            metadata.insert("subject".to_string(), batch.subjects[i].clone());
                            metadata.insert("from_addr".to_string(), batch.from_addrs[i].clone());

                            // ADR-029 Phase D: Derive thread_key for thread-aware search.
                            // Note: `references` and `in_reply_to` are not yet stored in
                            // the emails table — a future migration should persist raw
                            // RFC 2822 headers to enable full thread derivation.
                            let thread_key = {
                                let key = thread::derive_thread_key(
                                    batch.message_ids[i].as_deref(),
                                    None, // references — needs future migration
                                    None, // in_reply_to — needs future migration
                                    batch.provider_thread_ids[i].as_deref(),
                                );
                                if key.is_empty() {
                                    batch.email_ids[i].clone()
                                } else {
                                    key
                                }
                            };
                            metadata.insert("thread_key".to_string(), thread_key.clone());

                            // Persist thread_key to the emails table.
                            if let Err(err) = update_thread_key_standalone(
                                &conn,
                                &batch.email_ids[i],
                                &thread_key,
                            )
                            .await
                            {
                                warn!(email_id = %batch.email_ids[i], "Failed to set thread_key: {err}");
                            }

                            let doc = VectorDocument {
                                id: vector_id.clone(),
                                email_id: batch.email_ids[i].clone(),
                                vector,
                                metadata,
                                collection: VectorCollection::EmailText,
                                created_at: Utc::now(),
                            };
                            docs.push(doc);

                            // Update DB embedding status in the producer.
                            if let Err(err) = update_embedding_status_standalone(
                                &conn,
                                &batch.email_ids[i],
                                &vector_id.to_string(),
                                &model_name,
                            )
                            .await
                            {
                                warn!(email_id = %batch.email_ids[i], "Failed to update embedding status: {err}");
                            }
                        }

                        producer_embedded.fetch_add(batch.batch_len, Ordering::Relaxed);

                        if tx
                            .send(Ok((docs, batch.last_email_id, batch.batch_len)))
                            .await
                            .is_err()
                        {
                            warn!("Consumer dropped, stopping embedding producer");
                            break;
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if tx.send(Err((batch.batch_len, msg))).await.is_err() {
                            warn!("Consumer dropped, stopping embedding producer");
                            break;
                        }
                    }
                }
            }
            // tx is dropped here, closing the channel.
        });

        // Consumer: receive embedded batches and insert into the vector store.
        while let Some(msg) = rx.recv().await {
            match msg {
                Ok((docs, batch_last_id, batch_len)) => {
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
                                job.failed += batch_len;
                                job.processed += batch_len;
                            }
                        }
                    }

                    // Track the last successfully processed email for checkpointing.
                    if batch_last_id.is_some() {
                        last_processed_id = batch_last_id;
                    }
                }
                Err((batch_len, err_msg)) => {
                    warn!("Batch embedding failed: {err_msg}");
                    let mut state = self.state.write().await;
                    if let Some(ref mut job) = state.current_job {
                        job.failed += batch_len;
                        job.processed += batch_len;
                    }

                    // Save checkpoint on batch failure so we can resume.
                    self.save_checkpoint(
                        &job_id,
                        &account_id,
                        "embedding",
                        "failed",
                        last_processed_id.as_deref(),
                        Some(&err_msg),
                    )
                    .await;
                }
            }

            // Periodic checkpoint save (every batch).
            self.save_checkpoint(
                &job_id,
                &account_id,
                "embedding",
                "running",
                last_processed_id.as_deref(),
                None,
            )
            .await;
            self.broadcast_progress(&job_id).await;
        }

        // Ensure the producer task has completed.
        if let Err(err) = producer.await {
            error!("Embedding producer task panicked: {err}");
        }

        // Log pipeline stats.
        let total_embedded = producer_embedded_consumer.load(Ordering::Relaxed);
        debug!(
            total_embedded,
            channel_buffer, "Pipelined embedding phase complete"
        );

        // Phase 3: Categorize
        self.update_phase(IngestionPhase::Categorizing).await;
        self.save_checkpoint(
            &job_id,
            &account_id,
            "categorizing",
            "running",
            last_processed_id.as_deref(),
            None,
        )
        .await;
        self.broadcast_progress(&job_id).await;

        // Determine whether to use onboarding mode (rules-only, skip LLM).
        // Onboarding mode activates when the config flag is set AND this is a
        // bulk sync (email count exceeds the embedding batch size threshold).
        let use_onboarding_mode = self.ingestion_tuning.onboarding_mode
            && total > self.ingestion_tuning.embedding_batch_size as u64;

        if use_onboarding_mode {
            info!(
                job_id = %job_id,
                total,
                "Onboarding mode: using rules-only classification (LLM backfill will run async)"
            );
        }

        let concurrency = self.ingestion_tuning.backfill_concurrency;
        let categorizer = self.categorizer.clone();
        let generative = self.generative.clone();
        let classification_config = self.classification_config.clone();

        // Pre-extract owned data from emails to avoid lifetime issues with buffer_unordered.
        let email_inputs: Vec<(String, String, String)> = emails
            .iter()
            .map(|e| {
                let text = EmbeddingPipeline::prepare_email_text(
                    &e.subject,
                    &e.from_addr,
                    &e.body_text,
                    e.from_name.as_deref(),
                    e.received_at.as_deref(),
                    e.category.as_deref(),
                );
                (e.id.clone(), e.from_addr.clone(), text)
            })
            .collect();

        let onboarding = use_onboarding_mode;
        let results: Vec<(String, Result<super::types::CategoryResult, VectorError>)> =
            stream::iter(email_inputs)
                .map(|(email_id, from_addr, text)| {
                    let categorizer = categorizer.clone();
                    let generative = generative.clone();
                    let classification_config = classification_config.clone();
                    async move {
                        let result = if onboarding {
                            categorizer
                                .categorize_rules_only(&text, &from_addr, &classification_config)
                                .await
                        } else {
                            let gen_ref = generative.as_deref();
                            categorizer
                                .categorize_with_fallback_config(
                                    &text,
                                    &from_addr,
                                    gen_ref,
                                    &classification_config,
                                )
                                .await
                        };
                        (email_id, result)
                    }
                })
                .buffer_unordered(concurrency)
                .collect()
                .await;

        // Apply DB updates and progress tracking after collecting all results.
        for (email_id, result) in &results {
            match result {
                Ok(cat_result) => {
                    if let Err(err) = self
                        .update_category(
                            email_id,
                            &cat_result.category.to_string(),
                            cat_result.confidence,
                            &cat_result.method,
                        )
                        .await
                    {
                        warn!(email_id = %email_id, "Failed to update category: {err}");
                    }
                    let mut state = self.state.write().await;
                    if let Some(ref mut job) = state.current_job {
                        job.categorized += 1;
                    }
                }
                Err(err) => {
                    debug!(email_id = %email_id, "Categorization failed: {err}");
                }
            }
        }

        self.broadcast_progress(&job_id).await;

        // Phase 4: Clustering (ADR-009)
        // Recluster when enough new emails have been embedded since the last
        // clustering run (checked via total vector count delta), OR when this
        // is a large batch (e.g., initial onboarding or full re-embed).
        self.update_phase(IngestionPhase::Clustering).await;
        self.broadcast_progress(&job_id).await;
        if let Some(ref engine) = self.cluster_engine {
            let current_count = self.store.count().await.unwrap_or(0);
            let threshold = self.ingestion_tuning.min_cluster_emails as u64;
            let recluster_threshold = engine.clustering_tuning.recluster_threshold as u64;

            let should_cluster = if current_count < threshold {
                // Not enough total vectors for meaningful clusters.
                false
            } else {
                // Check if enough new emails accumulated since last clustering.
                engine
                    .should_recluster(current_count, recluster_threshold)
                    .await
            };

            if should_cluster {
                match engine.full_recluster().await {
                    Ok(report) => {
                        info!(
                            job_id = %job_id,
                            clusters = report.cluster_count,
                            new = report.new_clusters,
                            merged = report.merged_clusters,
                            "Clustering complete"
                        );
                    }
                    Err(e) => {
                        warn!(job_id = %job_id, error = %e, "Clustering failed, continuing");
                    }
                }
            } else {
                debug!(job_id = %job_id, vectors = current_count, recluster_threshold, "Clustering phase: skipped (below recluster threshold)");
            }
        } else {
            debug!(job_id = %job_id, "Clustering phase: skipped (no cluster engine)");
        }

        // Phase 5: Analyzing
        self.update_phase(IngestionPhase::Analyzing).await;
        self.broadcast_progress(&job_id).await;
        // Insight analysis (subscription detection, temporal patterns) is triggered
        // via the REST API after sync completes, not inline during ingestion.
        // See GET /api/v1/insights/* endpoints.

        // Complete
        self.update_phase(IngestionPhase::Complete).await;
        self.mark_completed().await;
        self.save_checkpoint(
            &job_id,
            &account_id,
            "complete",
            "completed",
            last_processed_id.as_deref(),
            None,
        )
        .await;
        self.broadcast_progress(&job_id).await;

        info!(job_id = %job_id, "Ingestion complete");

        // Spawn background backfill for pending_backfill emails (onboarding mode).
        if use_onboarding_mode {
            self.spawn_backfill_task(job_id, account_id);
        }
    }

    /// Spawn a fire-and-forget async task that processes emails marked with
    /// `category_method = 'pending_backfill'` through the full LLM classification
    /// pipeline. Processes in configurable batches with throttling to avoid
    /// overloading the LLM provider.
    fn spawn_backfill_task(&self, job_id: String, account_id: String) {
        let conn = self.conn.clone();
        let categorizer = self.categorizer.clone();
        let generative = self.generative.clone();
        let classification_config = self.classification_config.clone();
        let progress_tx = self.progress_tx.clone();
        let batch_size = self.ingestion_tuning.backfill_batch_size;
        let concurrency = self.ingestion_tuning.backfill_concurrency;
        let delay_ms = self.ingestion_tuning.backfill_delay_between_ms;
        let backfill_state = self.backfill_state.clone();

        tokio::spawn(async move {
            info!(
                job_id = %job_id,
                account_id = %account_id,
                batch_size,
                concurrency,
                "Starting background LLM backfill for pending_backfill emails"
            );

            // Count total pending_backfill emails for progress tracking.
            // Best-effort like the pre-port query: errors count as zero.
            let total_count: u64 = emails::Entity::find()
                .select_only()
                .expr_as(Expr::cust("COUNT(*)"), "cnt")
                .filter(emails::Column::AccountId.eq(account_id.as_str()))
                .filter(emails::Column::CategoryMethod.eq("pending_backfill"))
                .into_tuple::<(i64,)>()
                .one(&conn)
                .await
                .ok()
                .flatten()
                .map(|(c,)| c)
                .unwrap_or(0) as u64;

            {
                let mut bs = backfill_state.write().await;
                *bs = BackfillProgress {
                    active: true,
                    total: total_count,
                    categorized: 0,
                    failed: 0,
                };
            }

            let mut total_backfilled: u64 = 0;
            let mut total_failed: u64 = 0;

            loop {
                // Fetch next batch of pending_backfill emails.
                type BackfillRow = (String, String, String, Option<String>);
                let rows: Result<Vec<BackfillRow>, _> = emails::Entity::find()
                    .select_only()
                    .column(emails::Column::Id)
                    .column(emails::Column::Subject)
                    .column(emails::Column::FromAddr)
                    .column(emails::Column::BodyText)
                    .filter(emails::Column::AccountId.eq(account_id.as_str()))
                    .filter(emails::Column::CategoryMethod.eq("pending_backfill"))
                    .order_by_desc(emails::Column::ReceivedAt)
                    .limit(batch_size as u64)
                    .into_tuple()
                    .all(&conn)
                    .await;

                let batch = match rows {
                    Ok(b) => b,
                    Err(err) => {
                        error!(job_id = %job_id, "Backfill DB query failed: {err}");
                        break;
                    }
                };

                if batch.is_empty() {
                    info!(
                        job_id = %job_id,
                        total_backfilled,
                        total_failed,
                        "Backfill complete: no more pending_backfill emails"
                    );
                    break;
                }

                let batch_len = batch.len();
                debug!(
                    job_id = %job_id,
                    batch_size = batch_len,
                    "Processing backfill batch"
                );

                // Build input tuples for parallel processing.
                let email_inputs: Vec<(String, String, String)> = batch
                    .into_iter()
                    .map(|(id, subject, from_addr, body_text)| {
                        let text = EmbeddingPipeline::prepare_email_text(
                            &subject,
                            &from_addr,
                            &body_text.unwrap_or_default(),
                            None,
                            None,
                            None,
                        );
                        (id, from_addr, text)
                    })
                    .collect();

                let cat = categorizer.clone();
                let gen = generative.clone();
                let cfg = classification_config.clone();

                let results: Vec<(String, Result<super::types::CategoryResult, VectorError>)> =
                    stream::iter(email_inputs)
                        .map(|(email_id, from_addr, text)| {
                            let cat = cat.clone();
                            let gen = gen.clone();
                            let cfg = cfg.clone();
                            async move {
                                let gen_ref = gen.as_deref();
                                let result = cat
                                    .categorize_with_fallback_config(
                                        &text, &from_addr, gen_ref, &cfg,
                                    )
                                    .await;
                                (email_id, result)
                            }
                        })
                        .buffer_unordered(concurrency)
                        .collect()
                        .await;

                // Apply DB updates.
                for (email_id, result) in &results {
                    match result {
                        Ok(cat_result) => {
                            let update_result = emails::Entity::update_many()
                                .col_expr(
                                    emails::Column::Category,
                                    Expr::value(cat_result.category.to_string()),
                                )
                                .col_expr(
                                    emails::Column::CategoryConfidence,
                                    Expr::value(cat_result.confidence),
                                )
                                .col_expr(
                                    emails::Column::CategoryMethod,
                                    Expr::value(cat_result.method.as_str()),
                                )
                                .filter(emails::Column::Id.eq(email_id.as_str()))
                                .exec(&conn)
                                .await;

                            if let Err(err) = update_result {
                                warn!(email_id = %email_id, "Backfill DB update failed: {err}");
                                total_failed += 1;
                            } else {
                                total_backfilled += 1;
                            }
                        }
                        Err(err) => {
                            debug!(email_id = %email_id, "Backfill categorization failed: {err}");
                            total_failed += 1;
                        }
                    }
                }

                // Update shared backfill state after each batch.
                {
                    let mut bs = backfill_state.write().await;
                    bs.categorized = total_backfilled;
                    bs.failed = total_failed;
                }

                // Broadcast backfill progress via SSE.
                let progress = IngestionProgress {
                    job_id: job_id.clone(),
                    total: total_count,
                    processed: total_backfilled + total_failed,
                    embedded: 0,
                    categorized: total_backfilled,
                    failed: total_failed,
                    phase: "backfilling".to_string(),
                    eta_seconds: None,
                    emails_per_second: 0.0,
                };
                let _ = progress_tx.send(progress);

                // Throttle between batches.
                if delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }

            // Mark backfill as complete.
            {
                let mut bs = backfill_state.write().await;
                bs.active = false;
                bs.categorized = total_backfilled;
                bs.failed = total_failed;
            }

            info!(
                job_id = %job_id,
                total_backfilled,
                total_failed,
                "Background LLM backfill finished"
            );
        });
    }

    async fn fetch_pending_emails(
        &self,
        account_id: &str,
    ) -> Result<Vec<PendingEmail>, VectorError> {
        type EmailRow = (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            chrono::NaiveDateTime,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        // `is_trash`/`is_spam` are NOT NULL DEFAULT 0 (migration 016), so the
        // pre-port `COALESCE(x, 0) = 0` is exactly `x = 0`.
        let rows: Vec<EmailRow> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::Subject)
            .column(emails::Column::FromAddr)
            .column(emails::Column::BodyText)
            .column(emails::Column::FromName)
            .column(emails::Column::ReceivedAt)
            .column(emails::Column::Category)
            .column(emails::Column::MessageId)
            .column(emails::Column::ThreadId)
            .filter(emails::Column::AccountId.eq(account_id))
            .filter(emails::Column::EmbeddingStatus.is_in(["pending", "stale"]))
            .filter(emails::Column::IsTrash.eq(0))
            .filter(emails::Column::IsSpam.eq(0))
            .filter(emails::Column::DeletedAt.is_null())
            .order_by_desc(emails::Column::ReceivedAt)
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        Ok(rows
            .into_iter()
            .map(|r| PendingEmail {
                id: r.0,
                subject: r.1,
                from_addr: r.2,
                body_text: r.3.unwrap_or_default(),
                from_name: r.4,
                // Embedding-text input, not a wire format: RFC3339 output is
                // byte-identical for legacy RFC3339 rows and normalizes naive
                // DDL-default rows (same policy as `tools::readonly::emails`).
                received_at: Some(r.5.and_utc().to_rfc3339()),
                category: r.6,
                message_id: r.7,
                provider_thread_id: r.8,
            })
            .collect())
    }

    async fn update_category(
        &self,
        email_id: &str,
        category: &str,
        confidence: f32,
        method: &str,
    ) -> Result<(), VectorError> {
        emails::Entity::update_many()
            .col_expr(emails::Column::Category, Expr::value(category))
            .col_expr(emails::Column::CategoryConfidence, Expr::value(confidence))
            .col_expr(emails::Column::CategoryMethod, Expr::value(method))
            .filter(emails::Column::Id.eq(email_id))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?;
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

    /// Persist checkpoint state to the database for resume-from-failure (item #26).
    async fn save_checkpoint(
        &self,
        job_id: &str,
        account_id: &str,
        stage: &str,
        status: &str,
        last_processed_id: Option<&str>,
        error_msg: Option<&str>,
    ) {
        let (total, processed, failed) = {
            let state = self.state.read().await;
            match &state.current_job {
                Some(job) => (job.total as i32, job.processed as i32, job.failed as i32),
                None => (0, 0, 0),
            }
        };

        let checkpoint_id = format!("{}-{}", job_id, stage);
        // Upsert: on conflict every mutable column takes the excluded value,
        // including `updated_at` — the excluded row carries the fresh
        // timestamp, so `updated_at = excluded.updated_at` is observably the
        // pre-port `updated_at = datetime('now')`.
        let result = ingestion_checkpoints::Entity::insert(ingestion_checkpoints::ActiveModel {
            id: Set(checkpoint_id),
            batch_id: Set(job_id.to_owned()),
            account_id: Set(account_id.to_owned()),
            stage: Set(stage.to_owned()),
            status: Set(status.to_owned()),
            total: Set(total),
            processed: Set(processed),
            failed: Set(failed),
            last_processed_id: Set(last_processed_id.map(str::to_owned)),
            error_msg: Set(error_msg.map(str::to_owned)),
            updated_at: Set(now_text()),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(ingestion_checkpoints::Column::Id)
                .update_columns([
                    ingestion_checkpoints::Column::Stage,
                    ingestion_checkpoints::Column::Status,
                    ingestion_checkpoints::Column::Total,
                    ingestion_checkpoints::Column::Processed,
                    ingestion_checkpoints::Column::Failed,
                    ingestion_checkpoints::Column::LastProcessedId,
                    ingestion_checkpoints::Column::ErrorMsg,
                    ingestion_checkpoints::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await;

        if let Err(err) = result {
            // Checkpoint save failures are non-fatal -- log and continue.
            warn!(job_id = %job_id, error = %err, "Failed to save ingestion checkpoint");
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

/// Standalone helper for updating embedding status from a spawned task.
///
/// Takes a `DatabaseConnection` directly so it can be called from the
/// producer task without requiring `&self`. `embedded_at` is a real
/// TIMESTAMP column, so the pre-port RFC3339 string bind becomes a naive-UTC
/// bind (closes pl-timestamp-write-tz for this write site).
async fn update_embedding_status_standalone(
    conn: &DatabaseConnection,
    email_id: &str,
    vector_id: &str,
    model: &str,
) -> Result<(), VectorError> {
    emails::Entity::update_many()
        .col_expr(emails::Column::EmbeddingStatus, Expr::value("embedded"))
        .col_expr(
            emails::Column::EmbeddedAt,
            Expr::value(Utc::now().naive_utc()),
        )
        .col_expr(emails::Column::VectorId, Expr::value(vector_id))
        .col_expr(emails::Column::EmbeddingModel, Expr::value(model))
        .filter(emails::Column::Id.eq(email_id))
        .exec(conn)
        .await
        .map_err(VectorError::Db)?;
    Ok(())
}

/// Persist `thread_key` for an email row (ADR-029 Phase D).
///
/// Standalone helper (same pattern as `update_embedding_status_standalone`) so
/// it can be called from the producer task without borrowing `self`.
async fn update_thread_key_standalone(
    conn: &DatabaseConnection,
    email_id: &str,
    thread_key: &str,
) -> Result<(), VectorError> {
    emails::Entity::update_many()
        .col_expr(emails::Column::ThreadKey, Expr::value(thread_key))
        .filter(emails::Column::Id.eq(email_id))
        .exec(conn)
        .await
        .map_err(VectorError::Db)?;
    Ok(())
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
    use sea_orm::ConnectionTrait;

    use super::*;
    use crate::vectors::categorizer::VectorCategorizer;
    use crate::vectors::config::EmbeddingConfig;
    use crate::vectors::embedding::EmbeddingPipeline;
    use crate::vectors::store::InMemoryVectorStore;

    async fn test_db() -> Database {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
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
        let pipeline = IngestionPipeline::new(
            embedding.clone(),
            store.clone(),
            categorizer,
            db,
            crate::vectors::yaml_config::IngestionTuning::default(),
            crate::vectors::yaml_config::ErrorRecoveryTuning::default(),
        );
        (pipeline, store, embedding)
    }

    async fn insert_test_emails(db: &Database, account_id: &str, count: usize) {
        for i in 0..count {
            // Account-prefixed ids so multi-account tests can seed bystanders.
            let id = format!("{account_id}-email-{i}");
            let subject = format!("Test Subject {}", i);
            let from_addr = format!("sender{}@example.com", i);
            let body_text = format!("This is the body of test email number {}", i);
            db.sea_orm()
                .execute_raw(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    r#"INSERT INTO emails (id, account_id, provider, subject, from_addr, body_text, embedding_status)
                   VALUES (?, ?, 'test', ?, ?, ?, 'pending')"#,
                    [
                        id.as_str().into(),
                        account_id.into(),
                        subject.as_str().into(),
                        from_addr.as_str().into(),
                        body_text.as_str().into(),
                    ],
                ))
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
            // If we caught it running, verify paused state.
            // The phase depends on how far the job progressed before being
            // paused — it could be any in-progress phase (syncing,
            // embedding, categorizing, etc.).
            let progress = pipeline.get_progress().await.unwrap();
            assert!(
                progress.phase != "complete",
                "Expected an in-progress phase after pause, got: {}",
                progress.phase,
            );

            // Resume
            pipeline.resume().await.unwrap();
        }

        // Wait for completion
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    /// Two-owner scoping pin: ingestion for one account must not consume the
    /// bystander account's pending emails, and checkpoints stay per-account.
    #[tokio::test]
    async fn test_ingestion_scopes_to_the_target_account() {
        let db = Arc::new(test_db().await);
        insert_test_emails(&db, "acct-1", 2).await;
        insert_test_emails(&db, "acct-2", 3).await;
        let (pipeline, store, _) = make_pipeline(db.clone());

        let _job_id = pipeline.start_ingestion("acct-1").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Only acct-1's two emails were embedded into the store.
        assert_eq!(
            store.count().await.unwrap(),
            2,
            "ingestion of acct-1 must not touch acct-2's pending emails"
        );

        // The bystander's rows are still pending in the database.
        let pending_b: Vec<(String,)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .filter(emails::Column::AccountId.eq("acct-2"))
            .filter(emails::Column::EmbeddingStatus.eq("pending"))
            .into_tuple()
            .all(&db.sea_orm())
            .await
            .unwrap();
        assert_eq!(pending_b.len(), 3, "acct-2's emails stay pending");

        // Checkpoints are per-account: acct-1 has one, acct-2 has none.
        assert!(pipeline.get_checkpoint("acct-1").await.unwrap().is_some());
        assert!(pipeline.get_checkpoint("acct-2").await.unwrap().is_none());
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
        let row: Option<(Option<String>,)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::EmbeddingStatus)
            .filter(emails::Column::Id.eq("acct-1-email-0"))
            .into_tuple()
            .one(&db.sea_orm())
            .await
            .unwrap();
        assert_eq!(row.unwrap().0.as_deref(), Some("embedded"));
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
        let row: Option<(Option<String>,)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Category)
            .filter(emails::Column::Id.eq("acct-1-email-0"))
            .into_tuple()
            .one(&db.sea_orm())
            .await
            .unwrap();
        assert!(row.unwrap().0.is_some());
    }
}
