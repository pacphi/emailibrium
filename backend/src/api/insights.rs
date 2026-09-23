//! Insight API endpoints (S2-06).
//!
//! - GET /api/v1/insights/subscriptions — detected subscriptions
//! - GET /api/v1/insights/recurring     — recurring sender analysis
//! - GET /api/v1/insights/report        — aggregated inbox report
//! - GET /api/v1/insights/temporal      — temporal analytics (volume, categories, day/hour)
//! - GET /api/v1/insights/topics        — topic clusters by AI-assigned category
//!
//! Persistence is single-code-path SeaORM (ADR-036): the `emails` entity owns
//! per-backend encode/decode, so every query below runs unchanged against SQLite
//! and PostgreSQL. Two consequences worth knowing before editing:
//!
//! - **The date/weekday/hour bucketing happens in Rust, not SQL.** The pre-port
//!   queries used `DATE(received_at)`, `STRFTIME('%w', …)` and `STRFTIME('%H', …)`
//!   — all SQLite-only, and `DATE()` additionally returns text on SQLite but a
//!   `DATE` value on PostgreSQL. Fetching the decoded `NaiveDateTime`s and
//!   bucketing them here is the one path that yields identical labels on both
//!   backends (same move as `tools::readonly::insights::fetch_insights`).
//! - **Window cutoffs are computed in Rust too**, because `DATE('now', '-90 days')`
//!   is SQLite's modifier syntax. The cutoff keeps the old midnight-truncated
//!   boundary the `DATE(...)` form produced.

use std::collections::{BTreeMap, HashMap};

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Timelike, Utc};
use sea_orm::sea_query::{Asterisk, Expr, ExprTrait, Func, FunctionCall};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Select,
};
use serde::Serialize;

use crate::db::entities::emails;
use crate::vectors::insights::InsightEngine;
use crate::AppState;

/// How far back the temporal endpoint's daily series reaches.
const TEMPORAL_WINDOW_DAYS: i64 = 90;

// ---------------------------------------------------------------------------
// Temporal analytics types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalInsights {
    /// Daily email counts for last 90 days.
    pub daily_volume: Vec<DailyCount>,
    /// Category counts per day for last 90 days.
    pub category_daily: Vec<CategoryDailyCount>,
    /// Email count by day of week (0=Sunday, 6=Saturday).
    pub day_of_week: Vec<DayOfWeekCount>,
    /// Email count by hour of day (0-23).
    pub hour_of_day: Vec<HourOfDayCount>,
}

#[derive(Debug, Serialize)]
pub struct DailyCount {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDailyCount {
    pub date: String,
    pub category: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct DayOfWeekCount {
    pub day: i32,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct HourOfDayCount {
    pub hour: i32,
    pub count: i64,
}

// ---------------------------------------------------------------------------
// Topic clusters (grouped by AI-assigned category)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicCluster {
    pub id: String,
    pub name: String,
    /// "category" or "subscription" — matches sidebar group prefixes.
    pub group: String,
    pub email_count: i64,
    pub unread_count: i64,
    pub date_range: DateRange,
    pub top_senders: Vec<String>,
    pub sample_subjects: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DateRange {
    pub start: String,
    pub end: String,
}

// ---------------------------------------------------------------------------
// Queries
//
// Each takes a `&DatabaseConnection` rather than `AppState` so the tests at the
// bottom of this file can drive it directly; the handlers pass `&state.orm`.
// ---------------------------------------------------------------------------

/// Render an `emails.received_at` (or an aggregate over it) for JSON output.
///
/// The entity reads the column as `NaiveDateTime` (plain `TIMESTAMP`, ADR-036);
/// output is RFC3339 UTC. For rows the ingestion path wrote (RFC3339 `+00:00`
/// strings on SQLite — the dominant case), this is byte-identical to the raw
/// `String` read it replaces. Rows written by the DDL's `CURRENT_TIMESTAMP`
/// default (naive `YYYY-MM-DD HH:MM:SS`) are normalized to RFC3339 rather than
/// echoed raw — a deliberate output normalization, so the field has one shape.
fn format_received_at(ts: NaiveDateTime) -> String {
    ts.and_utc().to_rfc3339()
}

/// `COUNT(*)`, spelled out at each use site rather than aliased once.
///
/// PostgreSQL rejects select-alias references in HAVING (it accepts them in
/// GROUP BY/ORDER BY), so the expression is repeated wherever it recurs — which
/// is what the pre-port SQL did too.
fn count_star() -> Expr {
    Expr::from(Func::count(Expr::col(Asterisk)))
}

/// `COALESCE(category, 'Uncategorized')` — the cluster's group key.
///
/// The fallback is a raw literal rather than `Expr::val(...)` on purpose: a bound
/// value would render as a *different* placeholder in the SELECT list than in the
/// GROUP BY, and PostgreSQL compares those two expressions structurally before
/// accepting the grouping (two distinct `$n` params don't match, and the query is
/// rejected with "column emails.category must appear in the GROUP BY clause").
/// Inlining the literal makes both sites render identically on both backends.
///
/// The COALESCE itself is load-bearing rather than defensive: migration 001
/// declares `category TEXT DEFAULT 'Uncategorized'`, so real data holds *both*
/// NULLs and the literal string, and they have always collapsed into one cluster.
fn category_expr() -> FunctionCall {
    let args: [Expr; 2] = [
        Expr::col(emails::Column::Category),
        Expr::cust("'Uncategorized'"),
    ];
    Func::coalesce(args)
}

/// Not spam, not trash.
///
/// The pre-port SQL wrapped both flags in `COALESCE(…, 0)`; migration 016
/// declares them `INTEGER NOT NULL DEFAULT 0`, so the COALESCE never fired. They
/// stay `i32` comparisons (not `bool`) because the columns are INTEGER while the
/// neighbouring `is_read` is BOOLEAN — the split typing is load-bearing on
/// PostgreSQL.
fn active_emails() -> Condition {
    Condition::all()
        .add(emails::Column::IsSpam.eq(0_i32))
        .add(emails::Column::IsTrash.eq(0_i32))
}

/// Volume, per-category volume, weekday and hour distributions.
///
/// Four SQL aggregates collapse into two fetches: the 90-day window feeds both
/// daily series, and the unbounded scan feeds both distributions. Buckets are
/// emitted only for keys that actually occur, exactly as the old `GROUP BY` did —
/// an hour with no mail is absent, not zero.
async fn fetch_temporal_insights(conn: &DatabaseConnection) -> Result<TemporalInsights, DbErr> {
    // `DATE('now', '-90 days')` truncated to midnight UTC, so the boundary day is
    // included whole; reproduced here rather than subtracting from the instant.
    let cutoff =
        (Utc::now().date_naive() - Duration::days(TEMPORAL_WINDOW_DAYS)).and_time(NaiveTime::MIN);

    let recent: Vec<(NaiveDateTime, Option<String>)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::ReceivedAt)
        .column(emails::Column::Category)
        .filter(emails::Column::ReceivedAt.gte(cutoff))
        .into_tuple()
        .all(conn)
        .await?;

    // BTreeMaps give the old `ORDER BY date ASC` for free; the category tier
    // additionally orders by name, which the old query left unspecified.
    let mut daily: BTreeMap<String, i64> = BTreeMap::new();
    let mut per_category: BTreeMap<(String, String), i64> = BTreeMap::new();
    for (ts, category) in recent {
        let date = ts.format("%Y-%m-%d").to_string();
        *daily.entry(date.clone()).or_insert(0) += 1;
        // NULL and the DDL's literal default collapse to one label, as the old
        // `COALESCE(category, 'Uncategorized')` grouping did.
        let label = category.unwrap_or_else(|| "Uncategorized".to_string());
        *per_category.entry((date, label)).or_insert(0) += 1;
    }

    // Both distributions cover the whole mailbox (no window, no spam/trash
    // filter), as before the port.
    let stamps: Vec<(NaiveDateTime,)> = emails::Entity::find()
        .select_only()
        .column(emails::Column::ReceivedAt)
        .into_tuple()
        .all(conn)
        .await?;

    let mut day_of_week: BTreeMap<i32, i64> = BTreeMap::new();
    let mut hour_of_day: BTreeMap<i32, i64> = BTreeMap::new();
    for (ts,) in stamps {
        // `STRFTIME('%w', …)` numbers Sunday as 0, which is what
        // `num_days_from_sunday()` returns.
        *day_of_week
            .entry(ts.weekday().num_days_from_sunday() as i32)
            .or_insert(0) += 1;
        *hour_of_day.entry(ts.hour() as i32).or_insert(0) += 1;
    }

    Ok(TemporalInsights {
        daily_volume: daily
            .into_iter()
            .map(|(date, count)| DailyCount { date, count })
            .collect(),
        category_daily: per_category
            .into_iter()
            .map(|((date, category), count)| CategoryDailyCount {
                date,
                category,
                count,
            })
            .collect(),
        day_of_week: day_of_week
            .into_iter()
            .map(|(day, count)| DayOfWeekCount { day, count })
            .collect(),
        hour_of_day: hour_of_day
            .into_iter()
            .map(|(hour, count)| HourOfDayCount { hour, count })
            .collect(),
    })
}

/// Per-category row: label, total, first seen, last seen.
type CategoryAggregate = (String, i64, NaiveDateTime, NaiveDateTime);

/// The query behind [`fetch_category_aggregates`], split out so a test can assert
/// the SQL text it produces for PostgreSQL without a live server.
fn category_aggregates_query() -> Select<emails::Entity> {
    emails::Entity::find()
        .select_only()
        .expr_as(category_expr(), "cat")
        .expr_as(count_star(), "cnt")
        .expr_as(
            Func::min(Expr::col(emails::Column::ReceivedAt)),
            "first_seen",
        )
        .expr_as(
            Func::max(Expr::col(emails::Column::ReceivedAt)),
            "last_seen",
        )
        .filter(active_emails())
        .group_by(Expr::expr(category_expr()))
        .order_by_desc(count_star())
}

/// Category totals with their date range, largest first.
///
/// `MIN`/`MAX(received_at)` decode as `NaiveDateTime` rather than the pre-port
/// `String`: on PostgreSQL those aggregates return a TIMESTAMP, so the old
/// `String` decode was a runtime error waiting there. Callers render them through
/// [`format_received_at`].
async fn fetch_category_aggregates(
    conn: &DatabaseConnection,
) -> Result<Vec<CategoryAggregate>, DbErr> {
    category_aggregates_query().into_tuple().all(conn).await
}

/// Unread totals per category.
///
/// This is the pre-port `SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END)` as a
/// second grouped COUNT instead. `is_read` is BOOLEAN (migration 001), so
/// `is_read = 0` is a type error on PostgreSQL; the typed `.eq(false)` predicate
/// is portable, and rows with a NULL `is_read` fall outside it — which is exactly
/// where the old CASE's `ELSE 0` put them.
async fn fetch_unread_counts(conn: &DatabaseConnection) -> Result<HashMap<String, i64>, DbErr> {
    let rows: Vec<(String, i64)> = emails::Entity::find()
        .select_only()
        .expr_as(category_expr(), "cat")
        .expr_as(count_star(), "cnt")
        .filter(active_emails())
        .filter(emails::Column::IsRead.eq(false))
        .group_by(Expr::expr(category_expr()))
        .into_tuple()
        .all(conn)
        .await?;
    Ok(rows.into_iter().collect())
}

/// The three addresses that sent this category the most mail.
async fn fetch_top_senders(
    conn: &DatabaseConnection,
    category: &str,
) -> Result<Vec<String>, DbErr> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::FromAddr)
        .filter(active_emails())
        .filter(Expr::expr(category_expr()).eq(category))
        .group_by(emails::Column::FromAddr)
        .order_by_desc(count_star())
        .limit(3)
        .into_tuple()
        .all(conn)
        .await
}

/// The three most recent subjects in this category.
async fn fetch_sample_subjects(
    conn: &DatabaseConnection,
    category: &str,
) -> Result<Vec<String>, DbErr> {
    emails::Entity::find()
        .select_only()
        .column(emails::Column::Subject)
        .filter(active_emails())
        .filter(Expr::expr(category_expr()).eq(category))
        .order_by_desc(emails::Column::ReceivedAt)
        .limit(3)
        .into_tuple()
        .all(conn)
        .await
}

/// Assemble one cluster per category, largest first.
async fn fetch_topic_clusters(conn: &DatabaseConnection) -> Result<Vec<TopicCluster>, DbErr> {
    let aggregates = fetch_category_aggregates(conn).await?;
    let unread = fetch_unread_counts(conn).await?;

    let mut clusters = Vec::with_capacity(aggregates.len());
    for (category, count, first_seen, last_seen) in aggregates {
        let top_senders = fetch_top_senders(conn, &category).await?;
        let sample_subjects = fetch_sample_subjects(conn, &category).await?;

        let group = crate::api::emails::categorize_group(&category).to_string();
        let prefix = if group == "subscription" {
            "sub-"
        } else {
            "cat-"
        };
        let id = format!("{prefix}{category}");
        let unread_count = unread.get(&category).copied().unwrap_or(0);
        clusters.push(TopicCluster {
            id,
            name: category,
            group,
            email_count: count,
            unread_count,
            date_range: DateRange {
                start: format_received_at(first_seen),
                end: format_received_at(last_seen),
            },
            top_senders,
            sample_subjects,
        });
    }

    Ok(clusters)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Build insight API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscriptions", get(subscriptions))
        .route("/recurring-senders", get(recurring))
        .route("/report", get(report))
        .route("/temporal", get(temporal_insights))
        .route("/topics", get(topic_clusters))
}

/// GET /api/v1/insights/subscriptions
async fn subscriptions(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::vectors::insights::SubscriptionInsight>>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(subs))
}

/// GET /api/v1/insights/recurring
async fn recurring(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::vectors::insights::SubscriptionInsight>>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    // Reuse the subscription detection which returns full SubscriptionInsight
    // (senderAddress, senderDomain, frequency, lastSeen, etc.) — matching
    // what the frontend SendersPanel and TopicsPanel expect.
    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(subs))
}

/// GET /api/v1/insights/report
async fn report(
    State(state): State<AppState>,
) -> Result<Json<crate::vectors::insights::InboxReport>, (StatusCode, String)> {
    let engine = InsightEngine::new(state.db.clone(), state.vector_service.store.clone());

    let report = engine
        .generate_report()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(report))
}

/// GET /api/v1/insights/temporal
async fn temporal_insights(
    State(state): State<AppState>,
) -> Result<Json<TemporalInsights>, (StatusCode, String)> {
    fetch_temporal_insights(&state.orm)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// GET /api/v1/insights/topics — topic clusters grouped by AI-assigned category.
async fn topic_clusters(
    State(state): State<AppState>,
) -> Result<Json<Vec<TopicCluster>>, (StatusCode, String)> {
    fetch_topic_clusters(&state.orm)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

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

    /// Seed one email, writing `received_at` as the given literal so a test can
    /// pin behavior across BOTH stored timestamp shapes (the pre-port RFC3339
    /// String binds and the DDL default's naive form).
    ///
    /// `category` / `is_read` are written as raw SQL so NULL is reachable.
    #[allow(clippy::too_many_arguments)]
    async fn seed(
        conn: &DatabaseConnection,
        id: &str,
        received_at: &str,
        category: Option<&str>,
        is_read: Option<bool>,
        from_addr: &str,
        is_spam: i32,
        is_trash: i32,
    ) {
        let cat = category
            .map(|c| format!("'{c}'"))
            .unwrap_or_else(|| "NULL".to_string());
        let read = is_read
            .map(|r| i32::from(r).to_string())
            .unwrap_or_else(|| "NULL".to_string());
        conn.execute_unprepared(&format!(
            "INSERT INTO emails (id, account_id, provider, subject, from_addr, to_addrs, \
             received_at, category, is_read, is_spam, is_trash) \
             VALUES ('{id}', 'a1', 'gmail', 'subject {id}', '{from_addr}', 'r@example.com', \
             '{received_at}', {cat}, {read}, {is_spam}, {is_trash})"
        ))
        .await
        .expect("seed");
    }

    /// A timestamp literal inside the 90-day window, in the RFC3339 shape the
    /// pre-port ingestion path wrote.
    fn recent_rfc3339(hour: u32) -> String {
        let day = Utc::now().date_naive() - Duration::days(1);
        format!("{}T{hour:02}:00:00+00:00", day.format("%Y-%m-%d"))
    }

    /// The same instant in the DDL default's naive shape.
    fn recent_naive(hour: u32) -> String {
        let day = Utc::now().date_naive() - Duration::days(1);
        format!("{} {hour:02}:00:00", day.format("%Y-%m-%d"))
    }

    /// Both legacy stored shapes bucket into the same day, and the category tier
    /// folds NULL together with the DDL's literal default.
    #[tokio::test]
    async fn temporal_buckets_span_both_legacy_timestamp_shapes() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            "e1",
            &recent_rfc3339(9),
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "e2",
            &recent_naive(9),
            None,
            Some(true),
            "b@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "e3",
            &recent_naive(11),
            Some("Uncategorized"),
            Some(true),
            "c@example.com",
            0,
            0,
        )
        .await;

        let t = fetch_temporal_insights(&conn).await.expect("temporal");

        let day = (Utc::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(t.daily_volume.len(), 1);
        assert_eq!(t.daily_volume[0].date, day);
        assert_eq!(t.daily_volume[0].count, 3);

        // NULL + 'Uncategorized' = one label with 2, alongside Work's 1.
        assert_eq!(t.category_daily.len(), 2);
        let uncategorized = t
            .category_daily
            .iter()
            .find(|c| c.category == "Uncategorized")
            .expect("uncategorized bucket");
        assert_eq!(uncategorized.count, 2);
        assert_eq!(uncategorized.date, day);
        let work = t
            .category_daily
            .iter()
            .find(|c| c.category == "Work")
            .expect("work bucket");
        assert_eq!(work.count, 1);
    }

    /// Weekday and hour buckets carry the labels `STRFTIME('%w'/'%H', …)` used
    /// to produce, and only for keys that actually occur.
    #[tokio::test]
    async fn weekday_and_hour_buckets_match_strftime_numbering() {
        let conn = fresh_conn().await;
        // 2026-08-02 is a Sunday (%w = 0); 2026-08-04 is a Tuesday (%w = 2).
        seed(
            &conn,
            "e1",
            "2026-08-02T07:15:00+00:00",
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "e2",
            "2026-08-04 23:45:00",
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;

        let t = fetch_temporal_insights(&conn).await.expect("temporal");

        assert_eq!(
            t.day_of_week
                .iter()
                .map(|d| (d.day, d.count))
                .collect::<Vec<_>>(),
            vec![(0, 1), (2, 1)]
        );
        assert_eq!(
            t.hour_of_day
                .iter()
                .map(|h| (h.hour, h.count))
                .collect::<Vec<_>>(),
            vec![(7, 1), (23, 1)]
        );
    }

    /// The Rust-computed cutoff drops rows older than the window while keeping
    /// the whole-mailbox distributions intact.
    #[tokio::test]
    async fn temporal_window_excludes_rows_older_than_ninety_days() {
        let conn = fresh_conn().await;
        let old = (Utc::now().date_naive() - Duration::days(TEMPORAL_WINDOW_DAYS + 10))
            .format("%Y-%m-%d")
            .to_string();
        seed(
            &conn,
            "old",
            &format!("{old}T08:00:00+00:00"),
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "new",
            &recent_rfc3339(8),
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;

        let t = fetch_temporal_insights(&conn).await.expect("temporal");

        assert_eq!(t.daily_volume.len(), 1, "only the in-window day survives");
        assert_eq!(t.daily_volume[0].count, 1);
        // Distributions are unwindowed, so both rows still land in hour 8.
        assert_eq!(t.hour_of_day.len(), 1);
        assert_eq!(t.hour_of_day[0].count, 2);
    }

    /// NULL and the DDL's literal `'Uncategorized'` default collapse into one
    /// cluster, as `COALESCE(category, 'Uncategorized')` has always done — two
    /// clusters would mean two entries sharing one `id`.
    #[tokio::test]
    async fn topic_clusters_merge_null_and_literal_uncategorized() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            "e1",
            "2026-07-01T08:00:00+00:00",
            None,
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "e2",
            "2026-07-02 09:00:00",
            Some("Uncategorized"),
            Some(true),
            "b@example.com",
            0,
            0,
        )
        .await;

        let clusters = fetch_topic_clusters(&conn).await.expect("clusters");

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].name, "Uncategorized");
        assert_eq!(clusters[0].email_count, 2);
    }

    /// Unread counts only rows whose `is_read` is explicitly false; a NULL reads
    /// as read, which is where the old `CASE … ELSE 0` put it.
    #[tokio::test]
    async fn unread_count_treats_null_is_read_as_read() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            "unread",
            "2026-07-01T08:00:00+00:00",
            Some("Work"),
            Some(false),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "read",
            "2026-07-01T09:00:00+00:00",
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "null",
            "2026-07-01T10:00:00+00:00",
            Some("Work"),
            None,
            "a@example.com",
            0,
            0,
        )
        .await;

        let clusters = fetch_topic_clusters(&conn).await.expect("clusters");

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].email_count, 3);
        assert_eq!(clusters[0].unread_count, 1);
    }

    /// Spam and trash stay out of every cluster tier.
    #[tokio::test]
    async fn topic_clusters_exclude_spam_and_trash() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            "keep",
            "2026-07-01T08:00:00+00:00",
            Some("Work"),
            Some(false),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "spam",
            "2026-07-01T09:00:00+00:00",
            Some("Work"),
            Some(false),
            "spam@example.com",
            1,
            0,
        )
        .await;
        seed(
            &conn,
            "trash",
            "2026-07-01T10:00:00+00:00",
            Some("Work"),
            Some(false),
            "trash@example.com",
            0,
            1,
        )
        .await;

        let clusters = fetch_topic_clusters(&conn).await.expect("clusters");

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].email_count, 1);
        assert_eq!(clusters[0].unread_count, 1);
        assert_eq!(clusters[0].top_senders, vec!["a@example.com".to_string()]);
    }

    /// `MIN`/`MAX(received_at)` decode as timestamps across both stored shapes
    /// and are emitted as RFC3339 UTC — the normalization documented on
    /// `format_received_at`.
    #[tokio::test]
    async fn cluster_date_range_is_rfc3339_across_both_shapes() {
        let conn = fresh_conn().await;
        seed(
            &conn,
            "first",
            "2026-07-01 08:00:00",
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;
        seed(
            &conn,
            "last",
            "2026-07-09T17:30:00+00:00",
            Some("Work"),
            Some(true),
            "a@example.com",
            0,
            0,
        )
        .await;

        let clusters = fetch_topic_clusters(&conn).await.expect("clusters");

        assert_eq!(clusters[0].date_range.start, "2026-07-01T08:00:00+00:00");
        assert_eq!(clusters[0].date_range.end, "2026-07-09T17:30:00+00:00");
    }

    /// Senders rank by volume and both sample lists cap at three.
    #[tokio::test]
    async fn cluster_samples_rank_and_cap_at_three() {
        let conn = fresh_conn().await;
        for (i, sender) in [
            "top@example.com",
            "top@example.com",
            "top@example.com",
            "mid@example.com",
            "mid@example.com",
            "low@example.com",
            "rare@example.com",
        ]
        .into_iter()
        .enumerate()
        {
            seed(
                &conn,
                &format!("e{i}"),
                &format!("2026-07-{:02}T08:00:00+00:00", i + 1),
                Some("Work"),
                Some(true),
                sender,
                0,
                0,
            )
            .await;
        }

        let clusters = fetch_topic_clusters(&conn).await.expect("clusters");

        assert_eq!(
            clusters[0].top_senders,
            vec![
                "top@example.com".to_string(),
                "mid@example.com".to_string(),
                // Third place is a tie between the two single-email senders;
                // only the count ordering is pinned.
                clusters[0].top_senders[2].clone(),
            ]
        );
        assert_eq!(clusters[0].sample_subjects.len(), 3);
        // Newest first: e6, e5, e4.
        assert_eq!(clusters[0].sample_subjects[0], "subject e6");
        assert_eq!(clusters[0].sample_subjects[2], "subject e4");
    }

    /// The grouping expression must render identically in the SELECT list and
    /// the GROUP BY, with the fallback inlined rather than bound — PostgreSQL
    /// matches those two expressions structurally, and two distinct `$n`
    /// placeholders would not match.
    #[test]
    fn category_grouping_renders_identically_on_postgres() {
        // `.sql` rather than `.to_string()`: the latter interpolates bound values
        // back into the text, which would hide the very thing under test.
        let sql = category_aggregates_query().build(DbBackend::Postgres).sql;
        let coalesce = r#"COALESCE("category", 'Uncategorized')"#;
        assert_eq!(
            sql.matches(coalesce).count(),
            2,
            "SELECT list and GROUP BY must both carry the literal form: {sql}"
        );
        assert!(
            sql.contains(&format!("GROUP BY {coalesce}")),
            "grouping is by expression, not by alias: {sql}"
        );
    }
}
