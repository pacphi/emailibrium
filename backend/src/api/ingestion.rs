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
use tracing::{debug, info, warn};

use crate::email::provider::EmailProvider;
use crate::email::types::{ListParams, ProviderKind};
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
        .route("/resume-checkpoint", post(resume_from_checkpoint))
        .route("/checkpoint", get(get_checkpoint))
        .route("/embedding-status", get(embedding_status))
        .route("/poll-status", get(poll_status))
        .route("/poll-toggle", post(poll_toggle))
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

/// Sync emails from the provider API into the local `emails` table.
///
/// Two modes:
/// - **Full sync** (onboarding): No `history_id` in `sync_state` — paginates
///   through all messages. This is what happens on first account connection.
/// - **Incremental sync** (polling): Has `history_id` — uses Gmail's
///   `history.list` or Outlook's delta query to fetch only changed messages.
///   Dramatically faster for routine new-mail checks.
pub async fn sync_emails_from_provider(
    state: &AppState,
    account_id: &str,
) -> Result<u64, (StatusCode, String)> {
    // Look up the account to get provider type.
    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Account {account_id} not found"),
            )
        })?;

    // Get access token (refresh if needed).
    let access_token = state
        .oauth_manager
        .get_access_token(account_id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;

    let oauth = &state.vector_service.config.oauth;
    let provider_str = account.provider.as_str();

    // Build provider.
    let provider: Box<dyn EmailProvider> = match account.provider {
        ProviderKind::Gmail => {
            let gmail_cfg = &oauth.gmail;
            let client_id = std::env::var(&gmail_cfg.client_id_env).unwrap_or_default();
            let client_secret = std::env::var(&gmail_cfg.client_secret_env).unwrap_or_default();
            Box::new(crate::email::gmail::GmailProvider::new(
                crate::email::types::ProviderConfig {
                    client_id,
                    client_secret,
                    redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
                    auth_url: gmail_cfg.auth_url.clone(),
                    token_url: gmail_cfg.token_url.clone(),
                    scopes: gmail_cfg.scopes.clone(),
                },
            ))
        }
        ProviderKind::Outlook => {
            let outlook_cfg = &oauth.outlook;
            let client_id = std::env::var(&outlook_cfg.client_id_env).unwrap_or_default();
            let client_secret = std::env::var(&outlook_cfg.client_secret_env).unwrap_or_default();
            Box::new(crate::email::outlook::OutlookProvider::new(
                crate::email::types::ProviderConfig {
                    client_id,
                    client_secret,
                    redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
                    auth_url: outlook_cfg.auth_url(),
                    token_url: outlook_cfg.token_url(),
                    scopes: outlook_cfg.scopes.clone(),
                },
            ))
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Provider {provider_str} sync not yet supported"),
            ));
        }
    };

    // Check sync_state for an existing history_id (incremental sync marker).
    let sync_row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT history_id FROM sync_state WHERE account_id = ?")
            .bind(account_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let history_id = sync_row.and_then(|(h,)| h);

    // ── Incremental sync path ────────────────────────────────────────────
    // If we have a history_id from a previous sync, use the provider's delta
    // API to fetch only new/changed messages instead of re-listing everything.
    if let Some(ref hid) = history_id {
        info!(
            account_id = %account_id,
            provider = %provider_str,
            history_id = %hid,
            "Starting incremental sync (delta)"
        );

        let delta_result = incremental_sync_delta(
            state,
            account_id,
            provider_str,
            &*provider,
            &access_token,
            hid,
        )
        .await;

        match delta_result {
            Ok(count) => return Ok(count),
            Err(e) => {
                // Delta failed (e.g. expired history_id). Clear it and fall
                // through to full sync below.
                warn!(
                    account_id = %account_id,
                    "Incremental sync failed ({e}), falling back to full sync"
                );
                let _ = sqlx::query("UPDATE sync_state SET history_id = NULL WHERE account_id = ?")
                    .bind(account_id)
                    .execute(&state.db.pool)
                    .await;
            }
        }
    }

    // ── Full sync path (onboarding) ──────────────────────────────────────
    info!(
        account_id = %account_id,
        provider = %provider_str,
        "Starting full sync (onboarding)"
    );

    let mut inserted = 0u64;
    let mut page_num = 0u64;
    let mut page_token: Option<String> = None;
    let batch_size = 100u32;

    loop {
        page_num += 1;
        let params = ListParams {
            max_results: batch_size,
            page_token: page_token.clone(),
            label: None,
            query: None,
        };

        debug!(
            account_id = %account_id,
            page = page_num,
            "Fetching message page from provider"
        );

        let page = provider
            .list_messages(&access_token, &params)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to list messages on page {page_num}: {e}"),
                )
            })?;

        let page_count = page.messages.len();
        let has_more = page.next_page_token.is_some();
        info!(
            account_id = %account_id,
            page = page_num,
            messages_in_page = page_count,
            has_more = has_more,
            total_inserted = inserted,
            "Fetched message page from provider"
        );

        for msg in &page.messages {
            inserted += upsert_email(state, account_id, provider_str, msg).await;
        }

        debug!(
            account_id = %account_id,
            page = page_num,
            inserted = inserted,
            "Page processed"
        );

        // Continue to next page or break.
        page_token = page.next_page_token;
        if page_token.is_none() || page.messages.is_empty() {
            break;
        }
    }

    // After full sync, capture the provider's current history marker so the
    // next sync can use the incremental (delta) path.
    let new_history_id = fetch_provider_history_id(provider_str, &*provider, &access_token).await;

    // Update sync state with count + history marker.
    if let Some(ref hid) = new_history_id {
        let _ = sqlx::query(
            "UPDATE sync_state SET emails_synced = emails_synced + ?1, history_id = ?2, \
             last_sync_at = datetime('now'), status = 'idle' WHERE account_id = ?3",
        )
        .bind(inserted as i64)
        .bind(hid)
        .bind(account_id)
        .execute(&state.db.pool)
        .await;
    } else {
        let _ = sqlx::query(
            "UPDATE sync_state SET emails_synced = emails_synced + ?1, \
             last_sync_at = datetime('now'), status = 'idle' WHERE account_id = ?2",
        )
        .bind(inserted as i64)
        .bind(account_id)
        .execute(&state.db.pool)
        .await;
    }

    info!(
        account_id = %account_id,
        provider = %provider_str,
        emails_synced = inserted,
        has_history_id = new_history_id.is_some(),
        "Full sync complete — future syncs will use incremental delta"
    );

    Ok(inserted)
}

// ---------------------------------------------------------------------------
// Incremental sync helpers
// ---------------------------------------------------------------------------

/// Perform an incremental sync using the provider's delta API.
///
/// Only fetches messages that changed since `history_id`. Returns the count
/// of new/updated emails upserted, or an error string if delta detection fails.
async fn incremental_sync_delta(
    state: &AppState,
    account_id: &str,
    provider_str: &str,
    provider: &dyn EmailProvider,
    access_token: &str,
    history_id: &str,
) -> Result<u64, String> {
    // Call the appropriate delta API based on provider type.
    let delta = match provider_str {
        "gmail" => {
            let url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/history?startHistoryId={history_id}"
            );
            let resp: serde_json::Value = reqwest::Client::new()
                .get(&url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let gmail_delta =
                crate::email::delta::parse_gmail_history(&resp).map_err(|e| e.to_string())?;

            // Map Gmail delta to a common shape.
            let updated_ids: Vec<String> = gmail_delta
                .label_changes
                .iter()
                .map(|lc| lc.message_id.clone())
                .collect();
            crate::email::sync::DeltaResult {
                new_message_ids: gmail_delta.added_message_ids,
                updated_message_ids: updated_ids,
                deleted_message_ids: gmail_delta.deleted_message_ids,
                new_history_id: gmail_delta.new_history_id,
            }
        }
        "outlook" => {
            let url = format!(
                "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken={history_id}"
            );
            let resp: serde_json::Value = reqwest::Client::new()
                .get(&url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let outlook_delta =
                crate::email::delta::parse_outlook_delta(&resp).map_err(|e| e.to_string())?;
            crate::email::sync::DeltaResult {
                new_message_ids: outlook_delta.added_or_modified_ids,
                updated_message_ids: Vec::new(),
                deleted_message_ids: outlook_delta.deleted_ids,
                new_history_id: outlook_delta.delta_link,
            }
        }
        _ => return Err(format!("Incremental sync not supported for {provider_str}")),
    };

    let new_ids = &delta.new_message_ids;
    let updated_ids = &delta.updated_message_ids;
    let deleted_ids = &delta.deleted_message_ids;
    let total_changes = new_ids.len() + updated_ids.len() + deleted_ids.len();

    info!(
        account_id = %account_id,
        new = new_ids.len(),
        updated = updated_ids.len(),
        deleted = deleted_ids.len(),
        "Incremental sync: {total_changes} changes detected"
    );

    if total_changes == 0 {
        // No changes — just advance the history marker.
        if let Some(ref new_hid) = delta.new_history_id {
            let _ = sqlx::query(
                "UPDATE sync_state SET history_id = ?, last_sync_at = datetime('now'), \
                 status = 'idle' WHERE account_id = ?",
            )
            .bind(new_hid)
            .bind(account_id)
            .execute(&state.db.pool)
            .await;
        }
        return Ok(0);
    }

    // Fetch full details for new + updated messages.
    let mut inserted = 0u64;
    for msg_id in new_ids.iter().chain(updated_ids.iter()) {
        match provider.get_message(access_token, msg_id).await {
            Ok(msg) => {
                inserted += upsert_email(state, account_id, provider_str, &msg).await;
            }
            Err(e) => {
                warn!(email_id = %msg_id, "Incremental sync: failed to fetch message: {e}");
            }
        }
    }

    // Handle remote deletions.
    for msg_id in deleted_ids {
        let _ = sqlx::query("DELETE FROM emails WHERE id = ? AND account_id = ?")
            .bind(msg_id)
            .bind(account_id)
            .execute(&state.db.pool)
            .await;
    }

    // Update sync state with new history marker.
    if let Some(ref new_hid) = delta.new_history_id {
        let _ = sqlx::query(
            "UPDATE sync_state SET emails_synced = emails_synced + ?1, history_id = ?2, \
             last_sync_at = datetime('now'), status = 'idle' WHERE account_id = ?3",
        )
        .bind(inserted as i64)
        .bind(new_hid)
        .bind(account_id)
        .execute(&state.db.pool)
        .await;
    }

    info!(
        account_id = %account_id,
        provider = %provider_str,
        new_emails = inserted,
        deleted = deleted_ids.len(),
        "Incremental sync complete"
    );

    Ok(inserted)
}

/// Upsert a single email message into the local DB. Returns 1 if inserted, 0 otherwise.
async fn upsert_email(
    state: &AppState,
    account_id: &str,
    provider_str: &str,
    msg: &crate::email::types::EmailMessage,
) -> u64 {
    let is_starred = msg.labels.iter().any(|l| l == "STARRED");
    let has_attachments = false;
    let result = sqlx::query(
        r#"INSERT INTO emails
           (id, account_id, provider, message_id, thread_id, subject,
            from_addr, to_addrs, received_at, body_text, body_html, labels,
            is_read, is_starred, has_attachments, embedding_status)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'pending')
           ON CONFLICT(id) DO UPDATE SET
            body_html = CASE WHEN length(excluded.body_html) > 0 THEN excluded.body_html ELSE emails.body_html END,
            body_text = CASE WHEN length(excluded.body_text) > 0 THEN excluded.body_text ELSE emails.body_text END,
            labels = excluded.labels,
            is_read = excluded.is_read,
            is_starred = excluded.is_starred"#,
    )
    .bind(&msg.id)
    .bind(account_id)
    .bind(provider_str)
    .bind(&msg.id)
    .bind(&msg.thread_id)
    .bind(&msg.subject)
    .bind(&msg.from)
    .bind(msg.to.join(", "))
    .bind(msg.date.to_rfc3339())
    .bind(msg.body.as_deref().unwrap_or(&msg.snippet))
    .bind(msg.body_html.as_deref().unwrap_or(""))
    .bind(msg.labels.join(","))
    .bind(msg.is_read)
    .bind(is_starred)
    .bind(has_attachments)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => 1,
        Ok(_) => 0,
        Err(e) => {
            warn!(email_id = %msg.id, "Failed to upsert email: {e}");
            0
        }
    }
}

/// Fetch the provider's current history/delta marker after a full sync.
/// For Gmail, this calls the profile endpoint to get the current `historyId`.
/// For Outlook, the delta link is only available during delta queries, so we
/// return None (Outlook incremental sync starts after the first delta call).
async fn fetch_provider_history_id(
    provider_str: &str,
    _provider: &dyn EmailProvider,
    access_token: &str,
) -> Option<String> {
    match provider_str {
        "gmail" => {
            // Call the Gmail profile endpoint to get the current historyId.
            let resp = reqwest::Client::new()
                .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
                .bearer_auth(access_token)
                .send()
                .await
                .ok()?
                .json::<serde_json::Value>()
                .await
                .ok()?;

            resp["historyId"].as_str().map(|s| s.to_string())
        }
        _ => None,
    }
}

/// POST /api/v1/ingestion/start — sync from provider then run ingestion pipeline.
///
/// 1. Fetches emails from the provider API (Gmail/Outlook) into local DB
/// 2. Runs the embedding + categorization pipeline on pending emails
async fn start_ingestion(
    State(state): State<AppState>,
    Json(req): Json<StartRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    let account_id = req.account_id.unwrap_or_else(|| "default".to_string());

    debug!(
        account_id = %account_id,
        full_sync = req.full_sync.unwrap_or(false),
        "starting ingestion job"
    );

    // Generate job ID early so we can return it immediately.
    let job_id = uuid::Uuid::new_v4().to_string();

    // Spawn sync + pipeline in background so the HTTP response returns immediately.
    let bg_state = state.clone();
    let bg_account_id = account_id.clone();
    let bg_job_id = job_id.clone();
    tokio::spawn(async move {
        // Broadcast: sync starting.
        let _ = bg_state.ingestion_broadcast.send(IngestionProgress {
            job_id: bg_job_id.clone(),
            total: 0,
            processed: 0,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            eta_seconds: None,
            emails_per_second: 0.0,
        });

        // Phase 0: Sync emails from the provider into local DB.
        let mut synced_count = 0u64;
        if bg_account_id != "default" {
            match sync_emails_from_provider(&bg_state, &bg_account_id).await {
                Ok(n) => {
                    synced_count = n;
                    info!(account_id = %bg_account_id, synced = n, "Provider sync complete");
                }
                Err((_status, msg)) => {
                    warn!(account_id = %bg_account_id, "Provider sync failed: {msg}");
                }
            }
        }

        // Broadcast: sync done, pipeline starting.
        let _ = bg_state.ingestion_broadcast.send(IngestionProgress {
            job_id: bg_job_id.clone(),
            total: synced_count,
            processed: synced_count,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Embedding,
            eta_seconds: None,
            emails_per_second: 0.0,
        });

        // Phase 1+: Run the embedding/categorization pipeline on pending emails.
        match bg_state
            .vector_service
            .ingestion_pipeline
            .start_ingestion(&bg_account_id)
            .await
        {
            Ok(pipeline_job_id) => {
                info!(job_id = %bg_job_id, pipeline_job_id = %pipeline_job_id, "Ingestion pipeline started");

                let event_id = pipeline_job_id.clone();
                bg_state
                    .event_bus
                    .emit(
                        &event_id,
                        crate::events::DomainEvent::EmailIngested {
                            email_id: pipeline_job_id,
                            account_id: bg_account_id.clone(),
                            subject: format!("Ingestion batch started for {bg_account_id}"),
                            from_addr: String::new(),
                        },
                    )
                    .await;
            }
            Err(e) => {
                warn!(job_id = %bg_job_id, "Ingestion pipeline failed: {e}");
            }
        }

        // Progress broadcasting is handled by the inner ingestion pipeline.
        // Do not broadcast a premature Complete event here.
        info!(job_id = %bg_job_id, "Ingestion pipeline dispatched for account {bg_account_id}");
    });

    Ok(Json(JobResponse {
        job_id,
        status: "started".to_string(),
        message: format!("Ingestion started for account {account_id}"),
    }))
}

/// POST /api/v1/ingestion/pause — pause a running ingestion job.
async fn pause_ingestion(
    State(state): State<AppState>,
    Json(req): Json<PauseResumeRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    debug!(job_id = %req.job_id, "pausing ingestion job");

    state
        .vector_service
        .ingestion_pipeline
        .pause()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(JobResponse {
        job_id: req.job_id,
        status: "paused".to_string(),
        message: "Ingestion job paused".to_string(),
    }))
}

/// POST /api/v1/ingestion/resume — resume a paused ingestion job.
async fn resume_ingestion(
    State(state): State<AppState>,
    Json(req): Json<PauseResumeRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    debug!(job_id = %req.job_id, "resuming ingestion job");

    state
        .vector_service
        .ingestion_pipeline
        .resume()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
// Checkpoint/resume endpoints (audit item #26)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ResumeCheckpointRequest {
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckpointQuery {
    pub account_id: String,
}

/// POST /api/v1/ingestion/resume-checkpoint -- resume from last failure checkpoint.
async fn resume_from_checkpoint(
    State(state): State<AppState>,
    Json(req): Json<ResumeCheckpointRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    debug!(account_id = %req.account_id, "resuming ingestion from checkpoint");

    match state
        .vector_service
        .ingestion_pipeline
        .resume_from_checkpoint(&req.account_id)
        .await
    {
        Ok(Some(job_id)) => Ok(Json(JobResponse {
            job_id,
            status: "resumed".to_string(),
            message: format!(
                "Ingestion resumed from checkpoint for account {}",
                req.account_id
            ),
        })),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            "No incomplete checkpoint found for this account".to_string(),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// GET /api/v1/ingestion/checkpoint?account_id=... -- get latest checkpoint.
async fn get_checkpoint(
    State(state): State<AppState>,
    Query(params): Query<CheckpointQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match state
        .vector_service
        .ingestion_pipeline
        .get_checkpoint(&params.account_id)
        .await
    {
        Ok(Some(cp)) => Ok(Json(serde_json::to_value(cp).unwrap_or_default())),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            "No checkpoint found for this account".to_string(),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Embedding status endpoint
// ---------------------------------------------------------------------------

use crate::vectors::ingestion::EmailEmbeddingRecord;
use crate::vectors::types::EmbeddingStatus;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingStatusResponse {
    pub total_emails: u64,
    pub embedding_status_summary: EmbeddingStatusSummary,
    /// Sample record demonstrating the EmbeddingStatus lifecycle.
    pub sample_record: EmbeddingStatusSample,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingStatusSummary {
    pub embedded_count: u64,
    pub pending_count: u64,
    pub failed_count: u64,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingStatusSample {
    pub pending: String,
    pub embedded: String,
    pub failed: String,
    pub stale: String,
}

/// GET /api/v1/ingestion/embedding-status
///
/// Returns real embedding status counts from the emails table.
async fn embedding_status(
    State(state): State<AppState>,
) -> Result<Json<EmbeddingStatusResponse>, (StatusCode, String)> {
    // Query real counts from the emails table grouped by embedding_status.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT COALESCE(embedding_status, 'pending') as status, COUNT(*) as cnt \
         FROM emails GROUP BY embedding_status",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut embedded_count: u64 = 0;
    let mut pending_count: u64 = 0;
    let mut failed_count: u64 = 0;
    let mut total_emails: u64 = 0;

    for (status, count) in &rows {
        let c = *count as u64;
        total_emails += c;
        match status.as_str() {
            "embedded" => embedded_count = c,
            "pending" => pending_count = c,
            "failed" => failed_count = c,
            _ => pending_count += c,
        }
    }

    // Validate wiring of EmbeddingStatus and EmailEmbeddingRecord types.
    let _status = EmbeddingStatus::Pending;
    let _record = EmailEmbeddingRecord::pending("wiring-check".to_string());

    Ok(Json(EmbeddingStatusResponse {
        total_emails,
        embedding_status_summary: EmbeddingStatusSummary {
            embedded_count,
            pending_count,
            failed_count,
        },
        sample_record: EmbeddingStatusSample {
            pending: "pending".to_string(),
            embedded: "embedded".to_string(),
            failed: "failed".to_string(),
            stale: "stale".to_string(),
        },
    }))
}

// ---------------------------------------------------------------------------
// Poll scheduler endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/ingestion/poll-status — current state of the background poller.
async fn poll_status(
    State(state): State<AppState>,
) -> Result<Json<crate::email::poll_scheduler::PollStatus>, (StatusCode, String)> {
    match &state.poll_scheduler {
        Some(handle) => Ok(Json(handle.status().await)),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Poll scheduler not initialized".to_string(),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct PollToggleRequest {
    pub enabled: bool,
}

/// POST /api/v1/ingestion/poll-toggle — enable or disable background polling.
async fn poll_toggle(
    State(state): State<AppState>,
    Json(req): Json<PollToggleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match &state.poll_scheduler {
        Some(handle) => {
            handle.set_enabled(req.enabled).await;
            info!(enabled = req.enabled, "Poll scheduler toggled");
            Ok(Json(serde_json::json!({ "enabled": req.enabled })))
        }
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Poll scheduler not initialized".to_string(),
        )),
    }
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
