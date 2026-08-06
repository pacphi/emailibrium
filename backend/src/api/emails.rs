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
//!
//! Persistence is single-code-path SeaORM (ADR-036): the `emails` and
//! `attachments` entities own per-backend encode/decode, so every query below
//! runs unchanged against SQLite and PostgreSQL. The queries live in the
//! "Queries" section and take a bare `&DatabaseConnection` so the tests at the
//! bottom can drive them without a full `AppState`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use sea_orm::sea_query::{Asterisk, CaseStatement, Expr, ExprTrait, Func};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, FromQueryResult,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Select,
};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::api::provider_helpers::resolve_provider_and_token;
use crate::db::entities::{attachments, emails};
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
    pub is_archived: Option<bool>,
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

// ---------------------------------------------------------------------------
// Queries
//
// Each takes a `&DatabaseConnection` rather than `AppState` so the tests at the
// bottom of this file can drive it directly; the handlers pass `&state.orm`.
// ---------------------------------------------------------------------------

/// The 21-column listing projection every email-shaped endpoint reads.
///
/// Replaces the `EMAIL_COLUMNS` string the pre-port handlers interpolated into
/// their SQL; the column set is unchanged, so no endpoint gained or lost a field.
#[derive(Debug, FromQueryResult)]
struct EmailRow {
    id: String,
    account_id: String,
    provider: String,
    message_id: Option<String>,
    thread_id: Option<String>,
    subject: String,
    from_addr: String,
    from_name: Option<String>,
    to_addrs: String,
    cc_addrs: Option<String>,
    /// Plain `TIMESTAMP` (no zone) in both dialects, hence `NaiveDateTime` —
    /// rendered by [`format_received_at`].
    received_at: chrono::NaiveDateTime,
    body_text: Option<String>,
    body_html: Option<String>,
    labels: Option<String>,
    is_read: Option<bool>,
    is_starred: Option<bool>,
    has_attachments: Option<bool>,
    embedding_status: Option<String>,
    category: Option<String>,
    /// REAL in both dialects — a 4-byte float on PostgreSQL, so the entity's
    /// `f32` is the honest width; the response type's `f64` widens in Rust.
    category_confidence: Option<f32>,
    folder: String,
}

/// The projection behind [`EmailRow`], in the pre-port `EMAIL_COLUMNS` order.
fn email_projection() -> Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Id)
        .column(emails::Column::AccountId)
        .column(emails::Column::Provider)
        .column(emails::Column::MessageId)
        .column(emails::Column::ThreadId)
        .column(emails::Column::Subject)
        .column(emails::Column::FromAddr)
        .column(emails::Column::FromName)
        .column(emails::Column::ToAddrs)
        .column(emails::Column::CcAddrs)
        .column(emails::Column::ReceivedAt)
        .column(emails::Column::BodyText)
        .column(emails::Column::BodyHtml)
        .column(emails::Column::Labels)
        .column(emails::Column::IsRead)
        .column(emails::Column::IsStarred)
        .column(emails::Column::HasAttachments)
        .column(emails::Column::EmbeddingStatus)
        .column(emails::Column::Category)
        .column(emails::Column::CategoryConfidence)
        .column(emails::Column::Folder)
}

/// Render `emails.received_at` for JSON output.
///
/// The entity reads the column as `NaiveDateTime` (plain `TIMESTAMP`, ADR-036);
/// output is RFC3339 UTC. For rows the ingestion path wrote (RFC3339 `+00:00`
/// strings on SQLite — the dominant case), this is byte-identical to the raw
/// `String` read it replaces. Rows written by the DDL's `CURRENT_TIMESTAMP`
/// default (naive `YYYY-MM-DD HH:MM:SS`) are normalized to RFC3339 rather than
/// echoed raw — the same single output policy `tools/readonly/emails.rs` applies.
fn format_received_at(ts: chrono::NaiveDateTime) -> String {
    ts.and_utc().to_rfc3339()
}

/// Map a projected row onto the wire type.
///
/// The nullable-boolean and nullable-text columns read leniently — a NULL takes
/// the column's own DDL default rather than failing the whole request, which is
/// what the pre-port `row.get::<bool, _>(...)` decodes did. `labels` stays
/// `Option<String>`, so a NULL still serialises as `null` exactly as before.
fn email_response(row: EmailRow) -> EmailResponse {
    EmailResponse {
        id: row.id,
        account_id: row.account_id,
        provider: row.provider,
        message_id: row.message_id,
        thread_id: row.thread_id,
        subject: row.subject,
        from_addr: row.from_addr,
        from_name: row.from_name,
        to_addrs: row.to_addrs,
        cc_addrs: row.cc_addrs,
        received_at: format_received_at(row.received_at),
        body_text: row.body_text,
        body_html: row.body_html,
        labels: row.labels,
        is_read: row.is_read.unwrap_or(false),
        is_starred: row.is_starred.unwrap_or(false),
        has_attachments: row.has_attachments.unwrap_or(false),
        embedding_status: row
            .embedding_status
            .unwrap_or_else(|| "pending".to_string()),
        category: row.category.unwrap_or_else(|| "Uncategorized".to_string()),
        category_confidence: row.category_confidence.map(f64::from),
        folder: Some(row.folder),
    }
}

/// Case-insensitive equality, replacing SQLite's `COLLATE NOCASE`.
///
/// `COLLATE NOCASE` is a SQLite-only collation (ADR-035's dialect catalog);
/// `LOWER()` on both sides is the portable spelling, and on SQLite it folds the
/// same ASCII range NOCASE does, so the SQLite result set is unchanged.
fn ci_eq(column: emails::Column, value: &str) -> Expr {
    // ASCII-only fold on BOTH sides: SQLite's `lower()` folds ASCII only, so a
    // Unicode-aware `str::to_lowercase()` on the bind would stop exact-case
    // non-ASCII values from matching (pre-port `COLLATE NOCASE` is ASCII-only
    // too). PostgreSQL's `lower()` IS Unicode-aware, so non-ASCII case folding
    // differs per backend — irreducible without ICU collation.
    Expr::from(Func::lower(Expr::col(column))).eq(value.to_ascii_lowercase())
}

/// Match a label inside the comma-separated `labels` column.
///
/// `||` is the string concatenation both dialects share (ADR-035's catalog), but
/// sea-query only exposes it as a PostgreSQL extension, so the delimiter-padded
/// column stays a constant raw fragment — no interpolation, nothing bindable in
/// it. The pattern itself is built in Rust and bound, which is exactly what the
/// pre-port `'%,' || ? || ',%'` concatenation evaluated to (including the fact
/// that `%`/`_` inside a label act as wildcards, unchanged).
fn label_filter(label: &str) -> Condition {
    const DELIMITED_LABELS: &str = "(',' || labels || ',')";
    Condition::any()
        .add(Expr::cust(DELIMITED_LABELS).like(format!("%,{label},%")))
        // Gmail user labels carry a `$` prefix; match that variant too.
        .add(Expr::cust(DELIMITED_LABELS).like(format!("%,${label},%")))
}

/// The dynamic `WHERE` the listing endpoint builds from its query parameters.
fn list_filter(params: &ListEmailsParams) -> Condition {
    let mut cond = Condition::all();
    if let Some(ref v) = params.account_id {
        cond = cond.add(emails::Column::AccountId.eq(v.as_str()));
    }
    if let Some(ref v) = params.category {
        cond = cond.add(ci_eq(emails::Column::Category, v));
    }
    if let Some(ref v) = params.label {
        cond = cond.add(label_filter(v));
    }
    if let Some(v) = params.is_read {
        cond = cond.add(emails::Column::IsRead.eq(v));
    }
    if let Some(v) = params.is_starred {
        cond = cond.add(emails::Column::IsStarred.eq(v));
    }
    // Spam/trash: when explicitly requested show only those; otherwise exclude
    // them. Both are INTEGER 0/1 flags (migration 016), not BOOLEAN — the
    // pre-port `COALESCE(is_spam, 0) = 0` guarded a NOT NULL column, so the
    // COALESCE never fired and drops out here.
    match params.is_spam {
        Some(true) => cond = cond.add(emails::Column::IsSpam.eq(1_i32)),
        None => cond = cond.add(emails::Column::IsSpam.eq(0_i32)),
        Some(false) => {}
    }
    match params.is_trash {
        Some(true) => cond = cond.add(emails::Column::IsTrash.eq(1_i32)),
        None => cond = cond.add(emails::Column::IsTrash.eq(0_i32)),
        Some(false) => {}
    }
    // Archived: when explicitly true show only archived; when explicitly false
    // show all; when omitted (None) exclude archived by default — matching
    // spam/trash behaviour. This column *is* BOOLEAN (migration 027), so it
    // compares against `true`/`false`, not 1/0.
    match params.is_archived {
        Some(true) => cond = cond.add(emails::Column::IsArchived.eq(true)),
        None => cond = cond.add(emails::Column::IsArchived.eq(false)),
        Some(false) => {}
    }
    if let Some(ref v) = params.folder {
        cond = cond.add(ci_eq(emails::Column::Folder, v));
    }
    cond
}

/// One page of the listing, newest first.
fn email_page_query(cond: Condition, limit: u64, offset: u64) -> Select<emails::Entity> {
    email_projection()
        .filter(cond)
        .order_by_desc(emails::Column::ReceivedAt)
        .limit(limit)
        .offset(offset)
}

async fn fetch_email_page(
    conn: &DatabaseConnection,
    cond: Condition,
    limit: u64,
    offset: u64,
) -> Result<Vec<EmailRow>, DbErr> {
    email_page_query(cond, limit, offset)
        .into_model::<EmailRow>()
        .all(conn)
        .await
}

/// One projected email by id.
async fn fetch_email_row(conn: &DatabaseConnection, id: &str) -> Result<Option<EmailRow>, DbErr> {
    email_projection()
        .filter(emails::Column::Id.eq(id))
        .into_model::<EmailRow>()
        .one(conn)
        .await
}

/// Every email in a provider-side thread, oldest first.
async fn fetch_thread_rows(
    conn: &DatabaseConnection,
    thread_id: &str,
) -> Result<Vec<EmailRow>, DbErr> {
    email_projection()
        .filter(emails::Column::ThreadId.eq(thread_id))
        .order_by_asc(emails::Column::ReceivedAt)
        .into_model::<EmailRow>()
        .all(conn)
        .await
}

/// The "not spam, not trash" predicate every reporting query shares.
///
/// Both columns are INTEGER 0/1 and NOT NULL (migration 016), so the pre-port
/// `COALESCE(is_spam, 0) = 0` was guarding a column that cannot be NULL.
fn active_condition() -> Condition {
    Condition::all()
        .add(emails::Column::IsSpam.eq(0_i32))
        .add(emails::Column::IsTrash.eq(0_i32))
}

/// `COUNT(*)`, spelled out at each use site rather than aliased once, so the
/// ORDER BY does not depend on a select alias being in scope.
fn count_star() -> Expr {
    Expr::from(Func::count(Expr::col(Asterisk)))
}

/// `COALESCE(SUM(CASE WHEN <unread> THEN 1 ELSE 0 END), 0)`.
///
/// The pre-port condition was `is_read = 0`, which is a type error on
/// PostgreSQL: `is_read` is BOOLEAN there (migration 001), and `boolean =
/// integer` has no operator. The predicate below compares against the boolean
/// instead. It also treats a NULL `is_read` as unread — the column's DDL
/// default is FALSE, the same lenient unification the row mapping applies —
/// where the old `is_read = 0` evaluated to NULL and silently counted such a
/// row as read.
///
/// `THEN`/`ELSE` are inlined constants rather than binds so PostgreSQL can
/// resolve `SUM`'s argument type (a bare parameter there is untypable).
fn unread_sum() -> Expr {
    let unread = Condition::any()
        .add(emails::Column::IsRead.is_null())
        .add(emails::Column::IsRead.eq(false));
    let case = CaseStatement::new()
        .case(unread, Expr::Constant(1_i32.into()))
        .finally(Expr::Constant(0_i32.into()));
    Expr::from(Func::coalesce([
        Expr::from(Func::sum(case)),
        Expr::Constant(0_i64.into()),
    ]))
}

/// Mailbox-wide `(total, unread)` over the active corpus.
fn total_and_unread_query() -> Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column_as(count_star(), "total")
        .column_as(unread_sum(), "unread")
        .filter(active_condition())
}

/// `(category, total, unread)` per category, most populous first. The NULL
/// category forms its own group and is labelled by the caller.
fn category_counts_query() -> Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Category)
        .column_as(count_star(), "total")
        .column_as(unread_sum(), "unread")
        .filter(active_condition())
        .group_by(emails::Column::Category)
        .order_by_desc(count_star())
}

/// [`category_counts_query`] restricted to named categories — what the enriched
/// categories endpoint reports.
fn enriched_category_query() -> Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Category)
        .column_as(count_star(), "total")
        .column_as(unread_sum(), "unread")
        .filter(emails::Column::Category.is_not_null())
        .filter(emails::Column::Category.ne("Uncategorized"))
        .filter(active_condition())
        .group_by(emails::Column::Category)
        .order_by_desc(count_star())
}

/// Resolve [`category_counts_query`]'s groups onto the response type.
///
/// `GROUP BY category` puts SQL NULL in a group of its own, but the column's
/// DDL default is the *literal* `'Uncategorized'` (migration 001) — so one
/// mailbox can hold both, and labelling the NULL group "Uncategorized" app-side
/// would emit that label twice. The two buckets are merged here. (The pre-port
/// query had the same split: it selected `COALESCE(category, 'Uncategorized')`
/// while still grouping by the bare column, so it emitted the duplicate too.)
///
/// The re-sort restores "most populous first" after a merge; it is stable, so
/// every group the merge did not touch keeps the order the SQL produced.
fn fold_category_counts(rows: Vec<(Option<String>, i64, i64)>) -> Vec<CategoryCount> {
    let mut folded: Vec<CategoryCount> = Vec::with_capacity(rows.len());
    for (category, total, unread) in rows {
        let category = category.unwrap_or_else(|| "Uncategorized".to_string());
        match folded.iter_mut().find(|c| c.category == category) {
            Some(existing) => {
                existing.total += total as u64;
                existing.unread += unread as u64;
            }
            None => folded.push(CategoryCount {
                category,
                total: total as u64,
                unread: unread as u64,
            }),
        }
    }
    folded.sort_by_key(|c| std::cmp::Reverse(c.total));
    folded
}

/// Overwrite an email's comma-separated `labels` column.
async fn set_labels(conn: &DatabaseConnection, id: &str, labels: &str) -> Result<(), DbErr> {
    emails::Entity::update_many()
        .col_expr(emails::Column::Labels, Expr::value(labels))
        .filter(emails::Column::Id.eq(id))
        .exec(conn)
        .await?;
    Ok(())
}

/// GET /api/v1/emails
async fn list_emails(
    State(state): State<AppState>,
    Query(params): Query<ListEmailsParams>,
) -> Result<Json<ListEmailsResponse>, (StatusCode, String)> {
    // Clamped rather than merely capped: a negative LIMIT disabled the limit
    // entirely on SQLite and is a bind error on PostgreSQL, and a negative
    // OFFSET is meaningless on both.
    let limit = params.limit.unwrap_or(50).clamp(0, 200) as u64;
    let offset = u64::try_from(params.offset.unwrap_or(0)).unwrap_or(0);

    let cond = list_filter(&params);

    let total = emails::Entity::find()
        .filter(cond.clone())
        .count(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows = fetch_email_page(&state.orm, cond, limit, offset)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let emails = rows.into_iter().map(email_response).collect();

    Ok(Json(ListEmailsResponse {
        emails,
        total: total as i64,
    }))
}

/// GET /api/v1/emails/:id
async fn get_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EmailResponse>, (StatusCode, String)> {
    let row = fetch_email_row(&state.orm, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match row {
        Some(r) => Ok(Json(email_response(r))),
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
    let rows = fetch_thread_rows(&state.orm, &thread_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.is_empty() {
        // Fall back: try treating thread_id as a message ID (single-message thread).
        let single = fetch_email_row(&state.orm, &thread_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        match single {
            Some(r) => {
                let email = email_response(r);
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

    let emails: Vec<EmailResponse> = rows.into_iter().map(email_response).collect();
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
    let row: Option<(String,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::AccountId)
        .filter(emails::Column::Id.eq(email_id))
        .into_tuple()
        .one(&state.orm)
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

    // `is_archived` is BOOLEAN in both dialects (migration 027), so it is written
    // as a bool — the literal `1` the pre-port SQL used is a type error on
    // PostgreSQL.
    let rows = emails::Entity::update_many()
        .col_expr(emails::Column::Labels, Expr::value("ARCHIVED"))
        .col_expr(emails::Column::IsArchived, Expr::value(true))
        .filter(emails::Column::Id.eq(&id))
        .exec(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected == 0 {
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

    // Determine new starred state from local DB. A NULL `is_starred` reads as
    // the column's DDL default (FALSE) rather than failing the request, which is
    // what the pre-port non-optional `bool` decode did.
    let current: Option<(Option<bool>,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::IsStarred)
        .filter(emails::Column::Id.eq(&id))
        .into_tuple()
        .one(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let new_starred = !current
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Email not found".to_string()))?
        .0
        .unwrap_or(false);

    // Star/unstar on the provider (best-effort).
    if let Ok(account_id) = get_email_account_id(&state, &id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(&state, &account_id).await {
            if let Err(e) = provider.star_message(&token, &id, new_starred).await {
                debug!(email_id = %id, "Provider star failed (continuing locally): {e}");
            }
        }
    }

    emails::Entity::update_many()
        .col_expr(emails::Column::IsStarred, Expr::value(new_starred))
        .filter(emails::Column::Id.eq(&id))
        .exec(&state.orm)
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

    emails::Entity::update_many()
        .col_expr(emails::Column::IsRead, Expr::value(body.read))
        .filter(emails::Column::Id.eq(&id))
        .exec(&state.orm)
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
        let rows = crate::db::update_email_state(&state.orm, &id, true, false, "TRASH", Some(&now))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if rows == 0 {
            return Err((StatusCode::NOT_FOUND, "Email not found".to_string()));
        }
        debug!(email_id = %id, "Email soft-deleted (moved to trash)");
        Ok(Json(serde_json::json!({ "status": "trashed" })))
    }
}

/// Delete an email's cached attachment files (the DB rows cascade with the email).
async fn remove_attachment_files(
    conn: &DatabaseConnection,
    email_id: &str,
) -> Result<(), (StatusCode, String)> {
    let att_paths: Vec<(String,)> = attachments::Entity::find()
        .select_only()
        .column(attachments::Column::StoragePath)
        .filter(attachments::Column::EmailId.eq(email_id))
        .filter(attachments::Column::StoragePath.is_not_null())
        .into_tuple()
        .all(conn)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for (path,) in &att_paths {
        let _ = tokio::fs::remove_file(path).await; // best-effort
    }
    // Also try to remove the per-email attachment directory.
    let _ = tokio::fs::remove_dir(format!("data/attachments/{email_id}")).await;
    Ok(())
}

/// Hard-delete a single email: best-effort provider delete, then remove locally.
async fn hard_delete_email(state: &AppState, email_id: &str) -> Result<(), (StatusCode, String)> {
    // Best-effort: permanently delete from the provider before removing locally.
    if let Ok(account_id) = get_email_account_id(state, email_id).await {
        if let Ok((provider, token, _)) = resolve_provider_and_token(state, &account_id).await {
            if let Err(e) = provider.delete_message(&token, email_id).await {
                debug!(email_id = %email_id, "Provider permanent delete failed (continuing locally): {e}");
            }
        }
    }

    remove_attachment_files(&state.orm, email_id).await?;

    let rows = emails::Entity::delete_many()
        .filter(emails::Column::Id.eq(email_id))
        .exec(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.rows_affected == 0 {
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

    let rows = crate::db::update_email_state(&state.orm, &id, false, true, "SPAM", None)
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

    let rows = crate::db::update_email_state(&state.orm, &id, false, false, "INBOX", None)
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

    let rows = crate::db::update_email_state(&state.orm, &id, false, false, "INBOX", None)
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
/// The trash set `empty_trash` deletes: every trashed email, optionally
/// scoped to one account. The same condition drives the attachment-cleanup
/// lookup and the hard delete, so they cannot drift — and the account scope
/// is pinned by `empty_trash_scoped_to_account_spares_the_bystander`.
fn trashed_condition(account_id: Option<&str>) -> Condition {
    let mut trashed = Condition::all().add(emails::Column::IsTrash.eq(1_i32));
    if let Some(account_id) = account_id {
        trashed = trashed.add(emails::Column::AccountId.eq(account_id));
    }
    trashed
}

async fn empty_trash(
    State(state): State<AppState>,
    Query(params): Query<EmptyTrashParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    debug!("Emptying trash");

    let trashed = trashed_condition(params.account_id.as_deref());

    let ids: Vec<(String,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::Id)
        .filter(trashed.clone())
        .into_tuple()
        .all(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Clean up attachment files for each trashed email.
    for (email_id,) in &ids {
        remove_attachment_files(&state.orm, email_id).await?;
    }

    // Hard delete all trashed emails.
    let rows = emails::Entity::delete_many()
        .filter(trashed)
        .exec(&state.orm)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let deleted_count = rows.rows_affected;

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
    let rows: Vec<(String,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::Category)
        .distinct()
        .filter(emails::Column::Category.is_not_null())
        .filter(emails::Column::Category.ne("Uncategorized"))
        .order_by_asc(emails::Column::Category)
        .into_tuple()
        .all(&state.orm)
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
            crate::db::update_email_state(&state.orm, &id, is_trash, is_spam, folder, None)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Also update the labels column.
            set_labels(&state.orm, &id, &body.target_id)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        MoveKind::Label => {
            // Append label (avoid duplicates with a read-modify-write).
            // A NULL `labels` reads as "no labels" rather than failing the
            // request — the pre-port non-optional `String` decode errored there.
            let current: Option<(Option<String>,)> = emails::Entity::find()
                .select_only()
                .column(emails::Column::Labels)
                .filter(emails::Column::Id.eq(&id))
                .into_tuple()
                .one(&state.orm)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            if let Some((labels,)) = current {
                let labels = labels.unwrap_or_default();
                let mut label_set: Vec<&str> =
                    labels.split(',').filter(|s| !s.is_empty()).collect();
                if !label_set.iter().any(|l| *l == body.target_id) {
                    label_set.push(&body.target_id);
                }
                let new_labels = label_set.join(",");
                set_labels(&state.orm, &id, &new_labels)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Derive state from the combined labels.
                let label_vec: Vec<String> = new_labels.split(',').map(|s| s.to_string()).collect();
                let (is_trash, is_spam, folder) = crate::db::derive_state_from_labels(&label_vec);
                crate::db::update_email_state(&state.orm, &id, is_trash, is_spam, folder, None)
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
    let rows: Vec<(Option<String>, String, Option<bool>)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::Labels)
        .column(emails::Column::AccountId)
        .column(emails::Column::IsRead)
        .filter(emails::Column::Labels.is_not_null())
        .filter(emails::Column::Labels.ne(""))
        .filter(active_condition())
        .into_tuple()
        .all(&state.orm)
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
        // The `labels IS NOT NULL` filter already excludes NULLs; the lenient
        // read keeps a NULL from failing the whole aggregation, and a NULL
        // `is_read` counts as unread — the column's DDL default is FALSE.
        let is_read = is_read.unwrap_or(false);
        for label in labels_csv
            .as_deref()
            .unwrap_or_default()
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

    result.sort_by_key(|a| std::cmp::Reverse(a.email_count));
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
    let rows: Vec<(String, i64, i64)> = enriched_category_query()
        .into_tuple()
        .all(&state.orm)
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
    pub archived_count: u64,
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

/// GET /api/v1/emails/counts — accurate total, unread, archived, spam, trash, and per-category counts.
/// `total` and `unread` include archived emails so callers can derive active counts client-side.
async fn email_counts(
    State(state): State<AppState>,
) -> Result<Json<EmailCounts>, (StatusCode, String)> {
    let to_500 = |e: DbErr| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());

    // Total and unread (excluding spam/trash; archived emails included in total).
    // COUNT(*) is never NULL, so the pre-port COALESCE around it is dropped; the
    // one around SUM stays, because SUM over zero rows *is* NULL.
    let (total, unread): (i64, i64) = total_and_unread_query()
        .into_tuple()
        .one(&state.orm)
        .await
        .map_err(to_500)?
        // An aggregate with no GROUP BY always yields exactly one row.
        .unwrap_or_default();

    // Archived count (subset of total above). `is_archived` is BOOLEAN, so the
    // comparison is against `true`, not the pre-port literal 1.
    let archived_count = emails::Entity::find()
        .filter(emails::Column::IsArchived.eq(true))
        .filter(active_condition())
        .count(&state.orm)
        .await
        .map_err(to_500)?;

    // Spam and trash counts.
    let spam_count = emails::Entity::find()
        .filter(emails::Column::IsSpam.eq(1_i32))
        .count(&state.orm)
        .await
        .map_err(to_500)?;

    let trash_count = emails::Entity::find()
        .filter(emails::Column::IsTrash.eq(1_i32))
        .count(&state.orm)
        .await
        .map_err(to_500)?;

    let sent_count = emails::Entity::find()
        .filter(emails::Column::Folder.eq("SENT"))
        .filter(active_condition())
        .count(&state.orm)
        .await
        .map_err(to_500)?;

    // Per-category (excluding spam/trash; archived included so sidebar counts match toggle-off).
    let cat_rows: Vec<(Option<String>, i64, i64)> = category_counts_query()
        .into_tuple()
        .all(&state.orm)
        .await
        .map_err(to_500)?;

    let by_category = fold_category_counts(cat_rows);

    Ok(Json(EmailCounts {
        total: total as u64,
        unread: unread as u64,
        archived_count,
        spam_count,
        trash_count,
        sent_count,
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

// ---------------------------------------------------------------------------
// Tests
//
// This module had no coverage at all before the SeaORM port (ADR-036), and the
// API integration suite exercises a parallel reimplementation rather than these
// handlers, so these pin the queries the handlers wrap — the handlers themselves
// need a full `AppState`, which is why the queries take a bare connection. What
// they assert is derived from the pre-port SQL text: the dynamic listing WHERE,
// the `GROUP BY category` reporting shape, and the `EMAIL_COLUMNS` row mapping.
// Two of them read the generated PostgreSQL SQL instead of running it, because
// the divergences they guard (`is_read = 0` against a BOOLEAN column, SQLite's
// `COLLATE NOCASE`) are accepted by SQLite and can only be caught by reading it.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use sea_orm::{ConnectionTrait, DbBackend, QueryTrait};

    /// In-memory SQLite carrying every migration the `emails` entity spans: 001
    /// creates the table and 016/018/021/027 add the columns it declares.
    async fn fresh_conn() -> DatabaseConnection {
        let conn = crate::db::test_sqlite_database().await.sea_orm();
        crate::db::apply_sqlite_migrations(
            &conn,
            &[
                include_str!("../../migrations/sqlite/001_initial_schema.sql"),
                include_str!("../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
                include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
                include_str!("../../migrations/sqlite/021_thread_key.sql"),
                include_str!("../../migrations/sqlite/027_is_archived.sql"),
            ],
        )
        .await
        .expect("migrate");
        conn
    }

    /// One seeded email. `received_at` is a raw SQL literal so a test can write
    /// either legacy on-disk shape; everything else mirrors the DDL defaults.
    #[derive(Clone)]
    struct Seed<'a> {
        id: &'a str,
        account_id: &'a str,
        received_at: &'a str,
        folder: &'a str,
        labels: &'a str,
        category: Option<&'a str>,
        is_read: bool,
        is_spam: i32,
        is_trash: i32,
        is_archived: bool,
    }

    impl Default for Seed<'_> {
        fn default() -> Self {
            Self {
                id: "e1",
                account_id: "acct-1",
                received_at: "2026-08-01T10:00:00+00:00",
                folder: "INBOX",
                labels: "INBOX",
                category: None,
                is_read: false,
                is_spam: 0,
                is_trash: 0,
                is_archived: false,
            }
        }
    }

    /// Insert via raw SQL rather than an ActiveModel: the typed write would
    /// normalise `received_at` to sqlx's naive shape, and these tests exist to
    /// cover BOTH shapes already on disk.
    async fn seed(conn: &DatabaseConnection, row: Seed<'_>) {
        let category = row
            .category
            .map_or_else(|| "NULL".to_string(), |c| format!("'{c}'"));
        conn.execute_unprepared(&format!(
            "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
             received_at, folder, labels, category, is_read, is_spam, is_trash, is_archived) \
             VALUES ('{id}', '{account_id}', 'gmail', 'subject {id}', 's@example.com', \
             'me@example.com', '{received_at}', '{folder}', '{labels}', {category}, \
             {is_read}, {is_spam}, {is_trash}, {is_archived})",
            id = row.id,
            account_id = row.account_id,
            received_at = row.received_at,
            folder = row.folder,
            labels = row.labels,
            is_read = i32::from(row.is_read),
            is_spam = row.is_spam,
            is_trash = row.is_trash,
            is_archived = i32::from(row.is_archived),
        ))
        .await
        .expect("seed");
    }

    fn params() -> ListEmailsParams {
        ListEmailsParams {
            account_id: None,
            category: None,
            label: None,
            is_read: None,
            is_starred: None,
            is_spam: None,
            is_trash: None,
            is_archived: None,
            folder: None,
            limit: None,
            offset: None,
        }
    }

    async fn ids_matching(conn: &DatabaseConnection, params: &ListEmailsParams) -> Vec<String> {
        fetch_email_page(conn, list_filter(params), 50, 0)
            .await
            .expect("page")
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    /// Two-owner scoping pin for `empty_trash`: the account-scoped condition
    /// drives a REAL hard delete here, and the bystander account's trash must
    /// survive it. A dropped `account_id` filter is unrecoverable data loss.
    #[tokio::test]
    async fn empty_trash_scoped_to_account_spares_the_bystander() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            Seed {
                id: "a-trash",
                is_trash: 1,
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "a-keep",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "b-trash",
                account_id: "acct-2",
                is_trash: 1,
                ..Default::default()
            },
        )
        .await;

        let deleted = emails::Entity::delete_many()
            .filter(trashed_condition(Some("acct-1")))
            .exec(&conn)
            .await
            .expect("scoped delete")
            .rows_affected;
        assert_eq!(deleted, 1, "only acct-1's trashed email is deleted");

        let remaining: Vec<String> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .order_by_asc(emails::Column::Id)
            .into_tuple::<(String,)>()
            .all(&conn)
            .await
            .expect("ids")
            .into_iter()
            .map(|(id,)| id)
            .collect();
        assert_eq!(
            remaining,
            vec!["a-keep", "b-trash"],
            "the bystander account's trash survives an account-scoped empty"
        );

        // The unscoped form drains every account's trash.
        let deleted = emails::Entity::delete_many()
            .filter(trashed_condition(None))
            .exec(&conn)
            .await
            .expect("unscoped delete")
            .rows_affected;
        assert_eq!(deleted, 1);
    }

    /// The combination the listing UI sends most often — a folder, an unread
    /// filter and a label — over rows carrying BOTH legacy `received_at` shapes,
    /// so the typed decode has to handle each.
    #[tokio::test]
    async fn listing_filters_combine_folder_unread_and_label() {
        let conn = fresh_conn().await;
        // Wanted: INBOX, unread, labelled WORK — one row per stored shape, on
        // different days (see `mixed_shape_rows_interleave_within_one_day`).
        seed(
            &conn,
            Seed {
                id: "hit-rfc3339",
                received_at: "2026-08-02T09:00:00+00:00",
                labels: "INBOX,WORK",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "hit-naive",
                received_at: "2026-08-01 11:00:00",
                labels: "INBOX,WORK",
                ..Default::default()
            },
        )
        .await;
        // Each of these misses exactly one of the three predicates.
        seed(
            &conn,
            Seed {
                id: "miss-read",
                labels: "INBOX,WORK",
                is_read: true,
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "miss-folder",
                folder: "SENT",
                labels: "SENT,WORK",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "miss-label",
                labels: "INBOX,HOME",
                ..Default::default()
            },
        )
        .await;
        // A label that merely *contains* the filter is not a match: the
        // comma-delimited compare is what stops "WORKFLOW" matching "WORK".
        seed(
            &conn,
            Seed {
                id: "miss-substring",
                labels: "INBOX,WORKFLOW",
                ..Default::default()
            },
        )
        .await;

        let p = ListEmailsParams {
            folder: Some("inbox".into()), // lower-case: the filter is case-insensitive
            is_read: Some(false),
            label: Some("WORK".into()),
            ..params()
        };

        // Newest first.
        assert_eq!(ids_matching(&conn, &p).await, ["hit-rfc3339", "hit-naive"]);
    }

    /// A storage caveat THIS PORT introduces (pl-received-at-mixed-shape):
    /// pre-port, production writes were RFC3339-only, so one shape existed on
    /// disk; the entity's `NaiveDateTime` binds now write the second, naive
    /// `YYYY-MM-DD HH:MM:SS` shape. SQLite compares the TEXT it stored, so
    /// within ONE day a naive row sorts before an RFC3339 row regardless of
    /// the actual instants (`' ' < 'T'`) — on a live upgrade, newly-synced
    /// same-day mail sorts below pre-upgrade mail. Across different days both
    /// shapes order correctly. The entity's module doc records this; the
    /// queued `db-schema-modernization` pipeline (TIMESTAMPTZ) retires it.
    /// Pinned here so a future change to the ordering is a deliberate one.
    #[tokio::test]
    async fn mixed_shape_rows_interleave_within_one_day() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            Seed {
                id: "earlier-instant-rfc3339",
                received_at: "2026-08-01T09:00:00+00:00",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "later-instant-naive",
                received_at: "2026-08-01 11:00:00",
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            ids_matching(&conn, &params()).await,
            ["earlier-instant-rfc3339", "later-instant-naive"],
            "same-day mixed shapes sort by stored text, not by instant"
        );
    }

    /// Gmail user labels are stored with a `$` prefix, and the filter matches
    /// the bare name against both spellings.
    #[tokio::test]
    async fn label_filter_matches_the_dollar_prefixed_variant() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            Seed {
                id: "plain",
                labels: "INBOX,FINANCE",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "prefixed",
                labels: "INBOX,$FINANCE",
                ..Default::default()
            },
        )
        .await;
        seed(&conn, Seed::default()).await;

        let p = ListEmailsParams {
            label: Some("FINANCE".into()),
            ..params()
        };
        let mut ids = ids_matching(&conn, &p).await;
        ids.sort();
        assert_eq!(ids, ["plain", "prefixed"]);
    }

    /// Spam, trash and archived mail is excluded unless asked for by name.
    #[tokio::test]
    async fn listing_excludes_spam_trash_and_archived_by_default() {
        let conn = fresh_conn().await;
        seed(&conn, Seed::default()).await;
        seed(
            &conn,
            Seed {
                id: "spam",
                is_spam: 1,
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "trash",
                is_trash: 1,
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "archived",
                is_archived: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(ids_matching(&conn, &params()).await, ["e1"]);

        for (requested, expected) in [
            (
                ListEmailsParams {
                    is_spam: Some(true),
                    ..params()
                },
                "spam",
            ),
            (
                ListEmailsParams {
                    is_trash: Some(true),
                    ..params()
                },
                "trash",
            ),
            (
                ListEmailsParams {
                    is_archived: Some(true),
                    ..params()
                },
                "archived",
            ),
        ] {
            assert_eq!(ids_matching(&conn, &requested).await, [expected]);
        }
    }

    /// Both legacy stored shapes render as RFC3339 UTC — the one deliberate
    /// output normalization of this port (documented on `format_received_at`).
    #[tokio::test]
    async fn row_mapping_emits_rfc3339_for_both_stored_shapes() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            Seed {
                id: "rfc3339",
                received_at: "2026-08-01T10:30:00+00:00",
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "naive",
                received_at: "2026-08-01 10:30:00",
                ..Default::default()
            },
        )
        .await;

        for id in ["rfc3339", "naive"] {
            let row = fetch_email_row(&conn, id)
                .await
                .expect("query")
                .expect("row present");
            let resp = email_response(row);
            assert_eq!(resp.received_at, "2026-08-01T10:30:00+00:00", "id {id}");
        }
    }

    /// The nullable columns read leniently rather than failing the request, and
    /// `labels` still serialises as `null` when it is NULL.
    #[tokio::test]
    async fn row_mapping_reads_null_columns_as_their_column_defaults() {
        let conn = fresh_conn().await;
        conn.execute_unprepared(
            "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
             received_at, labels, is_read, is_starred, has_attachments, embedding_status, \
             category) VALUES ('sparse', 'acct-1', 'gmail', 's', 'a@b.c', 'd@e.f', \
             '2026-08-01T10:00:00+00:00', NULL, NULL, NULL, NULL, NULL, NULL)",
        )
        .await
        .expect("seed");

        let row = fetch_email_row(&conn, "sparse")
            .await
            .expect("query")
            .expect("row present");
        let resp = email_response(row);

        assert!(!resp.is_read);
        assert!(!resp.is_starred);
        assert!(!resp.has_attachments);
        assert_eq!(resp.embedding_status, "pending");
        assert_eq!(resp.category, "Uncategorized");
        // Unchanged from the pre-port response: the field is optional, so a
        // NULL stays `null` rather than becoming an empty string.
        assert_eq!(resp.labels, None);
        assert_eq!(resp.folder.as_deref(), Some("INBOX"));
    }

    /// The `/counts` reporting shape: totals over the active corpus, unread
    /// derived from the boolean column, and one row per category (NULL included
    /// as its own group), most populous first.
    #[tokio::test]
    async fn category_counts_group_by_category_with_unread_totals() {
        let conn = fresh_conn().await;
        for (id, category, is_read) in [
            ("w1", Some("Work"), false),
            ("w2", Some("Work"), false),
            ("w3", Some("Work"), true),
            ("n1", Some("News"), true),
            ("u1", None, false),
        ] {
            seed(
                &conn,
                Seed {
                    id,
                    category,
                    is_read,
                    ..Default::default()
                },
            )
            .await;
        }
        // Excluded from every count below.
        seed(
            &conn,
            Seed {
                id: "spam",
                category: Some("Work"),
                is_spam: 1,
                ..Default::default()
            },
        )
        .await;
        seed(
            &conn,
            Seed {
                id: "trash",
                category: Some("Work"),
                is_trash: 1,
                ..Default::default()
            },
        )
        .await;

        let (total, unread): (i64, i64) = total_and_unread_query()
            .into_tuple()
            .one(&conn)
            .await
            .expect("query")
            .expect("aggregate row");
        assert_eq!((total, unread), (5, 3));

        let rows: Vec<(Option<String>, i64, i64)> = category_counts_query()
            .into_tuple()
            .all(&conn)
            .await
            .expect("query");
        assert_eq!(
            rows,
            [
                (Some("Work".to_string()), 3, 2),
                (None, 1, 1),
                (Some("News".to_string()), 1, 0),
            ]
        );

        // The enriched endpoint reports the same groups minus the unnamed ones.
        let enriched: Vec<(String, i64, i64)> = enriched_category_query()
            .into_tuple()
            .all(&conn)
            .await
            .expect("query");
        assert_eq!(
            enriched,
            [("Work".to_string(), 3, 2), ("News".to_string(), 1, 0)]
        );
    }

    /// An empty corpus reports zeros rather than a NULL-decode error: `SUM` over
    /// no rows is NULL, which is what the surviving `COALESCE` is for.
    #[tokio::test]
    async fn total_and_unread_are_zero_on_an_empty_mailbox() {
        let conn = fresh_conn().await;
        let counts: (i64, i64) = total_and_unread_query()
            .into_tuple()
            .one(&conn)
            .await
            .expect("query")
            .expect("aggregate row");
        assert_eq!(counts, (0, 0));
    }

    /// A NULL `category` and the literal `'Uncategorized'` (the column's DDL
    /// default) are two SQL groups but one label, so they have to merge — the
    /// endpoint must not report the same category twice.
    #[tokio::test]
    async fn null_and_literal_uncategorized_merge_into_one_bucket() {
        let conn = fresh_conn().await;
        // Two rows land in the literal bucket, two in the NULL bucket. Named
        // "Work" stays ahead of neither until the merge, which is the point.
        for (id, category, is_read) in [
            ("lit-1", Some("Uncategorized"), false),
            ("lit-2", Some("Uncategorized"), true),
            ("null-1", None, false),
            ("null-2", None, true),
            ("work-1", Some("Work"), false),
            ("work-2", Some("Work"), false),
            ("work-3", Some("Work"), false),
        ] {
            seed(
                &conn,
                Seed {
                    id,
                    category,
                    is_read,
                    ..Default::default()
                },
            )
            .await;
        }

        let rows: Vec<(Option<String>, i64, i64)> = category_counts_query()
            .into_tuple()
            .all(&conn)
            .await
            .expect("query");
        // The query itself still reports them separately...
        assert_eq!(rows.len(), 3, "{rows:?}");

        // ...and the fold merges them, keeping most-populous-first.
        let folded = fold_category_counts(rows);
        let shape: Vec<(&str, u64, u64)> = folded
            .iter()
            .map(|c| (c.category.as_str(), c.total, c.unread))
            .collect();
        assert_eq!(shape, [("Uncategorized", 4, 2), ("Work", 3, 3)]);
    }

    #[test]
    fn category_grouping_selects_the_same_expression_it_groups_by() {
        // A `COALESCE(category, 'Uncategorized')` in the select list while
        // grouping by the bare column is what the pre-port SQL did; on
        // PostgreSQL the literal would render as a bind parameter and the
        // grouping is then structurally mismatched. Selecting the bare column
        // and folding in Rust sidesteps that — assert on the UNINTERPOLATED
        // SQL, since `to_string()` would hide a parameter by inlining it.
        let stmt = category_counts_query().build(DbBackend::Postgres);
        // The grouped column is projected bare — not wrapped in a COALESCE
        // whose literal would become a parameter.
        assert!(
            stmt.sql
                .starts_with(r#"SELECT "emails"."category", COUNT(*)"#),
            "{}",
            stmt.sql
        );
        assert!(
            stmt.sql.contains(r#"GROUP BY "emails"."category""#),
            "{}",
            stmt.sql
        );
        // Neither the grouped expression nor the GROUP BY is parameterised, so
        // the two cannot disagree on which parameter they mean. (The `$n` that
        // do appear are the WHERE binds and the unread CASE's boolean.)
        let group_by = stmt.sql.split("GROUP BY").nth(1).expect("GROUP BY clause");
        assert!(!group_by.contains('$'), "{group_by}");
    }

    #[test]
    fn unread_aggregate_compares_a_boolean_rather_than_an_integer() {
        // `is_read` is BOOLEAN on PostgreSQL, where `boolean = integer` has no
        // operator — the pre-port `SUM(CASE WHEN is_read = 0 ...)` is a hard
        // error there and silently fine on SQLite, so only the SQL text shows it.
        let sql = total_and_unread_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("\"is_read\" = FALSE"), "{sql}");
        assert!(sql.contains("\"is_read\" IS NULL"), "{sql}");
        assert!(!sql.contains("\"is_read\" = 0"), "{sql}");
        // THEN/ELSE stay inlined constants so PostgreSQL can type SUM's argument.
        assert!(sql.contains("THEN 1 ELSE 0"), "{sql}");
    }

    #[test]
    fn case_insensitive_filters_lower_both_sides_instead_of_collating() {
        // COLLATE NOCASE is SQLite-only; PostgreSQL has no such collation.
        let sql = email_page_query(
            list_filter(&ListEmailsParams {
                category: Some("Work".into()),
                folder: Some("Inbox".into()),
                ..params()
            }),
            50,
            0,
        )
        .build(DbBackend::Postgres)
        .to_string();
        assert!(!sql.to_uppercase().contains("COLLATE"), "{sql}");
        assert!(sql.contains("LOWER(\"category\") = 'work'"), "{sql}");
        assert!(sql.contains("LOWER(\"folder\") = 'inbox'"), "{sql}");
    }
}
