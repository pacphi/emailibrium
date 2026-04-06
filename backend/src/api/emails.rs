//! Email listing endpoints (read from local DB after sync/ingestion).
//!
//! - GET    /api/v1/emails              -- list emails with pagination and filters
//! - GET    /api/v1/emails/:id          -- get a single email by ID
//! - DELETE /api/v1/emails/:id          -- soft-delete (or permanent with ?permanent=true)
//! - POST   /api/v1/emails/:id/spam    -- mark as spam
//! - POST   /api/v1/emails/:id/unspam  -- remove from spam
//! - POST   /api/v1/emails/:id/restore -- restore from trash
//! - DELETE /api/v1/emails/trash        -- empty trash (permanent delete all)
//! - POST   /api/v1/emails/send         -- compose and send a new email
//! - POST   /api/v1/emails/:id/reply    -- reply to an email
//! - POST   /api/v1/emails/:id/forward  -- forward an email

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::debug;

use crate::api::provider_helpers::resolve_provider_and_token;
use crate::email::provider::{FolderOrLabel, MoveKind, SendDraft};
use crate::AppState;

/// Build email API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_emails))
        // Static paths before dynamic /{id} to avoid matching "labels" or "thread" as an id.
        .route("/labels/all", get(list_all_labels))
        .route("/labels", get(list_account_labels))
        .route("/categories/enriched", get(list_enriched_categories))
        .route("/categories", get(list_categories))
        .route("/counts", get(email_counts))
        .route("/trash", delete(empty_trash))
        .route("/send", post(send_email))
        .route("/thread/{thread_id}", get(get_thread))
        .nest("/{id}/attachments", super::attachments::routes())
        .route("/{id}", get(get_email).delete(delete_email))
        .route("/{id}/archive", post(archive_email))
        .route("/{id}/star", post(star_email))
        .route("/{id}/read", post(mark_read_email))
        .route("/{id}/move", post(move_email))
        .route("/{id}/spam", post(spam_email))
        .route("/{id}/unspam", post(unspam_email))
        .route("/{id}/restore", post(restore_email))
        .route("/{id}/reply", post(reply_email))
        .route("/{id}/forward", post(forward_email))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEmailsParams {
    pub account_id: Option<String>,
    pub category: Option<String>,
    pub label: Option<String>,
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
    pub is_spam: Option<bool>,
    pub is_trash: Option<bool>,
    pub folder: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailResponse {
    pub id: String,
    pub account_id: String,
    pub provider: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub subject: String,
    pub from_addr: String,
    pub from_name: Option<String>,
    pub to_addrs: String,
    pub cc_addrs: Option<String>,
    pub received_at: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub labels: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub embedding_status: String,
    pub category: String,
    pub category_confidence: Option<f64>,
    pub folder: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListEmailsResponse {
    pub emails: Vec<EmailResponse>,
    pub total: i64,
}

const EMAIL_COLUMNS: &str = "id, account_id, provider, message_id, thread_id, subject, \
    from_addr, from_name, to_addrs, cc_addrs, received_at, body_text, body_html, \
    labels, is_read, is_starred, has_attachments, embedding_status, \
    category, category_confidence, folder";

fn row_to_response(row: &sqlx::sqlite::SqliteRow) -> EmailResponse {
    EmailResponse {
        id: row.get("id"),
        account_id: row.get("account_id"),
        provider: row.get("provider"),
        message_id: row.get("message_id"),
        thread_id: row.get("thread_id"),
        subject: row.get("subject"),
        from_addr: row.get("from_addr"),
        from_name: row.get("from_name"),
        to_addrs: row.get("to_addrs"),
        cc_addrs: row.get("cc_addrs"),
        received_at: row.get("received_at"),
        body_text: row.get("body_text"),
        body_html: row.get("body_html"),
        labels: row.get("labels"),
        is_read: row.get::<bool, _>("is_read"),
        is_starred: row.get::<bool, _>("is_starred"),
        has_attachments: row.get::<bool, _>("has_attachments"),
        embedding_status: row.get("embedding_status"),
        category: row
            .get::<Option<String>, _>("category")
            .unwrap_or_else(|| "Uncategorized".to_string()),
        category_confidence: row.get("category_confidence"),
        folder: row.get("folder"),
    }
}

/// GET /api/v1/emails
async fn list_emails(
    State(state): State<AppState>,
    Query(params): Query<ListEmailsParams>,
) -> Result<Json<ListEmailsResponse>, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    // Build WHERE conditions from query params.
    let mut where_parts: Vec<String> = Vec::new();
    if params.account_id.is_some() {
        where_parts.push("account_id = ?".to_string());
    }
    if params.category.is_some() {
        where_parts.push("category = ? COLLATE NOCASE".to_string());
    }
    if params.label.is_some() {
        // Match both raw label name and $-prefixed variant (Gmail user labels use $ prefix).
        where_parts.push(
            "((',' || labels || ',') LIKE '%,' || ? || ',%' \
             OR (',' || labels || ',') LIKE '%,$' || ? || ',%')"
                .to_string(),
        );
    }
    if params.is_read.is_some() {
        where_parts.push("is_read = ?".to_string());
    }
    if params.is_starred.is_some() {
        where_parts.push("is_starred = ?".to_string());
    }
    // Spam/trash: when explicitly requested show only those; otherwise exclude them.
    if let Some(true) = params.is_spam {
        where_parts.push("is_spam = 1".to_string());
    } else if params.is_spam.is_none() {
        where_parts.push("COALESCE(is_spam, 0) = 0".to_string());
    }
    if let Some(true) = params.is_trash {
        where_parts.push("is_trash = 1".to_string());
    } else if params.is_trash.is_none() {
        where_parts.push("COALESCE(is_trash, 0) = 0".to_string());
    }
    if params.folder.is_some() {
        where_parts.push("folder = ? COLLATE NOCASE".to_string());
    }
    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_parts.join(" AND "))
    };

    // Count total.
    let count_sql = format!("SELECT COUNT(*) FROM emails {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(ref v) = params.account_id {
        count_q = count_q.bind(v);
    }
    if let Some(ref v) = params.category {
        count_q = count_q.bind(v);
    }
    if let Some(ref v) = params.label {
        count_q = count_q.bind(v);
        count_q = count_q.bind(v); // bound twice for the OR clause
    }
    if let Some(v) = params.is_read {
        count_q = count_q.bind(v);
    }
    if let Some(v) = params.is_starred {
        count_q = count_q.bind(v);
    }
    if let Some(ref v) = params.folder {
        count_q = count_q.bind(v);
    }
    let total = count_q
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Fetch page.
    let select_sql = format!(
        "SELECT {EMAIL_COLUMNS} FROM emails {where_clause} ORDER BY received_at DESC LIMIT ? OFFSET ?"
    );
    let mut query = sqlx::query(&select_sql);
    if let Some(ref v) = params.account_id {
        query = query.bind(v);
    }
    if let Some(ref v) = params.category {
        query = query.bind(v);
    }
    if let Some(ref v) = params.label {
        query = query.bind(v);
        query = query.bind(v); // bound twice for the OR clause
    }
    if let Some(v) = params.is_read {
        query = query.bind(v);
    }
    if let Some(v) = params.is_starred {
        query = query.bind(v);
    }
    if let Some(ref v) = params.folder {
        query = query.bind(v);
    }
    query = query.bind(limit).bind(offset);

    let rows = query
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let emails = rows.iter().map(row_to_response).collect();

    Ok(Json(ListEmailsResponse { emails, total }))
}

/// GET /api/v1/emails/:id
async fn get_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EmailResponse>, (StatusCode, String)> {
    let sql = format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE id = ?1");
    let row = sqlx::query(&sql)
        .bind(&id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match row {
        Some(r) => Ok(Json(row_to_response(&r))),
        None => Err((StatusCode::NOT_FOUND, "Email not found".to_string())),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResponse {
    pub thread_id: String,
    pub emails: Vec<EmailResponse>,
    pub subject: String,
    pub participants: Vec<String>,
    pub last_activity: String,
}

/// GET /api/v1/emails/thread/:thread_id
async fn get_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, (StatusCode, String)> {
    let sql =
        format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE thread_id = ?1 ORDER BY received_at ASC");
    let rows = sqlx::query(&sql)
        .bind(&thread_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.is_empty() {
        // Fall back: try treating thread_id as a message ID (single-message thread).
        let single_sql = format!("SELECT {EMAIL_COLUMNS} FROM emails WHERE id = ?1");
        let single = sqlx::query(&single_sql)
            .bind(&thread_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        match single {
            Some(r) => {
                let email = row_to_response(&r);
                let subject = email.subject.clone();
                let last_activity = email.received_at.clone();
                let mut participants = vec![email.from_addr.clone()];
                if !email.to_addrs.is_empty() {
                    participants.push(email.to_addrs.clone());
                }
                return Ok(Json(ThreadResponse {
                    thread_id,
                    subject,
                    participants,
                    last_activity,
                    emails: vec![email],
                }));
            }
            None => return Err((StatusCode::NOT_FOUND, "Thread not found".to_string())),
        }
    }

    let emails: Vec<EmailResponse> = rows.iter().map(row_to_response).collect();
    let subject = emails
        .first()
        .map(|e| e.subject.clone())
        .unwrap_or_default();
    let last_activity = emails
        .last()
        .map(|e| e.received_at.clone())
        .unwrap_or_default();

    let mut participants: Vec<String> = emails
        .iter()
        .flat_map(|e| {
            let mut p = vec![e.from_addr.clone()];
            if !e.to_addrs.is_empty() {
                p.push(e.to_addrs.clone());
            }
            p
        })
        .collect();
    participants.sort();
    participants.dedup();

    Ok(Json(ThreadResponse {
        thread_id,
        emails,
        subject,
        participants,
        last_activity,
    }))
}

/// Look up the account_id for an email so we can resolve the provider.
async fn get_email_account_id(
    state: &AppState,
    email_id: &str,
) -> Result<String, (StatusCode, String)> {
    let row: Option<(String,)> = sqlx::query_as("SELECT account_id FROM emails WHERE id = ?1")
        .bind(email_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    row.map(|(aid,)| aid)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Email not found".to_string()))
}

/// POST /api/v1/emails/:id/archive — archive on provider + update local DB.
async fn archive_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Archiving email");

    // Try to archive on the provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider.archive_message(&token, &id).await {
                debug!(email_id = %id, "Provider archive failed (continuing locally): {e}");
            }
        }
    }

    let rows = sqlx::query("UPDATE emails SET labels = 'ARCHIVED' WHERE id = ?1")
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Email archived");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/emails/:id/star — toggle starred on provider + local DB.
async fn star_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Toggling star");

    // Determine new starred state from local DB.
    let current: Option<(bool,)> = sqlx::query_as("SELECT is_starred FROM emails WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let new_starred = !current
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Email not found".to_string()))?
        .0;

    // Star/unstar on the provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider.star_message(&token, &id, new_starred).await {
                debug!(email_id = %id, "Provider star failed (continuing locally): {e}");
            }
        }
    }

    sqlx::query("UPDATE emails SET is_starred = ?1 WHERE id = ?2")
        .bind(new_starred)
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    debug!(email_id = %id, starred = new_starred, "Star toggled");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkReadBody {
    pub read: bool,
}

/// POST /api/v1/emails/:id/read — mark email as read or unread.
async fn mark_read_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MarkReadBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, read = body.read, "Marking email read/unread");

    // Sync to provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider.mark_read(&token, &id, body.read).await {
                debug!(email_id = %id, "Provider mark_read failed (continuing locally): {e}");
            }
        }
    }

    sqlx::query("UPDATE emails SET is_read = ?1 WHERE id = ?2")
        .bind(body.read)
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    debug!(email_id = %id, read = body.read, "Read status updated");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct DeleteEmailParams {
    pub permanent: Option<bool>,
}

/// DELETE /api/v1/emails/:id — soft-delete (default) or permanent delete (?permanent=true).
async fn delete_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<DeleteEmailParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let permanent = params.permanent.unwrap_or(false);
    debug!(email_id = %id, permanent, "Deleting email");

    if permanent {
        // Permanent delete: remove from DB + clean up attachments.
        hard_delete_email(&state, &id).await?;
        debug!(email_id = %id, "Email permanently deleted");
        Ok(Json(serde_json::json!({ "status": "permanently_deleted" })))
    } else {
        // Soft-delete: mark as trash.
        // Try to move to trash on provider (best-effort).
        if let Ok(account_id) = get_email_account_id(&state, &id).await {
            if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await
            {
                if let Err(e) = provider
                    .move_message(&token, &id, "TRASH", MoveKind::Folder)
                    .await
                {
                    debug!(email_id = %id, "Provider trash failed (continuing locally): {e}");
                }
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let rows =
            crate::db::update_email_state(&state.db.pool, &id, true, false, "TRASH", Some(&now))
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if rows == 0 {
            return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
        }
        debug!(email_id = %id, "Email soft-deleted (moved to trash)");
        Ok(Json(serde_json::json!({ "status": "trashed" })))
    }
}

/// Hard-delete a single email: remove attachments then DELETE from DB.
async fn hard_delete_email(state: &AppState, email_id: &str) -> Result<(), (StatusCode, String)> {
    // Clean up attachment files before deleting the email (DB rows cascade).
    let att_paths: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT storage_path FROM attachments WHERE email_id = ?1 AND storage_path IS NOT NULL",
    )
    .bind(email_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for (path,) in &att_paths {
        if let Some(p) = path {
            let _ = tokio::fs::remove_file(p).await; // best-effort
        }
    }
    // Also try to remove the per-email attachment directory.
    let _ = tokio::fs::remove_dir(format!("data/attachments/{}", email_id)).await;

    let rows = sqlx::query("DELETE FROM emails WHERE id = ?1")
        .bind(email_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    Ok(())
}

/// POST /api/v1/emails/:id/spam — mark email as spam.
async fn spam_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!(email_id = %id, "Marking email as spam");

    // Move to spam on provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider
                .move_message(&token, &id, "SPAM", MoveKind::Folder)
                .await
            {
                debug!(email_id = %id, "Provider spam move failed (continuing locally): {e}");
            }
        }
    }

    let rows = crate::db::update_email_state(&state.db.pool, &id, false, true, "SPAM", None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Email marked as spam");
    Ok(Json(serde_json::json!({ "status": "marked_as_spam" })))
}

/// POST /api/v1/emails/:id/unspam — remove email from spam.
async fn unspam_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!(email_id = %id, "Removing email from spam");

    // Move back to inbox on provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider
                .move_message(&token, &id, "INBOX", MoveKind::Folder)
                .await
            {
                debug!(email_id = %id, "Provider unspam move failed (continuing locally): {e}");
            }
        }
    }

    let rows = crate::db::update_email_state(&state.db.pool, &id, false, false, "INBOX", None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Email removed from spam");
    Ok(Json(serde_json::json!({ "status": "removed_from_spam" })))
}

/// POST /api/v1/emails/:id/restore — restore email from trash.
async fn restore_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!(email_id = %id, "Restoring email from trash");

    // Move back to inbox on provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider
                .move_message(&token, &id, "INBOX", MoveKind::Folder)
                .await
            {
                debug!(email_id = %id, "Provider restore move failed (continuing locally): {e}");
            }
        }
    }

    let rows = crate::db::update_email_state(&state.db.pool, &id, false, false, "INBOX", None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Email restored from trash");
    Ok(Json(serde_json::json!({ "status": "restored" })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyTrashParams {
    pub account_id: Option<String>,
}

/// DELETE /api/v1/emails/trash — permanently delete all trashed emails.
async fn empty_trash(
    State(state): State<AppState>,
    Query(params): Query<EmptyTrashParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!("Emptying trash");

    // Find all trashed emails, optionally filtered by account.
    let (sql, ids): (_, Vec<String>) = if let Some(ref account_id) = params.account_id {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM emails WHERE is_trash = 1 AND account_id = ?1")
                .bind(account_id)
                .fetch_all(&state.db.pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        (
            "DELETE FROM emails WHERE is_trash = 1 AND account_id = ?1".to_string(),
            rows.into_iter().map(|(id,)| id).collect(),
        )
    } else {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM emails WHERE is_trash = 1")
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        (
            "DELETE FROM emails WHERE is_trash = 1".to_string(),
            rows.into_iter().map(|(id,)| id).collect(),
        )
    };

    // Clean up attachment files for each trashed email.
    for email_id in &ids {
        let att_paths: Vec<(Option<String>,)> = sqlx::query_as(
            "SELECT storage_path FROM attachments WHERE email_id = ?1 AND storage_path IS NOT NULL",
        )
        .bind(email_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        for (path,) in &att_paths {
            if let Some(p) = path {
                let _ = tokio::fs::remove_file(p).await;
            }
        }
        let _ = tokio::fs::remove_dir(format!("data/attachments/{}", email_id)).await;
    }

    // Hard delete all trashed emails.
    let result = if let Some(account_id) = &params.account_id {
        sqlx::query(&sql)
            .bind(account_id)
            .execute(&state.db.pool)
            .await
    } else {
        sqlx::query(&sql).execute(&state.db.pool).await
    };

    let rows = result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let deleted_count = rows.rows_affected();

    debug!(deleted_count, "Trash emptied");
    Ok(Json(serde_json::json!({ "deleted_count": deleted_count })))
}

// --- Categories endpoint ---

#[derive(Debug, Serialize)]
pub struct CategoriesResponse {
    pub categories: Vec<String>,
}

/// GET /api/v1/emails/categories — list distinct email categories.
async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<CategoriesResponse>, (StatusCode, String)> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT category FROM emails WHERE category IS NOT NULL AND category != 'Uncategorized' ORDER BY category",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let categories = rows.into_iter().map(|(c,)| c).collect();
    Ok(Json(CategoriesResponse { categories }))
}

// --- Labels / Move endpoints ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLabelsParams {
    pub account_id: String,
}

/// GET /api/v1/emails/labels?accountId=X — list folders and labels for an account.
async fn list_account_labels(
    State(state): State<AppState>,
    Query(params): Query<ListLabelsParams>,
) -> Result<Json<Vec<FolderOrLabel>>, (StatusCode, String)> {
    debug!(account_id = %params.account_id, "Listing folders/labels");

    let (provider, token, _) = resolve_provider_and_token(&state, &params.account_id).await?;
    let labels = provider.list_folders(&token).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to list folders: {e}"),
        )
    })?;

    debug!(account_id = %params.account_id, count = labels.len(), "Listed folders/labels");
    Ok(Json(labels))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveEmailBody {
    pub account_id: String,
    pub target_id: String,
    pub kind: MoveKind,
}

/// POST /api/v1/emails/:id/move — move email to a folder or add a label.
async fn move_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveEmailBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(
        email_id = %id,
        target = %body.target_id,
        kind = ?body.kind,
        "Moving email"
    );

    let (provider, token, _) = resolve_provider_and_token(&state, &body.account_id).await?;

    // Move on the provider.
    provider
        .move_message(&token, &id, &body.target_id, body.kind.clone())
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Move failed: {e}")))?;

    // Update local DB labels and state columns.
    match body.kind {
        MoveKind::Folder => {
            // Derive state from the target folder name.
            let target_upper = body.target_id.to_uppercase();
            let (is_trash, is_spam, folder) = match target_upper.as_str() {
                "TRASH" => (true, false, "TRASH"),
                "SPAM" => (false, true, "SPAM"),
                "INBOX" => (false, false, "INBOX"),
                "SENT" => (false, false, "SENT"),
                "DRAFT" | "DRAFTS" => (false, false, "DRAFT"),
                _ => (false, false, "INBOX"),
            };
            crate::db::update_email_state(&state.db.pool, &id, is_trash, is_spam, folder, None)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Also update the labels column.
            sqlx::query("UPDATE emails SET labels = ?1 WHERE id = ?2")
                .bind(&body.target_id)
                .bind(&id)
                .execute(&state.db.pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        MoveKind::Label => {
            // Append label (avoid duplicates with a read-modify-write).
            let current: Option<(String,)> =
                sqlx::query_as("SELECT labels FROM emails WHERE id = ?1")
                    .bind(&id)
                    .fetch_optional(&state.db.pool)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            if let Some((labels,)) = current {
                let mut label_set: Vec<&str> =
                    labels.split(',').filter(|s| !s.is_empty()).collect();
                if !label_set.iter().any(|l| *l == body.target_id) {
                    label_set.push(&body.target_id);
                }
                let new_labels = label_set.join(",");
                sqlx::query("UPDATE emails SET labels = ?1 WHERE id = ?2")
                    .bind(&new_labels)
                    .bind(&id)
                    .execute(&state.db.pool)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Derive state from the combined labels.
                let label_vec: Vec<String> = new_labels.split(',').map(|s| s.to_string()).collect();
                let (is_trash, is_spam, folder) = crate::db::derive_state_from_labels(&label_vec);
                crate::db::update_email_state(&state.db.pool, &id, is_trash, is_spam, folder, None)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            }
        }
    }

    debug!(email_id = %id, "Email moved");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Gap 4: Cross-account label aggregation
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedLabel {
    pub name: String,
    pub kind: String,
    pub is_system: bool,
    pub email_count: u64,
    pub unread_count: u64,
    pub account_ids: Vec<String>,
}

/// Convert a label string to Title Case for display.
/// "NEWSLETTERS" → "Newsletters", "$FINANCE" → "Finance", "CATEGORY_SOCIAL" → "Social"
fn to_title_case(s: &str) -> String {
    // Strip leading special chars and known prefixes.
    let cleaned = s
        .trim_start_matches('$')
        .strip_prefix("CATEGORY_")
        .unwrap_or(s.trim_start_matches('$'));

    if cleaned.is_empty() {
        return s.to_string();
    }

    // If already mixed case (e.g. "Newsletters"), keep as-is.
    let has_lower = cleaned.chars().any(|c| c.is_lowercase());
    if has_lower {
        return cleaned.to_string();
    }

    // ALL CAPS → Title Case
    let mut result = String::with_capacity(cleaned.len());
    let mut capitalize_next = true;
    for ch in cleaned.chars() {
        if ch == '_' || ch == '-' {
            result.push(' ');
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.extend(ch.to_lowercase());
        }
    }
    result
}

/// GET /api/v1/emails/labels/all — aggregate labels across all accounts.
async fn list_all_labels(
    State(state): State<AppState>,
) -> Result<Json<Vec<AggregatedLabel>>, (StatusCode, String)> {
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT labels, account_id, is_read FROM emails \
         WHERE labels IS NOT NULL AND labels != '' \
         AND COALESCE(is_spam, 0) = 0 AND COALESCE(is_trash, 0) = 0",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    use std::collections::HashMap;

    struct LabelAgg {
        display_name: String,
        email_count: u64,
        unread_count: u64,
        account_ids: std::collections::HashSet<String>,
    }

    let mut agg: HashMap<String, LabelAgg> = HashMap::new();

    for (labels_csv, account_id, is_read) in &rows {
        for label in labels_csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let key = label.to_uppercase();
            let entry = agg.entry(key).or_insert_with(|| LabelAgg {
                display_name: to_title_case(label),
                email_count: 0,
                unread_count: 0,
                account_ids: std::collections::HashSet::new(),
            });
            entry.email_count += 1;
            if !is_read {
                entry.unread_count += 1;
            }
            entry.account_ids.insert(account_id.clone());
        }
    }

    const SYSTEM_LABELS: &[&str] = &[
        "INBOX",
        "SENT",
        "TRASH",
        "SPAM",
        "STARRED",
        "DRAFT",
        "IMPORTANT",
        "UNREAD",
        "CATEGORY_SOCIAL",
        "CATEGORY_PROMOTIONS",
        "CATEGORY_UPDATES",
        "CATEGORY_FORUMS",
        "CATEGORY_PERSONAL",
    ];

    let mut result: Vec<AggregatedLabel> = agg
        .into_iter()
        .map(|(key, a)| {
            let is_system = SYSTEM_LABELS.contains(&key.as_str());
            AggregatedLabel {
                name: a.display_name,
                kind: "label".to_string(),
                is_system,
                email_count: a.email_count,
                unread_count: a.unread_count,
                account_ids: a.account_ids.into_iter().collect(),
            }
        })
        .collect();

    result.sort_by(|a, b| b.email_count.cmp(&a.email_count));
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// Gap 5: Enriched categories
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichedCategory {
    pub name: String,
    pub group: String,
    pub email_count: u64,
    pub unread_count: u64,
}

pub fn categorize_group(category: &str) -> &'static str {
    match category.to_lowercase().as_str() {
        "newsletter" | "marketing" | "promotions" => "subscription",
        "alerts" | "notification" => "system",
        "travel" => "category",
        _ => "category",
    }
}

/// GET /api/v1/emails/categories/enriched — categories with group and counts.
async fn list_enriched_categories(
    State(state): State<AppState>,
) -> Result<Json<Vec<EnrichedCategory>>, (StatusCode, String)> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) as total, \
         SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END) as unread \
         FROM emails \
         WHERE category IS NOT NULL AND category != 'Uncategorized' \
         AND COALESCE(is_spam, 0) = 0 AND COALESCE(is_trash, 0) = 0 \
         GROUP BY category ORDER BY total DESC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let enriched = rows
        .into_iter()
        .map(|(name, total, unread)| EnrichedCategory {
            group: categorize_group(&name).to_string(),
            name,
            email_count: total as u64,
            unread_count: unread as u64,
        })
        .collect();

    Ok(Json(enriched))
}

// ---------------------------------------------------------------------------
// Gap 6: Accurate email counts
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCounts {
    pub total: u64,
    pub unread: u64,
    pub spam_count: u64,
    pub trash_count: u64,
    pub sent_count: u64,
    pub by_category: Vec<CategoryCount>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub category: String,
    pub total: u64,
    pub unread: u64,
}

/// GET /api/v1/emails/counts — accurate total, unread, spam, trash, and per-category counts.
async fn email_counts(
    State(state): State<AppState>,
) -> Result<Json<EmailCounts>, (StatusCode, String)> {
    // Total and unread (excluding spam/trash).
    let (total, unread): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(COUNT(*), 0), \
         COALESCE(SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END), 0) \
         FROM emails \
         WHERE COALESCE(is_spam, 0) = 0 AND COALESCE(is_trash, 0) = 0",
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Spam and trash counts.
    let (spam_count,): (i64,) =
        sqlx::query_as("SELECT COALESCE(COUNT(*), 0) FROM emails WHERE is_spam = 1")
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (trash_count,): (i64,) =
        sqlx::query_as("SELECT COALESCE(COUNT(*), 0) FROM emails WHERE is_trash = 1")
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (sent_count,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(COUNT(*), 0) FROM emails WHERE folder = 'SENT' \
         AND COALESCE(is_spam, 0) = 0 AND COALESCE(is_trash, 0) = 0",
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Per-category (excluding spam/trash).
    let cat_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT COALESCE(category, 'Uncategorized'), COUNT(*), \
         SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END) \
         FROM emails \
         WHERE COALESCE(is_spam, 0) = 0 AND COALESCE(is_trash, 0) = 0 \
         GROUP BY category ORDER BY COUNT(*) DESC",
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let by_category = cat_rows
        .into_iter()
        .map(|(cat, t, u)| CategoryCount {
            category: cat,
            total: t as u64,
            unread: u as u64,
        })
        .collect();

    Ok(Json(EmailCounts {
        total: total as u64,
        unread: unread as u64,
        spam_count: spam_count as u64,
        trash_count: trash_count as u64,
        sent_count: sent_count as u64,
        by_category,
    }))
}

// ---------------------------------------------------------------------------
// Send / Reply / Forward
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendEmailRequest {
    to: String,
    cc: Option<String>,
    bcc: Option<String>,
    subject: String,
    body_text: Option<String>,
    body_html: Option<String>,
    account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendEmailResponse {
    message_id: String,
}

/// POST /api/v1/emails/send — compose and send a new email.
async fn send_email(
    State(state): State<AppState>,
    Json(req): Json<SendEmailRequest>,
) -> Result<Json<SendEmailResponse>, (StatusCode, String)> {
    debug!(account_id = %req.account_id, to = %req.to, "Sending new email");

    let (provider, token, _) = resolve_provider_and_token(&state, &req.account_id).await?;
    let draft = SendDraft {
        to: &req.to,
        cc: req.cc.as_deref(),
        bcc: req.bcc.as_deref(),
        subject: &req.subject,
        body_text: req.body_text.as_deref(),
        body_html: req.body_html.as_deref(),
    };
    let message_id = provider
        .send_message(&token, &draft)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Send failed: {e}")))?;

    // Trigger a delta sync so the sent message appears in the local DB via the
    // canonical ingestion path — avoids duplicate rows from parallel inserts.
    let _ = super::ingestion::sync_emails_from_provider(&state, &req.account_id).await;

    Ok(Json(SendEmailResponse { message_id }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyEmailRequest {
    body_text: Option<String>,
    body_html: Option<String>,
}

/// POST /api/v1/emails/:id/reply — reply to an email.
async fn reply_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ReplyEmailRequest>,
) -> Result<Json<SendEmailResponse>, (StatusCode, String)> {
    debug!(email_id = %id, "Replying to email");

    let account_id = get_email_account_id(&state, &id).await?;
    let (provider, token, _) = resolve_provider_and_token(&state, &account_id).await?;

    let message_id = provider
        .reply_to_message(
            &token,
            &id,
            req.body_text.as_deref(),
            req.body_html.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Reply failed: {e}")))?;

    // Trigger a delta sync so the sent reply appears via the canonical ingestion path.
    let _ = super::ingestion::sync_emails_from_provider(&state, &account_id).await;

    Ok(Json(SendEmailResponse { message_id }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForwardEmailRequest {
    to: String,
}

/// POST /api/v1/emails/:id/forward — forward an email.
async fn forward_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ForwardEmailRequest>,
) -> Result<Json<SendEmailResponse>, (StatusCode, String)> {
    debug!(email_id = %id, to = %req.to, "Forwarding email");

    let account_id = get_email_account_id(&state, &id).await?;
    let (provider, token, _) = resolve_provider_and_token(&state, &account_id).await?;

    let message_id = provider
        .forward_message(&token, &id, &req.to)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Forward failed: {e}")))?;

    // Trigger a delta sync so the forwarded message appears via the canonical ingestion path.
    let _ = super::ingestion::sync_emails_from_provider(&state, &account_id).await;

    Ok(Json(SendEmailResponse { message_id }))
}
