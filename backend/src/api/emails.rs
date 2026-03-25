//! Email listing endpoints (read from local DB after sync/ingestion).
//!
//! - GET  /api/v1/emails         -- list emails with pagination and filters
//! - GET  /api/v1/emails/:id     -- get a single email by ID

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::debug;

use crate::AppState;

/// Build email API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_emails))
        .route("/{id}", get(get_email).delete(delete_email))
        .route("/{id}/archive", post(archive_email))
        .route("/{id}/star", post(star_email))
        .route("/thread/{thread_id}", get(get_thread))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEmailsParams {
    pub account_id: Option<String>,
    pub category: Option<String>,
    pub is_read: Option<bool>,
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
    let sql = format!(
        "SELECT {EMAIL_COLUMNS} FROM emails WHERE thread_id = ?1 ORDER BY received_at ASC"
    );
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

/// POST /api/v1/emails/:id/archive — mark email as archived (remove from inbox view).
async fn archive_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Archiving email");
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

/// POST /api/v1/emails/:id/star — toggle the starred status.
async fn star_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Toggling star");
    let rows =
        sqlx::query("UPDATE emails SET is_starred = NOT is_starred WHERE id = ?1")
            .bind(&id)
            .execute(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
    }
    debug!(email_id = %id, "Star toggled");
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/emails/:id — delete email from local DB.
async fn delete_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    debug!(email_id = %id, "Deleting email");
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
