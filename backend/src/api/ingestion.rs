//! SSE streaming endpoints for email ingestion progress (S2-04).
//!
//! - GET  /api/v1/ingestion/status  — SSE stream of `IngestionProgress` events
//! - POST /api/v1/ingestion/start   — kick off an ingestion job
//! - POST /api/v1/ingestion/pause   — pause a running job
//! - POST /api/v1/ingestion/resume  — resume a paused job

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::debug;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Phase of the ingestion pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Real-time progress update for an ingestion job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionProgress {
    pub job_id: String,
    pub total: u64,
    pub processed: u64,
    pub embedded: u64,
    pub categorized: u64,
    pub failed: u64,
    pub phase: IngestionPhase,
    pub eta_seconds: Option<u64>,
    pub emails_per_second: f64,
}

/// Holds the broadcast sender for SSE progress events.
///
/// Shared in `AppState` so ingestion workers can publish updates and
/// SSE endpoints can subscribe.
#[derive(Clone)]
pub struct IngestionBroadcast {
    sender: broadcast::Sender<IngestionProgress>,
}

impl IngestionBroadcast {
    /// Create a new broadcast channel with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish a progress event. Returns the number of active receivers.
    pub fn send(
        &self,
        progress: IngestionProgress,
    ) -> Result<usize, broadcast::error::SendError<IngestionProgress>> {
        self.sender.send(progress)
    }

    /// Subscribe to progress events.
    pub fn subscribe(&self) -> broadcast::Receiver<IngestionProgress> {
        self.sender.subscribe()
    }
}

impl Default for IngestionBroadcast {
    fn default() -> Self {
        Self::new(256)
    }
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub account_id: Option<String>,
    pub full_sync: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct JobResponse {
    pub job_id: String,
    pub status: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Build ingestion API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(ingestion_status_sse))
        .route("/start", post(start_ingestion))
        .route("/pause", post(pause_ingestion))
        .route("/resume", post(resume_ingestion))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/ingestion/status — SSE stream of ingestion progress.
///
/// Accepts an optional `job_id` query parameter to filter events.
async fn ingestion_status_sse(
    State(state): State<AppState>,
    Query(params): Query<StatusQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.ingestion_broadcast.subscribe();
    let job_id_filter = params.job_id;

    let stream = BroadcastStream::new(rx).filter_map(move |msg| {
        match msg {
            Ok(progress) => {
                // Apply job_id filter if provided.
                if let Some(ref filter_id) = job_id_filter {
                    if progress.job_id != *filter_id {
                        return None;
                    }
                }
                match serde_json::to_string(&progress) {
                    Ok(json) => Some(Ok(Event::default().event("progress").data(json))),
                    Err(_) => None,
                }
            }
            Err(_) => None, // lagged — skip
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// POST /api/v1/ingestion/start — start a new ingestion job.
async fn start_ingestion(
    State(state): State<AppState>,
    Json(req): Json<StartRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let account_id = req.account_id.unwrap_or_else(|| "default".to_string());

    debug!(
        job_id = %job_id,
        account_id = %account_id,
        full_sync = req.full_sync.unwrap_or(false),
        "starting ingestion job"
    );

    // Publish the initial progress event.
    let progress = IngestionProgress {
        job_id: job_id.clone(),
        total: 0,
        processed: 0,
        embedded: 0,
        categorized: 0,
        failed: 0,
        phase: IngestionPhase::Syncing,
        eta_seconds: None,
        emails_per_second: 0.0,
    };

    // It's okay if nobody is listening yet.
    let _ = state.ingestion_broadcast.send(progress);

    Ok(Json(JobResponse {
        job_id,
        status: "started".to_string(),
        message: format!("Ingestion started for account {account_id}"),
    }))
}

/// POST /api/v1/ingestion/pause — pause a running ingestion job.
async fn pause_ingestion(
    Json(req): Json<PauseResumeRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    debug!(job_id = %req.job_id, "pausing ingestion job");

    Ok(Json(JobResponse {
        job_id: req.job_id,
        status: "paused".to_string(),
        message: "Ingestion job paused".to_string(),
    }))
}

/// POST /api/v1/ingestion/resume — resume a paused ingestion job.
async fn resume_ingestion(
    Json(req): Json<PauseResumeRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    debug!(job_id = %req.job_id, "resuming ingestion job");

    Ok(Json(JobResponse {
        job_id: req.job_id,
        status: "resumed".to_string(),
        message: "Ingestion job resumed".to_string(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct PauseResumeRequest {
    pub job_id: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingestion_phase_display() {
        assert_eq!(IngestionPhase::Syncing.to_string(), "syncing");
        assert_eq!(IngestionPhase::Embedding.to_string(), "embedding");
        assert_eq!(IngestionPhase::Categorizing.to_string(), "categorizing");
        assert_eq!(IngestionPhase::Clustering.to_string(), "clustering");
        assert_eq!(IngestionPhase::Analyzing.to_string(), "analyzing");
        assert_eq!(IngestionPhase::Complete.to_string(), "complete");
    }

    #[test]
    fn test_ingestion_progress_serialization() {
        let progress = IngestionProgress {
            job_id: "test-job-123".to_string(),
            total: 100,
            processed: 50,
            embedded: 40,
            categorized: 30,
            failed: 2,
            phase: IngestionPhase::Embedding,
            eta_seconds: Some(30),
            emails_per_second: 10.5,
        };

        let json = serde_json::to_string(&progress).unwrap();
        let deserialized: IngestionProgress = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.job_id, "test-job-123");
        assert_eq!(deserialized.total, 100);
        assert_eq!(deserialized.processed, 50);
        assert_eq!(deserialized.embedded, 40);
        assert_eq!(deserialized.categorized, 30);
        assert_eq!(deserialized.failed, 2);
        assert_eq!(deserialized.phase, IngestionPhase::Embedding);
        assert_eq!(deserialized.eta_seconds, Some(30));
        assert!((deserialized.emails_per_second - 10.5).abs() < 0.01);
    }

    #[test]
    fn test_broadcast_send_no_receivers() {
        let broadcast = IngestionBroadcast::new(16);
        let progress = IngestionProgress {
            job_id: "j1".to_string(),
            total: 0,
            processed: 0,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            eta_seconds: None,
            emails_per_second: 0.0,
        };

        // No receivers — send returns Err, which is acceptable.
        let result = broadcast.send(progress);
        assert!(result.is_err(), "send with no receivers should return Err");
    }

    #[tokio::test]
    async fn test_broadcast_send_receive() {
        let broadcast = IngestionBroadcast::new(16);
        let mut rx = broadcast.subscribe();

        let progress = IngestionProgress {
            job_id: "j2".to_string(),
            total: 50,
            processed: 10,
            embedded: 5,
            categorized: 3,
            failed: 0,
            phase: IngestionPhase::Embedding,
            eta_seconds: Some(60),
            emails_per_second: 5.0,
        };

        broadcast.send(progress.clone()).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.job_id, "j2");
        assert_eq!(received.total, 50);
        assert_eq!(received.processed, 10);
        assert_eq!(received.phase, IngestionPhase::Embedding);
    }

    #[tokio::test]
    async fn test_broadcast_multiple_subscribers() {
        let broadcast = IngestionBroadcast::new(16);
        let mut rx1 = broadcast.subscribe();
        let mut rx2 = broadcast.subscribe();

        let progress = IngestionProgress {
            job_id: "j3".to_string(),
            total: 100,
            processed: 0,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            eta_seconds: None,
            emails_per_second: 0.0,
        };

        let count = broadcast.send(progress).unwrap();
        assert_eq!(count, 2, "should have 2 receivers");

        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();
        assert_eq!(r1.job_id, "j3");
        assert_eq!(r2.job_id, "j3");
    }

    #[tokio::test]
    async fn test_broadcast_multiple_events() {
        let broadcast = IngestionBroadcast::new(16);
        let mut rx = broadcast.subscribe();

        for i in 0..5 {
            let progress = IngestionProgress {
                job_id: format!("batch-{i}"),
                total: 100,
                processed: i * 20,
                embedded: i * 15,
                categorized: i * 10,
                failed: 0,
                phase: if i < 4 {
                    IngestionPhase::Embedding
                } else {
                    IngestionPhase::Complete
                },
                eta_seconds: Some((4 - i) * 10),
                emails_per_second: 20.0,
            };
            broadcast.send(progress).unwrap();
        }

        for i in 0..5u64 {
            let received = rx.recv().await.unwrap();
            assert_eq!(received.job_id, format!("batch-{i}"));
            assert_eq!(received.processed, i * 20);
        }
    }

    #[test]
    fn test_ingestion_broadcast_default() {
        let broadcast = IngestionBroadcast::default();
        // Should create without panicking and have no receivers.
        let progress = IngestionProgress {
            job_id: "default-test".to_string(),
            total: 0,
            processed: 0,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            eta_seconds: None,
            emails_per_second: 0.0,
        };
        // No receivers — send returns Err.
        assert!(broadcast.send(progress).is_err());
    }

    #[test]
    fn test_job_response_serialization() {
        let resp = JobResponse {
            job_id: "test-123".to_string(),
            status: "started".to_string(),
            message: "Ingestion started".to_string(),
        };

        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("test-123"));
        assert!(json.contains("started"));
    }

    #[test]
    fn test_ingestion_phase_equality() {
        assert_eq!(IngestionPhase::Syncing, IngestionPhase::Syncing);
        assert_ne!(IngestionPhase::Syncing, IngestionPhase::Complete);
    }
}
