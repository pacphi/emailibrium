//! Similar-email search and attachment metadata tools.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::params::{
    CountEmailsRequest, FindSimilarEmailsRequest, GetEmailRequest, GetEmailThreadRequest,
    ListAttachmentsRequest, ListRecentEmailsRequest, SearchEmailsRequest,
};
use super::{validate_date, validate_id, validate_limit};
use crate::tools::{ToolContext, ToolError};
use crate::vectors::search::{HybridSearchQuery, SearchMode};
use crate::vectors::types::SearchParams;

/// Row shape for the enrichment lookup: id, subject, from_addr, from_name,
/// received_at, category.
type EmailMetaRow = (String, String, String, Option<String>, String, String);

/// Find emails semantically similar to a given email.
///
/// Resolves the source email's stored embedding and searches the same
/// collection, excluding the source itself. Results are enriched from SQLite
/// so callers get subjects and senders rather than bare IDs.
pub async fn find_similar_emails(
    ctx: Arc<ToolContext>,
    req: FindSimilarEmailsRequest,
) -> Result<Value, ToolError> {
    validate_id("email_id", &req.email_id).map_err(ToolError::Invalid)?;
    let limit = validate_limit(req.limit.unwrap_or(10), 50) as usize;
    let min_score = req.min_score.unwrap_or(0.5).clamp(0.0, 1.0);

    let store = &ctx.vectors()?.store;

    let existing = match store.get_by_email_id(&req.email_id).await {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            return Err(ToolError::NotFound(format!(
                "No embedding found for email_id: {}",
                req.email_id
            )))
        }
        Err(e) => return Err(super::db_error("Vector lookup", e)),
    };

    // Over-fetch by one so excluding the source email cannot short the result set.
    let params = SearchParams {
        vector: existing.vector,
        limit: limit + 1,
        collection: existing.collection,
        filters: None,
        min_score: Some(min_score),
    };

    let results = store
        .search(&params)
        .await
        .map_err(|e| super::db_error("Similarity search", e))?;

    let scored: Vec<(String, f32)> = results
        .iter()
        .filter(|r| r.document.email_id != req.email_id)
        .take(limit)
        .map(|r| (r.document.email_id.clone(), r.score))
        .collect();

    let ids: Vec<String> = scored.iter().map(|(id, _)| id.clone()).collect();
    let meta = fetch_email_meta(&ctx, &ids).await;

    let items: Vec<Value> = scored
        .iter()
        .map(|(id, score)| match meta.get(id) {
            Some((subject, from_addr, from_name, received_at, category)) => {
                let sender = match from_name {
                    Some(name) if !name.is_empty() => format!("{name} <{from_addr}>"),
                    _ => from_addr.clone(),
                };
                json!({
                    "email_id": id,
                    "score": score,
                    "subject": subject,
                    "from": sender,
                    "date": received_at,
                    "category": category,
                })
            }
            None => json!({ "email_id": id, "score": score }),
        })
        .collect();

    Ok(json!({
        "source_email_id": req.email_id,
        "count": items.len(),
        "min_score": min_score,
        "results": items,
    }))
}

/// Batch-load display metadata for a set of email IDs.
///
/// Returns an empty map on error: enrichment is a nicety, and a failure here
/// should degrade the result to bare IDs rather than fail the whole tool.
pub(super) async fn fetch_email_meta(
    ctx: &ToolContext,
    ids: &[String],
) -> HashMap<String, (String, String, Option<String>, String, String)> {
    if ids.is_empty() {
        return HashMap::new();
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT id, subject, from_addr, from_name, received_at, category \
         FROM emails WHERE id IN ({})",
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, EmailMetaRow>(crate::db::audited_sql(&sql));
    for id in ids {
        query = query.bind(id);
    }

    query
        .fetch_all(ctx.pool())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(
            |(id, subject, from_addr, from_name, received_at, category)| {
                (id, (subject, from_addr, from_name, received_at, category))
            },
        )
        .collect()
}

/// List attachment metadata for an email.
///
/// Metadata only — no file contents, and neither the on-disk `storage_path`
/// nor the `provider_attachment_id` is exposed. Use the REST download
/// endpoint to fetch bytes.
pub async fn list_attachments(
    ctx: Arc<ToolContext>,
    req: ListAttachmentsRequest,
) -> Result<Value, ToolError> {
    validate_id("email_id", &req.email_id).map_err(ToolError::Invalid)?;
    let include_inline = req.include_inline.unwrap_or(false);

    // Two static statements rather than a built string: no dynamic SQL here.
    let sql = if include_inline {
        "SELECT id, email_id, filename, content_type, size_bytes, is_inline, fetch_status \
         FROM attachments WHERE email_id = ?1 ORDER BY filename"
    } else {
        "SELECT id, email_id, filename, content_type, size_bytes, is_inline, fetch_status \
         FROM attachments WHERE email_id = ?1 AND is_inline = FALSE ORDER BY filename"
    };

    let rows = sqlx::query_as::<_, (String, String, String, String, i64, bool, String)>(sql)
        .bind(&req.email_id)
        .fetch_all(ctx.pool())
        .await
        .map_err(|e| super::db_error("Listing attachments", e))?;

    let total_bytes: i64 = rows.iter().map(|r| r.4).sum();
    let items: Vec<Value> = rows
        .iter()
        .map(
            |(id, email_id, filename, content_type, size_bytes, is_inline, fetch_status)| {
                json!({
                    "id": id,
                    "email_id": email_id,
                    "filename": filename,
                    "content_type": content_type,
                    "size_bytes": size_bytes,
                    "is_inline": is_inline,
                    "fetch_status": fetch_status,
                })
            },
        )
        .collect();

    Ok(json!({
        "email_id": req.email_id,
        "count": items.len(),
        "total_bytes": total_bytes,
        "attachments": items,
    }))
}

// ---------------------------------------------------------------------------
// The five pre-existing email tools, moved from `mcp/server.rs`
// ---------------------------------------------------------------------------
//
// Split into a fetch layer and a handler layer (design §2.1.1). The A5 resource
// handlers bind to the fetch functions, which return typed records and tell
// "no such email" apart from "the query failed" — a distinction the old
// `#[tool]` methods erased by returning both as `{"error": ...}` strings.

/// Upper bound on `search_emails`' free-text query.
const MAX_QUERY_LEN: usize = 1000;

/// One email with its body, as `get_email` and `email://{id}` return it.
#[derive(Debug, serde::Serialize)]
pub struct EmailRecord {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub category: String,
    pub body: String,
}

/// One email in a listing. No body, so a long thread stays a small payload.
#[derive(Debug, serde::Serialize)]
pub struct ThreadEmail {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub category: String,
}

#[derive(Debug, sqlx::FromRow)]
struct EmailRow {
    id: String,
    subject: String,
    from_name: Option<String>,
    from_addr: String,
    received_at: String,
    body_text: Option<String>,
    category: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ThreadEmailRow {
    id: String,
    subject: String,
    from_name: Option<String>,
    from_addr: String,
    received_at: String,
    category: String,
}

/// Render a display sender, preferring `Name <addr>` when a name is stored.
fn format_sender(from_name: Option<&String>, from_addr: &str) -> String {
    match from_name {
        Some(name) if !name.is_empty() => format!("{name} <{from_addr}>"),
        _ => from_addr.to_string(),
    }
}

fn thread_email(row: ThreadEmailRow) -> ThreadEmail {
    ThreadEmail {
        from: format_sender(row.from_name.as_ref(), &row.from_addr),
        id: row.id,
        subject: row.subject,
        date: row.received_at,
        category: row.category,
    }
}

/// Load one email including its body.
pub async fn fetch_email(ctx: &ToolContext, email_id: &str) -> Result<EmailRecord, ToolError> {
    validate_id("email_id", email_id).map_err(ToolError::Invalid)?;

    let row = sqlx::query_as::<_, EmailRow>(
        "SELECT id, subject, from_name, from_addr, received_at, body_text, category \
         FROM emails WHERE id = ?1",
    )
    .bind(email_id)
    .fetch_optional(ctx.pool())
    .await
    .map_err(|e| super::db_error("Email lookup", e))?
    .ok_or_else(|| ToolError::NotFound("Email not found".to_string()))?;

    Ok(EmailRecord {
        from: format_sender(row.from_name.as_ref(), &row.from_addr),
        id: row.id,
        subject: row.subject,
        date: row.received_at,
        category: row.category,
        body: row.body_text.unwrap_or_default(),
    })
}

/// Resolve the thread an email belongs to.
///
/// The `get_email_thread` tool is keyed by email id and needs this hop; the
/// `thread://{key}` resource carries the thread key already and calls
/// [`fetch_thread_by_key`] directly.
pub async fn resolve_thread_key(ctx: &ToolContext, email_id: &str) -> Result<String, ToolError> {
    validate_id("email_id", email_id).map_err(ToolError::Invalid)?;

    sqlx::query_scalar::<_, String>("SELECT thread_key FROM emails WHERE id = ?1")
        .bind(email_id)
        .fetch_optional(ctx.pool())
        .await
        .map_err(|e| super::db_error("Thread lookup", e))?
        .ok_or_else(|| ToolError::NotFound("Email not found".to_string()))
}

/// Every email sharing a thread key, oldest first.
pub async fn fetch_thread_by_key(
    ctx: &ToolContext,
    thread_key: &str,
) -> Result<Vec<ThreadEmail>, ToolError> {
    let rows = sqlx::query_as::<_, ThreadEmailRow>(
        "SELECT id, subject, from_name, from_addr, received_at, category \
         FROM emails WHERE thread_key = ?1 ORDER BY received_at ASC",
    )
    .bind(thread_key)
    .fetch_all(ctx.pool())
    .await
    .map_err(|e| super::db_error("Thread lookup", e))?;

    Ok(rows.into_iter().map(thread_email).collect())
}

/// Search the mailbox with hybrid vector + full-text retrieval.
pub async fn search_emails(
    ctx: Arc<ToolContext>,
    req: SearchEmailsRequest,
) -> Result<Value, ToolError> {
    if req.query.len() > MAX_QUERY_LEN {
        return Err(ToolError::Invalid(format!(
            "Query too long (max {MAX_QUERY_LEN} characters)"
        )));
    }
    let limit = validate_limit(req.limit, 100);

    let query = HybridSearchQuery {
        text: req.query,
        mode: SearchMode::Hybrid,
        filters: None,
        limit: Some(limit as usize),
        vector_weight: 1.0,
        fts_weight: 1.0,
    };

    let result = ctx
        .vectors()?
        .hybrid_search
        .search(&query)
        .await
        .map_err(|e| super::db_error("Search", e))?;

    let items: Vec<Value> = result
        .results
        .iter()
        .map(|r| {
            json!({
                "email_id": r.email_id,
                "score": r.score,
                "match_type": r.match_type,
                "subject": r.metadata.get("subject").map_or("", String::as_str),
                "from": r.metadata.get("from_addr").map_or("", String::as_str),
                "date": r.metadata.get("received_at").map_or("", String::as_str),
            })
        })
        .collect();

    Ok(json!({
        "total": result.total,
        "results": items,
        "latency_ms": result.latency_ms,
    }))
}

/// Full content of one email.
pub async fn get_email(ctx: Arc<ToolContext>, req: GetEmailRequest) -> Result<Value, ToolError> {
    Ok(json!(fetch_email(&ctx, &req.email_id).await?))
}

/// The most recent emails across all connected accounts.
pub async fn list_recent_emails(
    ctx: Arc<ToolContext>,
    req: ListRecentEmailsRequest,
) -> Result<Value, ToolError> {
    let limit = validate_limit(req.limit.unwrap_or(20), 100) as i64;

    let rows = sqlx::query_as::<_, ThreadEmailRow>(
        "SELECT id, subject, from_name, from_addr, received_at, category \
         FROM emails ORDER BY received_at DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(ctx.pool())
    .await
    .map_err(|e| super::db_error("Email listing", e))?;

    let items: Vec<ThreadEmail> = rows.into_iter().map(thread_email).collect();
    Ok(json!({ "count": items.len(), "emails": items }))
}

/// Count emails matching optional sender, category and date filters.
pub async fn count_emails(
    ctx: Arc<ToolContext>,
    req: CountEmailsRequest,
) -> Result<Value, ToolError> {
    for date in [req.after.as_deref(), req.before.as_deref()]
        .into_iter()
        .flatten()
    {
        validate_date(date).map_err(ToolError::Invalid)?;
    }

    let mut sql = String::from("SELECT COUNT(*) as cnt FROM emails WHERE 1=1");
    let mut binds: Vec<String> = Vec::new();

    if let Some(ref from) = req.from_filter {
        sql.push_str(&format!(" AND from_addr LIKE ?{}", binds.len() + 1));
        binds.push(format!("%{from}%"));
    }
    if let Some(ref category) = req.category {
        sql.push_str(&format!(" AND category = ?{}", binds.len() + 1));
        binds.push(category.clone());
    }
    if let Some(ref after) = req.after {
        sql.push_str(&format!(" AND received_at >= ?{}", binds.len() + 1));
        binds.push(after.clone());
    }
    if let Some(ref before) = req.before {
        sql.push_str(&format!(" AND received_at <= ?{}", binds.len() + 1));
        binds.push(before.clone());
    }

    let mut query = sqlx::query_scalar::<_, i64>(crate::db::audited_sql(&sql));
    for b in &binds {
        query = query.bind(b);
    }

    let count = query
        .fetch_one(ctx.pool())
        .await
        .map_err(|e| super::db_error("Email count", e))?;

    Ok(json!({ "count": count }))
}

/// Every email in the same conversation thread as the given email.
pub async fn get_email_thread(
    ctx: Arc<ToolContext>,
    req: GetEmailThreadRequest,
) -> Result<Value, ToolError> {
    let thread_key = resolve_thread_key(&ctx, &req.email_id).await?;
    let emails = fetch_thread_by_key(&ctx, &thread_key).await?;

    Ok(json!({
        "thread_key": thread_key,
        "count": emails.len(),
        "emails": emails,
    }))
}
