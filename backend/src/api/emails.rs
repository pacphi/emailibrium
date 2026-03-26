//! Email listing endpoints (read from local DB after sync/ingestion).
//!
//! - GET  /api/v1/emails         -- list emails with pagination and filters
//! - GET  /api/v1/emails/:id     -- get a single email by ID

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::debug;

use crate::api::provider_helpers::resolve_provider_and_token;
use crate::email::provider::{FolderOrLabel, MoveKind};
use crate::AppState;

/// Build email API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_emails))
        // Static paths before dynamic /{id} to avoid matching "labels" or "thread" as an id.
        .route("/labels", get(list_account_labels))
        .route("/thread/{thread_id}", get(get_thread))
        .nest("/{id}/attachments", super::attachments::routes())
        .route("/{id}", get(get_email).delete(delete_email))
        .route("/{id}/archive", post(archive_email))
        .route("/{id}/star", post(star_email))
        .route("/{id}/read", post(mark_read_email))
        .route("/{id}/move", post(move_email))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEmailsParams {
    pub account_id: Option<String>,
    pub category: Option<String>,
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
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
}

#[derive(Debug, Serialize)]
pub struct ListEmailsResponse {
    pub emails: Vec<EmailResponse>,
    pub total: i64,
}

const EMAIL_COLUMNS: &str = "id, account_id, provider, message_id, thread_id, subject, \
    from_addr, from_name, to_addrs, cc_addrs, received_at, body_text, body_html, \
    labels, is_read, is_starred, has_attachments, embedding_status, \
    category, category_confidence";

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
        where_parts.push("category = ?".to_string());
    }
    if params.is_read.is_some() {
        where_parts.push("is_read = ?".to_string());
    }
    if params.is_starred.is_some() {
        where_parts.push("is_starred = ?".to_string());
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
    if let Some(v) = params.is_read {
        count_q = count_q.bind(v);
    }
    if let Some(v) = params.is_starred {
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
    if let Some(v) = params.is_read {
        query = query.bind(v);
    }
    if let Some(v) = params.is_starred {
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

/// DELETE /api/v1/emails/:id — delete email from local DB.
async fn delete_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Deleting email");

    // Try to move to trash on provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider
                .move_message(&token, &id, "TRASH", MoveKind::Folder)
                .await
            {
                debug!(email_id = %id, "Provider trash failed (continuing locally): {e}");
            }
        }
    }

    // Clean up attachment files before deleting the email (DB rows cascade).
    let att_paths: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT storage_path FROM attachments WHERE email_id = ?1 AND storage_path IS NOT NULL",
    )
    .bind(&id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for (path,) in &att_paths {
        if let Some(p) = path {
            let _ = tokio::fs::remove_file(p).await; // best-effort
        }
    }
    // Also try to remove the per-email attachment directory.
    let _ = tokio::fs::remove_dir(format!("data/attachments/{}", id)).await;

    let rows = sqlx::query("DELETE FROM emails WHERE id = ?1")
        .bind(&id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Email deleted");
    Ok(StatusCode::NO_CONTENT)
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

    // Update local DB labels.
    match body.kind {
        MoveKind::Folder => {
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
            }
        }
    }

    debug!(email_id = %id, "Email moved");
    Ok(StatusCode::NO_CONTENT)
}
