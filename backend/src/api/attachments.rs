//! Attachment API endpoints.
//!
//! - GET  /api/v1/emails/{email_id}/attachments          -- list metadata
//! - GET  /api/v1/emails/{email_id}/attachments/zip      -- download all as ZIP
//! - GET  /api/v1/emails/{email_id}/attachments/{att_id} -- download single (lazy fetch)

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio_util::io::ReaderStream;
use tracing::{debug, warn};

use crate::api::provider_helpers::resolve_provider_and_token;
use crate::email::types::ProviderKind;
use crate::AppState;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Build attachment sub-routes (nested under `/emails/{email_id}/attachments`).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_attachments))
        .route("/zip", get(download_all_zip))
        .route("/{att_id}", get(download_attachment))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAttachmentsParams {
    /// When true, include inline (CID) attachments in the response.
    pub include_inline: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentResponse {
    pub id: String,
    pub email_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub is_inline: bool,
    pub fetch_status: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strip dangerous characters from a filename for safe filesystem storage.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
        .collect();
    let cleaned = cleaned.trim_matches('.');
    if cleaned.is_empty() {
        return format!("attachment-{}", uuid::Uuid::new_v4());
    }
    if cleaned.len() > 200 {
        cleaned[..200].to_string()
    } else {
        cleaned.to_string()
    }
}

/// Resolve the `message_id` (provider-side) for an email so we can call the
/// provider attachment API.  Falls back to the email `id` itself when there is
/// no separate `message_id` column value.
async fn get_email_message_id(
    state: &AppState,
    email_id: &str,
) -> Result<String, (StatusCode, String)> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT message_id FROM emails WHERE id = ?1")
            .bind(email_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match row {
        Some((Some(mid),)) if !mid.is_empty() => Ok(mid),
        Some(_) => Ok(email_id.to_string()),
        None => Err((StatusCode::NOT_FOUND, "Email not found".to_string())),
    }
}

/// Lazy-fetch a single attachment from the provider, cache to disk, and update
/// the DB record.  Returns the on-disk path.
async fn lazy_fetch_attachment(
    state: &AppState,
    att_id: &str,
    email_id: &str,
    account_id: &str,
    provider_attachment_id: Option<&str>,
    filename: &str,
) -> Result<String, (StatusCode, String)> {
    let (_, token, kind) = resolve_provider_and_token(state, account_id).await?;
    let message_id = get_email_message_id(state, email_id).await?;

    let bytes = match kind {
        ProviderKind::Gmail => {
            fetch_gmail_attachment(&token, &message_id, att_id, provider_attachment_id).await?
        }
        ProviderKind::Outlook => {
            fetch_outlook_attachment(&token, &message_id, att_id, provider_attachment_id).await?
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Attachment download not supported for this provider".to_string(),
            ));
        }
    };

    // Write to filesystem.
    let safe_name = sanitize_filename(filename);
    let dir = format!("data/attachments/{account_id}/{email_id}");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {e}")))?;

    let path = format!("{dir}/{safe_name}");
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write: {e}")))?;

    // Update DB.
    sqlx::query("UPDATE attachments SET fetch_status = 'fetched', storage_path = ?1 WHERE id = ?2")
        .bind(&path)
        .bind(att_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(path)
}

/// Fetch attachment bytes from the Gmail API.
async fn fetch_gmail_attachment(
    token: &str,
    message_id: &str,
    att_id: &str,
    provider_attachment_id: Option<&str>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    use base64::Engine;

    let pid = provider_attachment_id.unwrap_or(att_id);
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}/attachments/{pid}"
    );

    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Gmail fetch: {e}")))?
        .error_for_status()
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Gmail status: {e}")))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Gmail json: {e}")))?;

    let data_str = resp["data"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_GATEWAY, "Missing data field".to_string()))?;

    // Gmail returns base64url-encoded data.
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data_str)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("base64 decode: {e}")))
}

/// Fetch attachment bytes from the Outlook / Graph API.
async fn fetch_outlook_attachment(
    token: &str,
    message_id: &str,
    att_id: &str,
    provider_attachment_id: Option<&str>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let pid = provider_attachment_id.unwrap_or(att_id);
    let url = format!(
        "https://graph.microsoft.com/v1.0/me/messages/{message_id}/attachments/{pid}/$value"
    );

    let client = reqwest::Client::new();
    let bytes = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Outlook fetch: {e}")))?
        .error_for_status()
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Outlook status: {e}")))?
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Outlook bytes: {e}")))?;

    Ok(bytes.to_vec())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/emails/{email_id}/attachments
async fn list_attachments(
    State(state): State<AppState>,
    Path(email_id): Path<String>,
    Query(params): Query<ListAttachmentsParams>,
) -> Result<Json<Vec<AttachmentResponse>>, (StatusCode, String)> {
    let include_inline = params.include_inline.unwrap_or(false);

    let sql = if include_inline {
        "SELECT id, email_id, filename, content_type, size_bytes, is_inline, fetch_status \
         FROM attachments WHERE email_id = ?1 ORDER BY filename"
    } else {
        "SELECT id, email_id, filename, content_type, size_bytes, is_inline, fetch_status \
         FROM attachments WHERE email_id = ?1 AND is_inline = FALSE ORDER BY filename"
    };

    let rows = sqlx::query(sql)
        .bind(&email_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let attachments = rows
        .iter()
        .map(|r| AttachmentResponse {
            id: r.get("id"),
            email_id: r.get("email_id"),
            filename: r.get("filename"),
            content_type: r.get("content_type"),
            size_bytes: r.get("size_bytes"),
            is_inline: r.get::<bool, _>("is_inline"),
            fetch_status: r.get("fetch_status"),
        })
        .collect();

    Ok(Json(attachments))
}

/// GET /api/v1/emails/{email_id}/attachments/{att_id}
async fn download_attachment(
    State(state): State<AppState>,
    Path((email_id, att_id)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    debug!(email_id = %email_id, att_id = %att_id, "Downloading attachment");

    let row = sqlx::query(
        "SELECT id, email_id, account_id, filename, content_type, \
         storage_path, fetch_status, provider_attachment_id, is_inline \
         FROM attachments WHERE id = ?1 AND email_id = ?2",
    )
    .bind(&att_id)
    .bind(&email_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "Attachment not found".to_string()))?;

    let filename: String = row.get("filename");
    let content_type: String = row.get("content_type");
    let fetch_status: String = row.get("fetch_status");
    let storage_path: Option<String> = row.get("storage_path");
    let account_id: String = row.get("account_id");
    let provider_att_id: Option<String> = row.get("provider_attachment_id");

    // Determine on-disk path, lazy-fetching if necessary.
    let path = match (fetch_status.as_str(), storage_path) {
        ("fetched", Some(ref p)) if tokio::fs::try_exists(p).await.unwrap_or(false) => p.clone(),
        _ => {
            lazy_fetch_attachment(
                &state,
                &att_id,
                &email_id,
                &account_id,
                provider_att_id.as_deref(),
                &filename,
            )
            .await?
        }
    };

    stream_file(&path, &content_type, &filename).await
}

/// Stream a file from disk as an HTTP response with correct headers.
async fn stream_file(
    path: &str,
    content_type: &str,
    filename: &str,
) -> Result<Response, (StatusCode, String)> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("open: {e}")))?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let safe = sanitize_filename(filename);
    let disposition = format!("attachment; filename=\"{safe}\"");

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, &disposition)
        .body(body)
        .unwrap())
}

/// GET /api/v1/emails/{email_id}/attachments/zip
async fn download_all_zip(
    State(state): State<AppState>,
    Path(email_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    debug!(email_id = %email_id, "Downloading all attachments as ZIP");

    // Load all non-inline attachments.
    let rows = sqlx::query(
        "SELECT id, email_id, account_id, filename, content_type, \
         storage_path, fetch_status, provider_attachment_id \
         FROM attachments WHERE email_id = ?1 AND is_inline = FALSE ORDER BY filename",
    )
    .bind(&email_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, "No attachments found".to_string()));
    }

    // Ensure all attachments are fetched; collect (path, filename) pairs.
    let mut entries: Vec<(String, String)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let att_id: String = row.get("id");
        let filename: String = row.get("filename");
        let account_id: String = row.get("account_id");
        let fetch_status: String = row.get("fetch_status");
        let storage_path: Option<String> = row.get("storage_path");
        let provider_att_id: Option<String> = row.get("provider_attachment_id");

        let path = match (fetch_status.as_str(), storage_path) {
            ("fetched", Some(ref p)) if tokio::fs::try_exists(p).await.unwrap_or(false) => {
                p.clone()
            }
            _ => {
                lazy_fetch_attachment(
                    &state,
                    &att_id,
                    &email_id,
                    &account_id,
                    provider_att_id.as_deref(),
                    &filename,
                )
                .await?
            }
        };
        entries.push((path, sanitize_filename(&filename)));
    }

    // Build the ZIP via a duplex channel so we can stream the response.
    let (writer, reader) = tokio::io::duplex(64 * 1024);
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    tokio::spawn(async move {
        if let Err(e) = write_zip(writer, entries).await {
            warn!("ZIP write error: {e}");
        }
    });

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"attachments-{email_id}.zip\""),
        )
        .body(body)
        .unwrap())
}

/// Write all files into a ZIP archive on the provided async writer.
async fn write_zip(
    writer: tokio::io::DuplexStream,
    entries: Vec<(String, String)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use async_zip::tokio::write::ZipFileWriter;
    use async_zip::{Compression, ZipEntryBuilder};
    use tokio_util::compat::TokioAsyncWriteCompatExt;

    let mut zip = ZipFileWriter::new(writer.compat_write());

    for (path, filename) in &entries {
        let data = tokio::fs::read(path).await?;
        let entry = ZipEntryBuilder::new(filename.clone().into(), Compression::Deflate);
        zip.write_entry_whole(entry, &data).await?;
    }

    zip.close().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
    }

    #[test]
    fn test_sanitize_filename_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
    }

    #[test]
    fn test_sanitize_filename_strips_dangerous_chars() {
        assert_eq!(sanitize_filename("file<name>.txt"), "filename.txt");
        assert_eq!(sanitize_filename("a:b|c*d?e"), "abcde");
    }

    #[test]
    fn test_sanitize_filename_empty() {
        let result = sanitize_filename("");
        assert!(result.starts_with("attachment-"));
    }

    #[test]
    fn test_sanitize_filename_only_dots() {
        let result = sanitize_filename("...");
        assert!(result.starts_with("attachment-"));
    }

    #[test]
    fn test_sanitize_filename_long_name() {
        let long = "a".repeat(300);
        let result = sanitize_filename(&long);
        assert_eq!(result.len(), 200);
    }

    #[test]
    fn test_sanitize_filename_unicode() {
        assert_eq!(sanitize_filename("resume.pdf"), "resume.pdf");
    }

    #[test]
    fn test_sanitize_filename_backslash() {
        assert_eq!(sanitize_filename("dir\\file.txt"), "dirfile.txt");
    }
}
