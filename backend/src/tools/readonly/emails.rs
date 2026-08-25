//! Similar-email search and attachment metadata tools.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::params::{
    CountEmailsRequest, FindSimilarEmailsRequest, GetEmailRequest, GetEmailThreadRequest,
    ListAttachmentsRequest, ListRecentEmailsRequest, SearchEmailsRequest,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{validate_date, validate_id, validate_limit};
use crate::db::entities::{attachments, emails};
use crate::tools::{ToolContext, ToolError};
use crate::vectors::search::{HybridSearchQuery, SearchMode};
use crate::vectors::types::SearchParams;

/// Render `emails.received_at` for MCP JSON output.
///
/// The entity reads the column as `NaiveDateTime` (plain `TIMESTAMP`, ADR-036);
/// output is RFC3339 UTC. For rows the ingestion path wrote (RFC3339 `+00:00`
/// strings on SQLite — the dominant case), this is byte-identical to the raw
/// `String` read it replaces. Rows written by the DDL's `CURRENT_TIMESTAMP`
/// default (naive `YYYY-MM-DD HH:MM:SS`) are normalized to RFC3339 rather than
/// echoed raw — a deliberate output normalization, so `date` has one shape.
fn format_received_at(ts: chrono::NaiveDateTime) -> String {
    ts.and_utc().to_rfc3339()
}

/// Row shape for the enrichment lookup: id, subject, from_addr, from_name,
/// received_at, category.
type EmailMetaRow = (
    String,
    String,
    String,
    Option<String>,
    chrono::NaiveDateTime,
    Option<String>,
);

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

    let rows: Vec<EmailMetaRow> = emails::Entity::find()
        .select_only()
        .column(emails::Column::Id)
        .column(emails::Column::Subject)
        .column(emails::Column::FromAddr)
        .column(emails::Column::FromName)
        .column(emails::Column::ReceivedAt)
        .column(emails::Column::Category)
        .filter(emails::Column::Id.is_in(ids.iter().map(String::as_str)))
        .into_tuple()
        .all(&ctx.conn())
        .await
        .unwrap_or_default();

    rows.into_iter()
        .map(
            |(id, subject, from_addr, from_name, received_at, category)| {
                (
                    id,
                    (
                        subject,
                        from_addr,
                        from_name,
                        format_received_at(received_at),
                        // NULL category reads as the column's own default rather
                        // than erroring the whole batch (the lenient unification
                        // this phase settled on).
                        category.unwrap_or_else(|| "Uncategorized".to_string()),
                    ),
                )
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

    let mut query =
        attachments::Entity::find().filter(attachments::Column::EmailId.eq(req.email_id.as_str()));
    if !include_inline {
        query = query.filter(attachments::Column::IsInline.eq(false));
    }
    let rows = query
        .order_by_asc(attachments::Column::Filename)
        .all(&ctx.conn())
        .await
        .map_err(|e| super::db_error("Listing attachments", e))?;

    // `size_bytes` is a real 4-byte INTEGER on PostgreSQL — the old direct
    // `i64` decode was the ADR-035 width class; the entity's i32 widens here.
    let total_bytes: i64 = rows.iter().map(|r| i64::from(r.size_bytes)).sum();
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "email_id": r.email_id,
                "filename": r.filename,
                "content_type": r.content_type,
                "size_bytes": i64::from(r.size_bytes),
                "is_inline": r.is_inline,
                "fetch_status": r.fetch_status,
            })
        })
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

/// (id, subject, from_name, from_addr, received_at, category) — the listing
/// projection. `category` is nullable in the DDL; NULL reads as the column's
/// default (lenient unification).
type ThreadEmailTuple = (
    String,
    String,
    Option<String>,
    String,
    chrono::NaiveDateTime,
    Option<String>,
);

/// Render a display sender, preferring `Name <addr>` when a name is stored.
fn format_sender(from_name: Option<&String>, from_addr: &str) -> String {
    match from_name {
        Some(name) if !name.is_empty() => format!("{name} <{from_addr}>"),
        _ => from_addr.to_string(),
    }
}

fn thread_email(row: ThreadEmailTuple) -> ThreadEmail {
    let (id, subject, from_name, from_addr, received_at, category) = row;
    ThreadEmail {
        from: format_sender(from_name.as_ref(), &from_addr),
        id,
        subject,
        date: format_received_at(received_at),
        category: category.unwrap_or_else(|| "Uncategorized".to_string()),
    }
}

/// The shared listing projection, in `ThreadEmailTuple` column order.
fn thread_email_query() -> sea_orm::Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Id)
        .column(emails::Column::Subject)
        .column(emails::Column::FromName)
        .column(emails::Column::FromAddr)
        .column(emails::Column::ReceivedAt)
        .column(emails::Column::Category)
}

/// Load one email including its body.
pub async fn fetch_email(ctx: &ToolContext, email_id: &str) -> Result<EmailRecord, ToolError> {
    validate_id("email_id", email_id).map_err(ToolError::Invalid)?;

    type Row = (
        String,
        String,
        Option<String>,
        String,
        chrono::NaiveDateTime,
        Option<String>,
        Option<String>,
    );
    let (id, subject, from_name, from_addr, received_at, body_text, category): Row =
        emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::Subject)
            .column(emails::Column::FromName)
            .column(emails::Column::FromAddr)
            .column(emails::Column::ReceivedAt)
            .column(emails::Column::BodyText)
            .column(emails::Column::Category)
            .filter(emails::Column::Id.eq(email_id))
            .into_tuple()
            .one(&ctx.conn())
            .await
            .map_err(|e| super::db_error("Email lookup", e))?
            .ok_or_else(|| ToolError::NotFound("Email not found".to_string()))?;

    Ok(EmailRecord {
        from: format_sender(from_name.as_ref(), &from_addr),
        id,
        subject,
        date: format_received_at(received_at),
        category: category.unwrap_or_else(|| "Uncategorized".to_string()),
        body: body_text.unwrap_or_default(),
    })
}

/// Resolve the thread an email belongs to.
///
/// The `get_email_thread` tool is keyed by email id and needs this hop; the
/// `thread://{key}` resource carries the thread key already and calls
/// [`fetch_thread_by_key`] directly.
pub async fn resolve_thread_key(ctx: &ToolContext, email_id: &str) -> Result<String, ToolError> {
    validate_id("email_id", email_id).map_err(ToolError::Invalid)?;

    let row: Option<(Option<String>,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::ThreadKey)
        .filter(emails::Column::Id.eq(email_id))
        .into_tuple()
        .one(&ctx.conn())
        .await
        .map_err(|e| super::db_error("Thread lookup", e))?;
    match row {
        // A NULL thread_key was a decode error under the old non-optional
        // String read — an internal error, not NotFound. Preserve that class.
        Some((Some(key),)) => Ok(key),
        Some((None,)) => Err(super::db_error("Thread lookup", "email has no thread_key")),
        None => Err(ToolError::NotFound("Email not found".to_string())),
    }
}

/// Every email sharing a thread key, oldest first.
pub async fn fetch_thread_by_key(
    ctx: &ToolContext,
    thread_key: &str,
) -> Result<Vec<ThreadEmail>, ToolError> {
    let rows: Vec<ThreadEmailTuple> = thread_email_query()
        .filter(emails::Column::ThreadKey.eq(thread_key))
        .order_by_asc(emails::Column::ReceivedAt)
        .into_tuple()
        .all(&ctx.conn())
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

    let rows: Vec<ThreadEmailTuple> = thread_email_query()
        .order_by_desc(emails::Column::ReceivedAt)
        .limit(limit as u64)
        .into_tuple()
        .all(&ctx.conn())
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

    // `received_at` is a typed temporal column at the entity (ADR-036), so the
    // validated `YYYY-MM-DD` bounds become midnight instants. The old code
    // compared strings lexicographically, which was incoherent across the
    // column's two legacy on-disk shapes (RFC3339 vs space-separated); the
    // typed comparison behaves identically at date granularity and is coherent
    // for both.
    fn midnight(date: &str) -> Option<chrono::NaiveDateTime> {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    }

    let mut query = emails::Entity::find();
    if let Some(ref from) = req.from_filter {
        query = query.filter(emails::Column::FromAddr.contains(from.as_str()));
    }
    if let Some(ref category) = req.category {
        query = query.filter(emails::Column::Category.eq(category.as_str()));
    }
    if let Some(ts) = req.after.as_deref().and_then(midnight) {
        query = query.filter(emails::Column::ReceivedAt.gte(ts));
    }
    if let Some(ts) = req.before.as_deref().and_then(midnight) {
        // Strictly-less-than: the pre-port text compare against a bare
        // `YYYY-MM-DD` excluded the ENTIRE boundary day (every stored shape
        // sorts above the bare date), so midnight itself is out too.
        query = query.filter(emails::Column::ReceivedAt.lt(ts));
    }

    let count = sea_orm::PaginatorTrait::count(query, &ctx.conn())
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sea_orm::ConnectionTrait;

    use super::*;

    /// In-memory context with the full emails-table schema (001 + the ALTERs
    /// from 016/018/021/027 the entity's columns require).
    async fn ctx() -> Arc<ToolContext> {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        crate::db::apply_sqlite_migrations(
            &conn,
            &[
                include_str!("../../../migrations/sqlite/001_initial_schema.sql"),
                include_str!("../../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
                include_str!("../../../migrations/sqlite/018_unsubscribe_headers.sql"),
                include_str!("../../../migrations/sqlite/021_thread_key.sql"),
                include_str!("../../../migrations/sqlite/027_is_archived.sql"),
            ],
        )
        .await
        .expect("migrate");
        Arc::new(ToolContext::new(Arc::new(db)))
    }

    async fn seed_email(ctx: &ToolContext, id: &str, received_at_literal: &str) {
        ctx.conn()
            .execute_unprepared(&format!(
                "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
                 received_at, thread_key) VALUES ('{id}', 'a1', 'gmail', 'subj-{id}', \
                 's@example.com', 'r@example.com', '{received_at_literal}', 'tk-{id}')"
            ))
            .await
            .expect("seed");
    }

    /// Legacy RFC3339 rows (what the pre-port ingestion String binds wrote)
    /// decode through the NaiveDateTime entity and emit byte-identically.
    #[tokio::test]
    async fn rfc3339_utc_rows_round_trip_byte_identically() {
        let ctx = ctx().await;
        seed_email(&ctx, "e1", "2026-08-01T10:30:00+00:00").await;
        let rec = fetch_email(&ctx, "e1").await.expect("fetch");
        assert_eq!(rec.date, "2026-08-01T10:30:00+00:00");
    }

    /// Rows in the DDL default's naive shape are normalized to RFC3339 UTC —
    /// the one deliberate output normalization of this port (documented on
    /// `format_received_at`).
    #[tokio::test]
    async fn naive_default_shape_rows_normalize_to_rfc3339() {
        let ctx = ctx().await;
        seed_email(&ctx, "e2", "2026-08-01 10:30:00").await;
        let rec = fetch_email(&ctx, "e2").await.expect("fetch");
        assert_eq!(rec.date, "2026-08-01T10:30:00+00:00");
    }

    /// Typed date filters count coherently across BOTH legacy stored shapes —
    /// the old lexicographic compare could not.
    #[tokio::test]
    async fn count_filters_span_both_legacy_shapes() {
        let ctx = ctx().await;
        seed_email(&ctx, "e1", "2026-08-01T09:00:00+00:00").await;
        seed_email(&ctx, "e2", "2026-08-01 23:00:00").await;
        seed_email(&ctx, "e3", "2026-07-20T12:00:00+00:00").await;

        let v = count_emails(
            ctx.clone(),
            CountEmailsRequest {
                from_filter: None,
                category: None,
                after: Some("2026-08-01".into()),
                before: None,
            },
        )
        .await
        .expect("count");
        assert_eq!(v["count"], 2);

        let v = count_emails(
            ctx,
            CountEmailsRequest {
                from_filter: None,
                category: None,
                after: None,
                before: Some("2026-07-31".into()),
            },
        )
        .await
        .expect("count");
        assert_eq!(v["count"], 1);
    }
}
