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
use sea_orm::sea_query::{Expr, ExprTrait, Func, OnConflict};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect,
};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use crate::db::entities::{emails, sync_state};
use crate::email::provider::EmailProvider;
use crate::email::types::{ListParams, ProviderKind};
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

// These types now live in `vectors::ingestion` so library-side consumers (the
// MCP tool registry among them) can read sync progress without depending on the
// binary crate. Re-exported under their original names so every call site here
// — and `AppState` — is unaffected.
pub use crate::vectors::ingestion::{
    IngestionBroadcast, SyncPhase as IngestionPhase, SyncProgress as IngestionProgress,
};

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
    /// Who initiated this ingestion: "onboarding", "manual_sync", "inbox_clean", "poll".
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LockStatusQuery {
    pub account_id: String,
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
        .route("/progress", get(ingestion_progress_json))
        .route("/backfill-progress", get(backfill_progress_json))
        .route("/lock-status", get(lock_status))
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

// ---------------------------------------------------------------------------
// sync_state persistence
// ---------------------------------------------------------------------------

/// The `'YYYY-MM-DD HH:MM:SS'` UTC string `sync_state.last_sync_at` holds.
///
/// That column is TEXT, not TIMESTAMPTZ, in both dialects (ADR-035 §2.5), so the format is
/// the application's to own. SQLite's `datetime('now')` — what the pre-port writes used —
/// and the PostgreSQL DDL default's `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')`
/// both produce exactly this, so one Rust-side format string is the single code path for
/// both (ADR-036). Same helper, same reasoning, as `email/oauth.rs`.
fn now_text() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// `UPDATE sync_state SET emails_synced = emails_synced + n, [history_id = h,]
/// last_sync_at = <now>, status = 'idle' WHERE account_id = ?` — the shape every
/// end-of-sync write in this file shared.
///
/// `history_id` is left alone when `None`, matching the full-sync branch that ran when the
/// provider had no marker to record. The delta path's "no changes detected" write never
/// named `emails_synced`; it passes `synced = 0` here, and `emails_synced = emails_synced + 0`
/// is the same write as omitting the column.
async fn mark_sync_idle(
    conn: &DatabaseConnection,
    account_id: &str,
    synced: u64,
    history_id: Option<&str>,
) -> Result<u64, DbErr> {
    // `emails_synced` is INTEGER (INT4 on PostgreSQL) while the counter is u64, so the
    // delta narrows at the bind.
    let delta = i32::try_from(synced).unwrap_or(i32::MAX);

    let mut update = sync_state::Entity::update_many()
        .col_expr(
            sync_state::Column::EmailsSynced,
            Expr::col(sync_state::Column::EmailsSynced).add(delta),
        )
        .col_expr(sync_state::Column::LastSyncAt, Expr::value(now_text()))
        .col_expr(sync_state::Column::Status, Expr::value("idle"));
    if let Some(hid) = history_id {
        update = update.col_expr(sync_state::Column::HistoryId, Expr::value(hid));
    }

    Ok(update
        .filter(sync_state::Column::AccountId.eq(account_id))
        .exec(conn)
        .await?
        .rows_affected)
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

    let oauth = &state.vector_service.config.oauth;
    let provider_str = account.provider.as_str();

    // Build the provider and obtain its credential. OAuth providers (Gmail /
    // Outlook) use a per-account access token; IMAP uses stored credentials and
    // ignores the token, so only fetch a token for the OAuth kinds.
    let (provider, access_token): (Box<dyn EmailProvider>, String) = match account.provider {
        ProviderKind::Gmail => {
            let access_token = state
                .oauth_manager
                .get_access_token(account_id)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;
            let gmail_cfg = &oauth.gmail;
            let client_id = std::env::var(&gmail_cfg.client_id_env).unwrap_or_default();
            let client_secret = std::env::var(&gmail_cfg.client_secret_env).unwrap_or_default();
            (
                Box::new(crate::email::gmail::GmailProvider::new(
                    crate::email::types::ProviderConfig {
                        client_id,
                        client_secret,
                        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
                        auth_url: gmail_cfg.auth_url.clone(),
                        token_url: gmail_cfg.token_url.clone(),
                        scopes: gmail_cfg.scopes.clone(),
                    },
                )),
                access_token,
            )
        }
        ProviderKind::Outlook => {
            let access_token = state
                .oauth_manager
                .get_access_token(account_id)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;
            let outlook_cfg = &oauth.outlook;
            let client_id = std::env::var(&outlook_cfg.client_id_env).unwrap_or_default();
            let client_secret = std::env::var(&outlook_cfg.client_secret_env).unwrap_or_default();
            (
                Box::new(crate::email::outlook::OutlookProvider::new(
                    crate::email::types::ProviderConfig {
                        client_id,
                        client_secret,
                        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
                        auth_url: outlook_cfg.auth_url(),
                        token_url: outlook_cfg.token_url(),
                        scopes: outlook_cfg.scopes.clone(),
                    },
                )),
                access_token,
            )
        }
        ProviderKind::Imap => {
            let config = state
                .oauth_manager
                .load_imap_config(account_id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("IMAP config: {e}"),
                    )
                })?;
            // SSRF-guard the stored host and pin the resolved IP for the connect.
            let config = crate::api::provider_helpers::guard_and_pin_imap_config(config).await?;
            (
                Box::new(crate::email::imap::ImapProvider::new(config)),
                String::new(),
            )
        }
        ProviderKind::Pop3 => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Provider {provider_str} sync not yet supported"),
            ));
        }
    };

    // Check sync_state for an existing history_id (incremental sync marker).
    // Outer `None` = the account has no sync_state row; inner `None` = it has no marker.
    let sync_row: Option<Option<String>> = sync_state::Entity::find()
        .select_only()
        .column(sync_state::Column::HistoryId)
        .filter(sync_state::Column::AccountId.eq(account_id))
        .into_tuple()
        .one(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let history_id = sync_row.flatten();

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
                let _ = sync_state::Entity::update_many()
                    .col_expr(
                        sync_state::Column::HistoryId,
                        Expr::value(Option::<String>::None),
                    )
                    .filter(sync_state::Column::AccountId.eq(account_id))
                    .exec(&state.orm)
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

    // Load enabled rules once for the whole sync run so we can apply them
    // to each email as it arrives, avoiding stragglers.
    let enabled_rules: Vec<crate::rules::types::Rule> =
        crate::rules::rule_engine::RuleEngine::load_rules(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.enabled)
            .collect();

    let mut inserted = 0u64;
    let mut page_num = 0u64;
    let mut page_token: Option<String> = None;
    let batch_size = 100u32;
    // Estimated total from the provider (e.g. Gmail's resultSizeEstimate).
    // Set from the first page response to enable a determinate progress bar.
    let mut estimated_total: u64 = 0;

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

        // Capture estimated total from the first page if available.
        if page_num == 1 {
            if let Some(est) = page.result_size_estimate {
                estimated_total = est as u64;
            }
        }

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
            let n = upsert_email(&state.orm, account_id, provider_str, msg).await;
            if n > 0 && !enabled_rules.is_empty() {
                crate::rules::executor::apply_rules_to_email(&state.db, msg, &enabled_rules).await;
            }
            inserted += n;
        }

        // Broadcast per-page progress so the dashboard banner can show
        // "Fetching emails (N / ~total)" during the syncing phase.
        let _ = state.ingestion_broadcast.send(IngestionProgress {
            job_id: String::new(),
            total: estimated_total,
            processed: inserted,
            embedded: 0,
            categorized: 0,
            failed: 0,
            phase: IngestionPhase::Syncing,
            eta_seconds: None,
            emails_per_second: 0.0,
        });

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

    // Update sync state with count + history marker. The two former branches differed only
    // in whether `history_id` was in the SET list, which `mark_sync_idle` keys off `None`.
    let _ = mark_sync_idle(&state.orm, account_id, inserted, new_history_id.as_deref()).await;

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
    let (delta, gmail_label_changes) = match provider_str {
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
            let label_changes = gmail_delta.label_changes.clone();
            (
                crate::email::sync::DeltaResult {
                    new_message_ids: gmail_delta.added_message_ids,
                    updated_message_ids: updated_ids,
                    deleted_message_ids: gmail_delta.deleted_message_ids,
                    new_history_id: gmail_delta.new_history_id,
                },
                label_changes,
            )
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

            // Build label-change equivalents from Outlook folder info.
            let mut label_changes = Vec::new();
            for fm in &outlook_delta.folder_moves {
                let folder_upper = fm.folder_name.to_uppercase();
                let mut added = Vec::new();
                if folder_upper == "DELETEDITEMS" || folder_upper == "TRASH" {
                    added.push("TRASH".to_string());
                } else if folder_upper == "JUNKEMAIL" || folder_upper == "SPAM" {
                    added.push("SPAM".to_string());
                }
                if !added.is_empty() {
                    label_changes.push(crate::email::delta::GmailLabelDelta {
                        message_id: fm.message_id.clone(),
                        added_labels: added,
                        removed_labels: Vec::new(),
                    });
                }
            }

            (
                crate::email::sync::DeltaResult {
                    new_message_ids: outlook_delta.added_or_modified_ids,
                    updated_message_ids: Vec::new(),
                    deleted_message_ids: outlook_delta.deleted_ids,
                    new_history_id: outlook_delta.delta_link,
                },
                label_changes,
            )
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
            let _ = mark_sync_idle(&state.orm, account_id, 0, Some(new_hid)).await;
        }
        return Ok(0);
    }

    // Load enabled rules once so actions can be applied to each arriving email.
    let enabled_rules: Vec<crate::rules::types::Rule> =
        crate::rules::rule_engine::RuleEngine::load_rules(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.enabled)
            .collect();

    // Fetch full details for new + updated messages.
    let mut inserted = 0u64;
    for msg_id in new_ids.iter().chain(updated_ids.iter()) {
        match provider.get_message(access_token, msg_id).await {
            Ok(msg) => {
                let n = upsert_email(&state.orm, account_id, provider_str, &msg).await;
                if n > 0 && !enabled_rules.is_empty() {
                    crate::rules::executor::apply_rules_to_email(&state.db, &msg, &enabled_rules)
                        .await;
                }
                inserted += n;
            }
            Err(e) => {
                warn!(email_id = %msg_id, "Incremental sync: failed to fetch message: {e}");
            }
        }
    }

    // Handle remote deletions — soft-delete by marking as trashed with a
    // deleted_at timestamp instead of permanently removing rows.
    let now_iso = chrono::Utc::now().to_rfc3339();
    for msg_id in deleted_ids {
        if let Err(e) =
            crate::db::update_email_state(&state.orm, msg_id, true, false, "TRASH", Some(&now_iso))
                .await
        {
            warn!(email_id = %msg_id, "Failed to soft-delete email during delta sync: {e}");
        }
    }

    // Process label changes — map TRASH/SPAM label additions/removals to
    // local email state updates.
    for lc in &gmail_label_changes {
        let has_added = |name: &str| lc.added_labels.iter().any(|l| l.eq_ignore_ascii_case(name));
        let has_removed = |name: &str| {
            lc.removed_labels
                .iter()
                .any(|l| l.eq_ignore_ascii_case(name))
        };

        if has_added("TRASH") {
            let _ = crate::db::update_email_state(
                &state.orm,
                &lc.message_id,
                true,
                false,
                "TRASH",
                None,
            )
            .await;
        } else if has_removed("TRASH") {
            let _ = crate::db::update_email_state(
                &state.orm,
                &lc.message_id,
                false,
                false,
                "INBOX",
                None,
            )
            .await;
        }

        if has_added("SPAM") {
            let _ = crate::db::update_email_state(
                &state.orm,
                &lc.message_id,
                false,
                true,
                "SPAM",
                None,
            )
            .await;
        } else if has_removed("SPAM") {
            let _ = crate::db::update_email_state(
                &state.orm,
                &lc.message_id,
                false,
                false,
                "INBOX",
                None,
            )
            .await;
        }
    }

    // Update sync state with new history marker.
    if let Some(ref new_hid) = delta.new_history_id {
        let _ = mark_sync_idle(&state.orm, account_id, inserted, Some(new_hid)).await;
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

// The three `ON CONFLICT DO UPDATE` assignments sea-query has no typed form for. Both
// dialects name the proposed row `excluded` and the existing row by table name inside
// `DO UPDATE`, and both have `length()` and `COALESCE()`, so one portable SQL text covers
// what ADR-035 §2.3 catalogs as the genuinely-different-SQL-text (upsert) class. These are
// `&'static str` literals, never built from input, keeping ADR-036 §5's raw-fragment audit
// surface enumerable.

/// Keep the stored body when the incoming one is empty; `length(NULL)` is NULL on both
/// backends, so a NULL incoming body also keeps the stored one.
const CONFLICT_BODY_HTML: &str =
    "CASE WHEN length(excluded.body_html) > 0 THEN excluded.body_html ELSE emails.body_html END";
/// See [`CONFLICT_BODY_HTML`].
const CONFLICT_BODY_TEXT: &str =
    "CASE WHEN length(excluded.body_text) > 0 THEN excluded.body_text ELSE emails.body_text END";
/// Keep the stored unsubscribe header when the provider didn't send one this time.
const CONFLICT_LIST_UNSUBSCRIBE: &str =
    "COALESCE(excluded.list_unsubscribe, emails.list_unsubscribe)";
/// See [`CONFLICT_LIST_UNSUBSCRIBE`].
const CONFLICT_LIST_UNSUBSCRIBE_POST: &str =
    "COALESCE(excluded.list_unsubscribe_post, emails.list_unsubscribe_post)";

/// What a re-delivery of an already-stored message does to the row.
///
/// Split out from [`upsert_email`] so the rendered-SQL test can pin the real clause rather
/// than a copy of it.
fn upsert_conflict_action() -> OnConflict {
    OnConflict::column(emails::Column::Id)
        .value(emails::Column::BodyHtml, Expr::cust(CONFLICT_BODY_HTML))
        .value(emails::Column::BodyText, Expr::cust(CONFLICT_BODY_TEXT))
        .update_columns([
            emails::Column::Labels,
            emails::Column::IsRead,
            emails::Column::IsStarred,
            emails::Column::IsTrash,
            emails::Column::IsSpam,
            emails::Column::Folder,
        ])
        .value(
            emails::Column::ListUnsubscribe,
            Expr::cust(CONFLICT_LIST_UNSUBSCRIBE),
        )
        .value(
            emails::Column::ListUnsubscribePost,
            Expr::cust(CONFLICT_LIST_UNSUBSCRIBE_POST),
        )
        .to_owned()
}

/// The row a provider message inserts, before any conflict handling.
fn incoming_email_row(
    account_id: &str,
    provider_str: &str,
    msg: &crate::email::types::EmailMessage,
) -> emails::ActiveModel {
    let is_starred = msg.labels.iter().any(|l| l == "STARRED");
    let has_attachments = false;

    // Derive is_trash, is_spam, folder from provider labels.
    let (is_trash, is_spam, folder) = crate::db::derive_state_from_labels(&msg.labels);

    // Columns the old INSERT didn't name stay `NotSet` so the DDL default still fills them.
    emails::ActiveModel {
        id: Set(msg.id.clone()),
        account_id: Set(account_id.to_owned()),
        provider: Set(provider_str.to_owned()),
        message_id: Set(Some(msg.id.clone())),
        thread_id: Set(msg.thread_id.clone()),
        subject: Set(msg.subject.clone()),
        from_addr: Set(msg.from.clone()),
        to_addrs: Set(msg.to.join(", ")),
        // `received_at` is a plain TIMESTAMP in both dialects, so a temporal value is bound
        // rather than the RFC3339 String the pre-port code bound — that String is what broke
        // this write on PostgreSQL (ADR-035 §2.6; see the entity's module doc).
        received_at: Set(msg.date.naive_utc()),
        body_text: Set(Some(msg.body.as_deref().unwrap_or(&msg.snippet).to_owned())),
        body_html: Set(Some(msg.body_html.as_deref().unwrap_or("").to_owned())),
        labels: Set(Some(msg.labels.join(","))),
        is_read: Set(Some(msg.is_read)),
        is_starred: Set(Some(is_starred)),
        has_attachments: Set(Some(has_attachments)),
        embedding_status: Set(Some("pending".to_owned())),
        is_trash: Set(is_trash as i32),
        is_spam: Set(is_spam as i32),
        folder: Set(folder.to_owned()),
        list_unsubscribe: Set(msg.list_unsubscribe.clone()),
        list_unsubscribe_post: Set(msg.list_unsubscribe_post.clone()),
        ..Default::default()
    }
}

/// Upsert a single email message into the local DB. Returns 1 if inserted, 0 otherwise.
///
/// "1 if inserted" overstates it and always did: `ON CONFLICT DO UPDATE` reports one
/// affected row on the update path too, so a re-delivered message counts as new. Preserved.
async fn upsert_email(
    conn: &DatabaseConnection,
    account_id: &str,
    provider_str: &str,
    msg: &crate::email::types::EmailMessage,
) -> u64 {
    let result = emails::Entity::insert(incoming_email_row(account_id, provider_str, msg))
        .on_conflict(upsert_conflict_action())
        .exec_without_returning(conn)
        .await;

    match result {
        Ok(rows) if rows > 0 => 1,
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
/// 1. Acquires a per-account pipeline lock (returns 409 if already active)
/// 2. Fetches emails from the provider API (Gmail/Outlook) into local DB
/// 3. Runs the embedding + categorization pipeline on pending emails
async fn start_ingestion(
    State(state): State<AppState>,
    Json(req): Json<StartRequest>,
) -> Result<Json<JobResponse>, (StatusCode, String)> {
    let account_id = req.account_id.unwrap_or_else(|| "default".to_string());
    let source = req.source.unwrap_or_else(|| "unknown".to_string());

    debug!(
        account_id = %account_id,
        full_sync = req.full_sync.unwrap_or(false),
        source = %source,
        "starting ingestion job"
    );

    // Generate job ID early so we can return it immediately.
    let job_id = uuid::Uuid::new_v4().to_string();

    // ── Acquire per-account pipeline lock ────────────────────────────────
    let activity = crate::sync_lock::PipelineActivity {
        job_id: job_id.clone(),
        account_id: account_id.clone(),
        phase: "syncing".to_string(),
        started_at: chrono::Utc::now(),
        source: source.clone(),
    };

    if let Err(existing) = state
        .pipeline_locks
        .try_acquire(&account_id, activity)
        .await
    {
        warn!(
            account_id = %account_id,
            source = %source,
            existing_job = %existing.job_id,
            existing_source = %existing.source,
            existing_phase = %existing.phase,
            "Rejected ingestion start: pipeline already active for account"
        );
        let body = serde_json::json!({
            "error": "pipeline_busy",
            "message": format!(
                "A {} operation is already in progress (phase: {}, started: {})",
                existing.source, existing.phase, existing.started_at.to_rfc3339()
            ),
            "existingJobId": existing.job_id,
            "existingSource": existing.source,
            "existingPhase": existing.phase,
            "startedAt": existing.started_at.to_rfc3339(),
        });
        return Err((
            StatusCode::CONFLICT,
            serde_json::to_string(&body).unwrap_or_default(),
        ));
    }

    info!(
        account_id = %account_id,
        job_id = %job_id,
        source = %source,
        "Pipeline lock acquired"
    );

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
        // Use app.yaml `network.ingestion_start_timeout_ms` as the overall
        // sync timeout so a stalled provider doesn't block the pipeline forever.
        let mut synced_count = 0u64;
        if bg_account_id != "default" {
            let timeout_ms = bg_state.yaml_config.app.network.ingestion_start_timeout_ms;
            let sync_timeout = Duration::from_millis(timeout_ms);
            match tokio::time::timeout(
                sync_timeout,
                sync_emails_from_provider(&bg_state, &bg_account_id),
            )
            .await
            {
                Ok(Ok(n)) => {
                    synced_count = n;
                    info!(account_id = %bg_account_id, synced = n, "Provider sync complete");
                }
                Ok(Err((_status, msg))) => {
                    warn!(account_id = %bg_account_id, "Provider sync failed: {msg}");
                }
                Err(_) => {
                    warn!(
                        account_id = %bg_account_id,
                        timeout_ms = timeout_ms,
                        "Provider sync timed out after {}ms",
                        timeout_ms,
                    );
                }
            }
        }

        // Update lock phase: sync done, pipeline starting.
        bg_state
            .pipeline_locks
            .update_phase(&bg_account_id, "embedding")
            .await;

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

        // ── Release the per-account pipeline lock ────────────────────────
        // Wait for the inner pipeline to finish before releasing so that
        // concurrent requests are properly blocked for the full duration.
        // The inner pipeline's `start_ingestion` is awaited inside the
        // spawned task above, so by this point all phases have completed.
        bg_state.pipeline_locks.release(&bg_account_id).await;
        // Clear the broadcast cache so stale syncing progress doesn't
        // make the polling endpoint report `active: true` after completion.
        bg_state.ingestion_broadcast.clear_last_progress().await;
        info!(
            account_id = %bg_account_id,
            job_id = %bg_job_id,
            "Pipeline lock released"
        );
    });

    Ok(Json(JobResponse {
        job_id,
        status: "started".to_string(),
        message: format!("Ingestion started for account {account_id}"),
    }))
}

/// GET /api/v1/ingestion/lock-status?account_id=... — check pipeline lock.
///
/// Returns the current `PipelineActivity` if a pipeline is running for the
/// given account, or `null` if idle.
async fn lock_status(
    State(state): State<AppState>,
    Query(params): Query<LockStatusQuery>,
) -> Json<serde_json::Value> {
    match state.pipeline_locks.get_activity(&params.account_id).await {
        Some(activity) => Json(serde_json::to_value(activity).unwrap_or_default()),
        None => Json(serde_json::Value::Null),
    }
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
    pub stale_count: u64,
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
    // Query real counts from the emails table grouped by embedding_status. The GROUP BY
    // stays on the raw column, as before: NULL and 'pending' rows form two separate groups
    // that both come back labelled 'pending', and the loop below overwrites rather than sums
    // for a known label — so one of the two is dropped. Preserved.
    let status_label: [Expr; 2] = [
        Expr::col(emails::Column::EmbeddingStatus),
        Expr::value("pending"),
    ];
    let rows: Vec<(String, i64)> = emails::Entity::find()
        .select_only()
        .expr_as(Func::coalesce(status_label), "status")
        .column_as(Expr::cust("COUNT(*)"), "cnt")
        .group_by(emails::Column::EmbeddingStatus)
        .into_tuple()
        .all(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut embedded_count: u64 = 0;
    let mut pending_count: u64 = 0;
    let mut failed_count: u64 = 0;
    let mut stale_count: u64 = 0;
    let mut total_emails: u64 = 0;

    for (status, count) in &rows {
        let c = *count as u64;
        total_emails += c;
        match status.as_str() {
            "embedded" => embedded_count = c,
            "pending" => pending_count = c,
            "failed" => failed_count = c,
            "stale" => stale_count = c,
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
            stale_count,
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

/// GET /api/v1/ingestion/progress — JSON snapshot of current ingestion progress.
///
/// Returns the current phase, counts, and whether ingestion is active.
/// Unlike `/status` (SSE), this is a simple request/response for polling.
async fn ingestion_progress_json(State(state): State<AppState>) -> Json<serde_json::Value> {
    // First check the pipeline's own job state (covers embedding/categorizing/etc).
    // current_job is never reset to None, so we must check phase to distinguish
    // a running job from a completed one.
    if let Some(progress) = state.vector_service.ingestion_pipeline.get_progress().await {
        if progress.phase != "complete" {
            return Json(serde_json::json!({
                "active": true,
                "jobId": progress.job_id,
                "phase": progress.phase,
                "total": progress.total,
                "processed": progress.processed,
                "embedded": progress.embedded,
                "categorized": progress.categorized,
                "failed": progress.failed,
                "etaSeconds": progress.eta_seconds,
                "emailsPerSecond": progress.emails_per_second,
            }));
        }
    }

    // Fall back to the broadcast cache — this covers the syncing phase
    // which runs *before* the pipeline creates a job.
    if let Some(bp) = state.ingestion_broadcast.last_progress().await {
        if bp.phase != IngestionPhase::Complete {
            return Json(serde_json::json!({
                "active": true,
                "jobId": bp.job_id,
                "phase": bp.phase.to_string(),
                "total": bp.total,
                "processed": bp.processed,
                "embedded": bp.embedded,
                "categorized": bp.categorized,
                "failed": bp.failed,
                "etaSeconds": bp.eta_seconds,
                "emailsPerSecond": bp.emails_per_second,
            }));
        }
    }

    Json(serde_json::json!({
        "active": false,
        "phase": null,
    }))
}

/// GET /api/v1/ingestion/backfill-progress — Poll current LLM backfill state.
async fn backfill_progress_json(State(state): State<AppState>) -> Json<serde_json::Value> {
    let progress = state
        .vector_service
        .ingestion_pipeline
        .get_backfill_progress()
        .await;
    Json(serde_json::json!({
        "active": progress.active,
        "total": progress.total,
        "categorized": progress.categorized,
        "failed": progress.failed,
    }))
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

    #[tokio::test]
    async fn test_broadcast_send_no_receivers() {
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

    #[tokio::test]
    async fn test_ingestion_broadcast_default() {
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

// ---------------------------------------------------------------------------
// Persistence tests
// ---------------------------------------------------------------------------

/// The upsert and `sync_state` writes this file owns, against a real schema.
///
/// Both were untested before the SeaORM port, so these pin the conflict-path semantics the
/// hand-written `ON CONFLICT DO UPDATE` encoded, the temporal `received_at` bind, and the
/// TEXT timestamp shape `sync_state.last_sync_at` must keep (ADR-035 §2.5).
#[cfg(test)]
mod persistence_tests {
    use chrono::{TimeZone, Utc};
    use sea_orm::ConnectionTrait;

    use super::*;
    use crate::db::Database;
    use crate::email::types::EmailMessage;

    /// In-memory connection carrying the `emails` schema (001 plus the ALTERs from
    /// 016/018/021/027 the entity's columns require) and `sync_state` (004).
    async fn test_conn() -> DatabaseConnection {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let conn = Database::Sqlite(pool).sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/004_accounts.sql"),
            include_str!("../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../migrations/sqlite/027_is_archived.sql"),
        ] {
            let cleaned: String = raw
                .lines()
                .map(|l| l.find("--").map_or(l, |idx| &l[..idx]))
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in cleaned.split(';') {
                let s = stmt.trim();
                if !s.is_empty() {
                    conn.execute_unprepared(s).await.expect("migrate");
                }
            }
        }
        conn
    }

    /// A delivery with every column the upsert writes populated.
    fn delivery(id: &str) -> EmailMessage {
        EmailMessage {
            id: id.to_string(),
            thread_id: Some(format!("thread-{id}")),
            from: "sender@example.com".to_string(),
            to: vec!["a@example.com".to_string(), "b@example.com".to_string()],
            subject: format!("subject-{id}"),
            snippet: "snippet".to_string(),
            body: Some("body-text".to_string()),
            body_html: Some("<p>body-html</p>".to_string()),
            labels: vec!["INBOX".to_string()],
            date: Utc.with_ymd_and_hms(2026, 8, 1, 10, 30, 0).unwrap(),
            is_read: false,
            list_unsubscribe: Some("<https://unsub.test/one>".to_string()),
            list_unsubscribe_post: Some("List-Unsubscribe=One-Click".to_string()),
        }
    }

    /// An account mid-sync: `status = 'syncing'`, so a later flip to `'idle'` is visible.
    ///
    /// `sync_state.account_id` has a FK to `connected_accounts` and sqlx turns SQLite's
    /// `foreign_keys` pragma on, so the parent row has to exist first.
    async fn seed_syncing_account(
        conn: &DatabaseConnection,
        account_id: &str,
        emails_synced: i32,
        history_id: Option<&str>,
    ) {
        conn.execute_unprepared(&format!(
            "INSERT INTO connected_accounts (id, provider, email_address) \
             VALUES ('{account_id}', 'gmail', '{account_id}@example.com')"
        ))
        .await
        .expect("seed account");

        sync_state::Entity::insert(sync_state::ActiveModel {
            account_id: Set(account_id.to_owned()),
            emails_synced: Set(emails_synced),
            history_id: Set(history_id.map(str::to_owned)),
            status: Set("syncing".to_owned()),
            ..Default::default()
        })
        .exec_without_returning(conn)
        .await
        .expect("seed sync_state");
    }

    async fn sync_row(conn: &DatabaseConnection, account_id: &str) -> sync_state::Model {
        sync_state::Entity::find_by_id(account_id.to_string())
            .one(conn)
            .await
            .expect("query")
            .expect("row")
    }

    async fn stored(conn: &DatabaseConnection, id: &str) -> emails::Model {
        emails::Entity::find_by_id(id.to_string())
            .one(conn)
            .await
            .expect("query")
            .expect("row")
    }

    /// The upsert is one portable SQL text, not the per-backend pair ADR-035 §2.3 says this
    /// class of statement otherwise needs: the whole `ON CONFLICT DO UPDATE` clause renders
    /// byte-identically on both backends, and only the placeholder style differs. With no
    /// live PostgreSQL in this loop, this is what catches a regression in the raw
    /// `Expr::cust` fragments.
    #[test]
    fn the_upsert_renders_one_portable_sql_text_on_both_backends() {
        use sea_orm::{DatabaseBackend, QueryTrait};

        let render = |backend| {
            emails::Entity::insert(incoming_email_row("acct-1", "gmail", &delivery("m1")))
                .on_conflict(upsert_conflict_action())
                .build(backend)
                .sql
        };
        let sqlite = render(DatabaseBackend::Sqlite);
        let postgres = render(DatabaseBackend::Postgres);

        let conflict_clause =
            |sql: &str| sql[sql.find(" ON CONFLICT ").expect("conflict clause")..].to_string();
        assert_eq!(conflict_clause(&sqlite), conflict_clause(&postgres));

        // The two forms sea-query has no typed equivalent for survive into both dialects.
        let clause = conflict_clause(&postgres);
        assert!(
            clause.contains(&format!(r#""body_html" = {CONFLICT_BODY_HTML}"#)),
            "body_html CASE missing from: {clause}"
        );
        assert!(
            clause.contains(&format!(
                r#""list_unsubscribe" = {CONFLICT_LIST_UNSUBSCRIBE}"#
            )),
            "list_unsubscribe COALESCE missing from: {clause}"
        );
        // The plain columns still take the proposed row.
        assert!(clause.contains(r#""folder" = "excluded"."folder""#));

        // All 21 columns bind as parameters — the raw fragments leak no literal into the
        // INSERT, and PostgreSQL's numbering runs the full width.
        assert!(postgres.contains("VALUES ($1, "), "{postgres}");
        assert!(postgres.contains("$21)"), "{postgres}");
        assert!(sqlite.contains("VALUES (?, "), "{sqlite}");
    }

    #[tokio::test]
    async fn fresh_delivery_inserts_every_column_the_upsert_names() {
        let conn = test_conn().await;
        let msg = delivery("m1");

        assert_eq!(upsert_email(&conn, "acct-1", "gmail", &msg).await, 1);

        let row = stored(&conn, "m1").await;
        assert_eq!(row.account_id, "acct-1");
        assert_eq!(row.provider, "gmail");
        assert_eq!(row.message_id.as_deref(), Some("m1"));
        assert_eq!(row.thread_id.as_deref(), Some("thread-m1"));
        assert_eq!(row.subject, "subject-m1");
        assert_eq!(row.from_addr, "sender@example.com");
        assert_eq!(row.to_addrs, "a@example.com, b@example.com");
        assert_eq!(row.body_text.as_deref(), Some("body-text"));
        assert_eq!(row.body_html.as_deref(), Some("<p>body-html</p>"));
        assert_eq!(row.labels.as_deref(), Some("INBOX"));
        assert_eq!(row.is_read, Some(false));
        assert_eq!(row.is_starred, Some(false));
        assert_eq!(row.has_attachments, Some(false));
        assert_eq!(row.embedding_status.as_deref(), Some("pending"));
        assert_eq!(row.is_trash, 0);
        assert_eq!(row.is_spam, 0);
        assert_eq!(row.folder, "INBOX");
        assert_eq!(
            row.list_unsubscribe.as_deref(),
            Some("<https://unsub.test/one>")
        );
        // `is_archived` was never in the INSERT list, so it still takes the DDL default.
        assert!(!row.is_archived);
    }

    /// The conflict path: `CASE WHEN length(excluded.…) > 0` keeps a stored body against an
    /// empty re-delivery, `COALESCE` keeps a stored unsubscribe header against a NULL one,
    /// and the plain `excluded.*` columns take the new values.
    #[tokio::test]
    async fn conflicting_redelivery_keeps_empty_bodies_and_null_headers() {
        let conn = test_conn().await;
        assert_eq!(
            upsert_email(&conn, "acct-1", "gmail", &delivery("m1")).await,
            1
        );

        let mut thin = delivery("m1");
        thin.body = Some(String::new());
        thin.snippet = String::new();
        thin.body_html = Some(String::new());
        thin.list_unsubscribe = None;
        thin.list_unsubscribe_post = None;
        thin.labels = vec!["SPAM".to_string(), "STARRED".to_string()];
        thin.is_read = true;

        // An update still reports one affected row, so the caller counts it as new.
        assert_eq!(upsert_email(&conn, "acct-1", "gmail", &thin).await, 1);

        let row = stored(&conn, "m1").await;
        assert_eq!(row.body_text.as_deref(), Some("body-text"));
        assert_eq!(row.body_html.as_deref(), Some("<p>body-html</p>"));
        assert_eq!(
            row.list_unsubscribe.as_deref(),
            Some("<https://unsub.test/one>")
        );
        assert_eq!(
            row.list_unsubscribe_post.as_deref(),
            Some("List-Unsubscribe=One-Click")
        );
        assert_eq!(row.labels.as_deref(), Some("SPAM,STARRED"));
        assert_eq!(row.is_read, Some(true));
        assert_eq!(row.is_starred, Some(true));
        assert_eq!(row.is_spam, 1);
        assert_eq!(row.is_trash, 0);
        assert_eq!(row.folder, "SPAM");
    }

    #[tokio::test]
    async fn conflicting_redelivery_replaces_bodies_when_the_incoming_ones_are_not_empty() {
        let conn = test_conn().await;
        upsert_email(&conn, "acct-1", "gmail", &delivery("m1")).await;

        let mut fuller = delivery("m1");
        fuller.body = Some("body-text-v2".to_string());
        fuller.body_html = Some("<p>body-html-v2</p>".to_string());
        upsert_email(&conn, "acct-1", "gmail", &fuller).await;

        let row = stored(&conn, "m1").await;
        assert_eq!(row.body_text.as_deref(), Some("body-text-v2"));
        assert_eq!(row.body_html.as_deref(), Some("<p>body-html-v2</p>"));
    }

    /// `received_at` is bound as a temporal value, not the RFC3339 String the pre-port code
    /// wrote — it decodes back identically and compares as a timestamp.
    #[tokio::test]
    async fn received_at_round_trips_as_a_temporal_value() {
        let conn = test_conn().await;
        let msg = delivery("m1");
        upsert_email(&conn, "acct-1", "gmail", &msg).await;

        let row = stored(&conn, "m1").await;
        assert_eq!(row.received_at, msg.date.naive_utc());

        let cutoff = Utc
            .with_ymd_and_hms(2026, 7, 1, 0, 0, 0)
            .unwrap()
            .naive_utc();
        let matched = emails::Entity::find()
            .filter(emails::Column::ReceivedAt.gt(cutoff))
            .all(&conn)
            .await
            .expect("filter");
        assert_eq!(matched.len(), 1);
    }

    #[tokio::test]
    async fn mark_sync_idle_writes_the_text_timestamp_shape_and_advances_the_counter() {
        let conn = test_conn().await;
        seed_syncing_account(&conn, "acct-1", 7, None).await;

        assert_eq!(
            mark_sync_idle(&conn, "acct-1", 3, Some("hist-9"))
                .await
                .expect("update"),
            1
        );

        let row = sync_row(&conn, "acct-1").await;
        assert_eq!(row.emails_synced, 10);
        assert_eq!(row.history_id.as_deref(), Some("hist-9"));
        assert_eq!(row.status, "idle");

        // The exact shape `datetime('now')` produced, which this column's readers parse.
        let stamp = row.last_sync_at.expect("last_sync_at");
        assert_eq!(
            stamp.len(),
            19,
            "expected 'YYYY-MM-DD HH:MM:SS', got {stamp}"
        );
        chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d %H:%M:%S")
            .unwrap_or_else(|e| panic!("last_sync_at {stamp} is not the TEXT shape: {e}"));
    }

    /// A `None` marker leaves `history_id` alone — the full-sync branch that ran when the
    /// provider had none to record.
    #[tokio::test]
    async fn mark_sync_idle_leaves_the_history_marker_alone_when_there_is_none_to_record() {
        let conn = test_conn().await;
        seed_syncing_account(&conn, "acct-1", 4, Some("hist-1")).await;

        mark_sync_idle(&conn, "acct-1", 2, None)
            .await
            .expect("update");

        let row = sync_row(&conn, "acct-1").await;
        assert_eq!(row.history_id.as_deref(), Some("hist-1"));
        assert_eq!(row.emails_synced, 6);
        assert_eq!(row.status, "idle");
    }

    /// Scoping pin: the write is keyed by account id and must not touch a bystander row.
    #[tokio::test]
    async fn mark_sync_idle_scopes_to_the_target_account() {
        let conn = test_conn().await;
        seed_syncing_account(&conn, "acct-a", 1, None).await;
        seed_syncing_account(&conn, "acct-b", 1, None).await;

        mark_sync_idle(&conn, "acct-a", 5, Some("hist-a"))
            .await
            .expect("update");

        let b = sync_row(&conn, "acct-b").await;
        assert_eq!(b.emails_synced, 1);
        assert_eq!(b.history_id, None);
        assert_eq!(b.last_sync_at, None);
        assert_eq!(b.status, "syncing");
    }
}
