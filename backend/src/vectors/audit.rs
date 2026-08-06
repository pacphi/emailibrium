//! Audit logging middleware for cloud API calls (ADR-008, ADR-012, item #39).
//!
//! Wraps cloud API call functions with timing and logging, storing every
//! call in the `cloud_api_audit_log` table with provider, model, tokens,
//! latency, status, and user context.
//!
//! Single-code-path SeaORM (ADR-036): the `cloud_api_audit_log` entity owns
//! encode/decode per backend, so the ten-column insert, the filtered/paginated
//! read and the summary aggregates are each one statement that runs unchanged on
//! both. `timestamp` is plain `TIMESTAMP` (no zone), so writes bind
//! `.naive_utc()` — the `pl-timestamp-write-tz` fix, see [`CloudApiAuditLogger::log`].
//! ADR-035 §2.7's `AVG(integer)`-is-NUMERIC divergence survives the port as one
//! portable `CAST(... AS double precision)`; see [`CloudApiAuditLogger::get_summary`].

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Alias, Expr, Func};
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, Order, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Select,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::error::VectorError;
use crate::db::entities::cloud_api_audit_log;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single entry in the cloud API audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudApiAuditEntry {
    pub id: Option<i64>,
    /// Timestamp of the API call.
    pub timestamp: DateTime<Utc>,
    /// Cloud provider name (e.g., "openai", "anthropic", "cohere", "gemini").
    pub provider: String,
    /// Model identifier used.
    pub model: String,
    /// Number of input tokens (if known).
    pub input_tokens: Option<i64>,
    /// Number of output tokens (if known).
    pub output_tokens: Option<i64>,
    /// Latency in milliseconds.
    pub latency_ms: i64,
    /// User ID that triggered the call (if applicable).
    pub user_id: Option<String>,
    /// Type of request (e.g., "embedding", "completion", "classification").
    pub request_type: String,
    /// HTTP status code or outcome (e.g., "200", "429", "error").
    pub status: String,
    /// Optional error message if the call failed.
    pub error_message: Option<String>,
}

/// Summary statistics for cloud API usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    /// Total number of API calls.
    pub total_calls: i64,
    /// Total input tokens consumed.
    pub total_input_tokens: i64,
    /// Total output tokens consumed.
    pub total_output_tokens: i64,
    /// Average latency in milliseconds.
    pub avg_latency_ms: f64,
    /// Number of failed calls.
    pub error_count: i64,
    /// Per-provider call counts.
    pub by_provider: Vec<ProviderStats>,
}

/// Per-provider usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStats {
    pub provider: String,
    pub call_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_latency_ms: f64,
    pub error_count: i64,
}

/// The whole-table half of [`CloudApiAuditLogger::get_summary`]'s aggregates.
#[derive(Debug, FromQueryResult)]
struct SummaryTotals {
    total_calls: i64,
    total_input_tokens: i64,
    total_output_tokens: i64,
    avg_latency_ms: Option<f64>,
    error_count: i64,
}

/// The per-provider half, grouped.
#[derive(Debug, FromQueryResult)]
struct SummaryByProvider {
    provider: String,
    call_count: i64,
    total_input_tokens: i64,
    total_output_tokens: i64,
    avg_latency_ms: Option<f64>,
    error_count: i64,
}

// ---------------------------------------------------------------------------
// AuditLogger
// ---------------------------------------------------------------------------

/// Audit logger for cloud API calls.
///
/// Stores detailed logs of every cloud API interaction for compliance,
/// debugging, and cost tracking.
pub struct CloudApiAuditLogger {
    conn: DatabaseConnection,
}

impl CloudApiAuditLogger {
    /// Create a new audit logger.
    pub fn new(db: Arc<Database>) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Retained no-op: migrations own this schema.
    ///
    /// Migration 008 creates `cloud_api_audit_log` and its three indexes for both
    /// backends, with DDL identical to what this method used to run at runtime —
    /// including the per-backend `AUTOINCREMENT` / `GENERATED ALWAYS AS IDENTITY`
    /// split it hand-wrote. A second copy of a schema is a copy free to drift, so
    /// ADR-036 keeps only the migration. The signature stays until
    /// `vectors/mod.rs` is ported and its call site drops.
    pub async fn ensure_table(&self) -> Result<(), VectorError> {
        Ok(())
    }

    /// Log a cloud API call.
    pub async fn log(&self, entry: &CloudApiAuditEntry) -> Result<(), VectorError> {
        debug!(
            provider = %entry.provider,
            model = %entry.model,
            latency_ms = entry.latency_ms,
            status = %entry.status,
            "Audit: cloud API call"
        );

        cloud_api_audit_log::Entity::insert(cloud_api_audit_log::ActiveModel {
            // pl-timestamp-write-tz: `timestamp` is TIMESTAMP (no zone), so the
            // bind is the naive UTC instant. The `DateTime<Utc>` bind this call
            // site used before the port goes through PostgreSQL's
            // timestamptz->timestamp assignment cast, which rewrites the value
            // into the session's TimeZone — a silent shift on any non-UTC session.
            timestamp: Set(entry.timestamp.naive_utc()),
            provider: Set(entry.provider.clone()),
            model: Set(entry.model.clone()),
            // The token/latency columns are real 4-byte INTEGERs on PostgreSQL;
            // saturate rather than wrap on the (not-in-practice) overflow.
            input_tokens: Set(entry.input_tokens.map(narrow_to_i32)),
            output_tokens: Set(entry.output_tokens.map(narrow_to_i32)),
            latency_ms: Set(narrow_to_i32(entry.latency_ms)),
            user_id: Set(entry.user_id.clone()),
            request_type: Set(entry.request_type.clone()),
            status: Set(entry.status.clone()),
            error_message: Set(entry.error_message.clone()),
            ..Default::default()
        })
        .exec_without_returning(&self.conn)
        .await
        .map_err(|e| db_error("log cloud API call", e))?;

        Ok(())
    }

    /// Get paginated audit log entries (newest first).
    pub async fn get_log(
        &self,
        page: u32,
        per_page: u32,
        provider_filter: Option<&str>,
    ) -> Result<(Vec<CloudApiAuditEntry>, i64), VectorError> {
        let offset = u64::from(page.saturating_sub(1)) * u64::from(per_page);

        // One filtered query, reused for the count and the page — the optional
        // WHERE is a builder branch now, not a second query string.
        let mut query = cloud_api_audit_log::Entity::find();
        if let Some(provider) = provider_filter {
            query = query.filter(cloud_api_audit_log::Column::Provider.eq(provider));
        }

        let total = query
            .clone()
            .count(&self.conn)
            .await
            .map_err(|e| db_error("count cloud API audit log", e))? as i64;

        let rows = query
            .order_by(cloud_api_audit_log::Column::Timestamp, Order::Desc)
            .limit(u64::from(per_page))
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(|e| db_error("read cloud API audit log", e))?;

        let entries = rows.into_iter().map(to_entry).collect();

        Ok((entries, total))
    }

    /// Get summary statistics, optionally filtered by time range.
    pub async fn get_summary(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<AuditSummary, VectorError> {
        let since_ts = since.unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
        // pl-timestamp-write-tz applies to comparison binds too: the column is
        // TIMESTAMP (no zone), so the cutoff crosses as a naive UTC instant.
        let since_naive = since_ts.naive_utc();

        let totals = totals_query(since_naive)
            .into_model::<SummaryTotals>()
            .one(&self.conn)
            .await
            .map_err(|e| db_error("summarize cloud API usage", e))?;

        let provider_rows = by_provider_query(since_naive)
            .into_model::<SummaryByProvider>()
            .all(&self.conn)
            .await
            .map_err(|e| db_error("summarize cloud API usage by provider", e))?;

        let by_provider = provider_rows
            .into_iter()
            .map(|r| ProviderStats {
                provider: r.provider,
                call_count: r.call_count,
                total_input_tokens: r.total_input_tokens,
                total_output_tokens: r.total_output_tokens,
                avg_latency_ms: r.avg_latency_ms.unwrap_or(0.0),
                error_count: r.error_count,
            })
            .collect();

        // An empty table yields no aggregate row on neither backend's grouped
        // query and a single all-zero row on the ungrouped one; treat a missing
        // row as the zero summary either way.
        Ok(match totals {
            Some(t) => AuditSummary {
                total_calls: t.total_calls,
                total_input_tokens: t.total_input_tokens,
                total_output_tokens: t.total_output_tokens,
                avg_latency_ms: t.avg_latency_ms.unwrap_or(0.0),
                error_count: t.error_count,
                by_provider,
            },
            None => AuditSummary {
                total_calls: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                avg_latency_ms: 0.0,
                error_count: 0,
                by_provider,
            },
        })
    }
}

/// Whole-table aggregates over the window. Separate from `get_summary` so the
/// SQL it generates can be pinned for both backends without a live database.
fn totals_query(since_naive: chrono::NaiveDateTime) -> Select<cloud_api_audit_log::Entity> {
    cloud_api_audit_log::Entity::find()
        .select_only()
        .column_as(Expr::cust("COUNT(*)"), "total_calls")
        .column_as(
            Expr::cust("COALESCE(SUM(input_tokens), 0)"),
            "total_input_tokens",
        )
        .column_as(
            Expr::cust("COALESCE(SUM(output_tokens), 0)"),
            "total_output_tokens",
        )
        .expr_as(avg_latency_expr(), "avg_latency_ms")
        .column_as(error_count_expr(), "error_count")
        .filter(cloud_api_audit_log::Column::Timestamp.gte(since_naive))
}

/// The same aggregates grouped by provider, busiest first.
fn by_provider_query(since_naive: chrono::NaiveDateTime) -> Select<cloud_api_audit_log::Entity> {
    cloud_api_audit_log::Entity::find()
        .select_only()
        .column(cloud_api_audit_log::Column::Provider)
        .column_as(Expr::cust("COUNT(*)"), "call_count")
        .column_as(
            Expr::cust("COALESCE(SUM(input_tokens), 0)"),
            "total_input_tokens",
        )
        .column_as(
            Expr::cust("COALESCE(SUM(output_tokens), 0)"),
            "total_output_tokens",
        )
        .expr_as(avg_latency_expr(), "avg_latency_ms")
        .column_as(error_count_expr(), "error_count")
        .filter(cloud_api_audit_log::Column::Timestamp.gte(since_naive))
        .group_by(cloud_api_audit_log::Column::Provider)
        .order_by_desc(Expr::cust("COUNT(*)"))
}

/// `AVG(latency_ms)` cast to a float, in one SQL text both backends accept.
///
/// ADR-035 §2.7: PostgreSQL's `avg(integer)` returns `NUMERIC`, which sqlx cannot
/// decode into `f64` without the `bigdecimal`/`rust_decimal` feature this crate
/// deliberately doesn't carry — so the cast has to be in the SQL. The `::float8`
/// spelling the pre-port PostgreSQL arm used is PostgreSQL-only syntax; standard
/// `CAST(... AS double precision)` is not. On SQLite the type name is read for
/// affinity, and `double precision` carries `DOUB`, so it takes REAL affinity and
/// the aggregate decodes as a float there too (pinned by
/// `test_summary_avg_latency_decodes_as_float`). That collapses §2.7's
/// "genuinely different SQL text per backend" case to a single expression, so no
/// `Statement::from_sql_and_values` escape hatch is needed here.
fn avg_latency_expr() -> sea_orm::sea_query::FunctionCall {
    Func::cast_as(
        Func::avg(Expr::col(cloud_api_audit_log::Column::LatencyMs)),
        Alias::new("double precision"),
    )
}

/// Failed-call count. `SUM(CASE ...)` over integer literals is portable text:
/// PostgreSQL widens it to `BIGINT` and SQLite to an integer, both `i64`.
fn error_count_expr() -> Expr {
    Expr::cust("COALESCE(SUM(CASE WHEN status != '200' AND status != 'ok' THEN 1 ELSE 0 END), 0)")
}

fn to_entry(model: cloud_api_audit_log::Model) -> CloudApiAuditEntry {
    CloudApiAuditEntry {
        // id and the counters are INTEGER/INT4 (i32) in both dialects; the public
        // entry type keeps its i64 widths.
        id: Some(i64::from(model.id)),
        // Stored naive, always UTC by construction (see `log`).
        timestamp: model.timestamp.and_utc(),
        provider: model.provider,
        model: model.model,
        input_tokens: model.input_tokens.map(i64::from),
        output_tokens: model.output_tokens.map(i64::from),
        latency_ms: i64::from(model.latency_ms),
        user_id: model.user_id,
        request_type: model.request_type,
        status: model.status,
        error_message: model.error_message,
    }
}

/// Saturating i64 -> i32 for the INTEGER counter columns.
fn narrow_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Map a SeaORM error onto this module's error type.
///
/// `VectorError` has no `DbErr` variant yet (`DatabaseError` wraps a
/// `sqlx::Error`, and SeaORM only ever hands back an `Arc`-shared one), so the
/// operation name is prefixed to keep the message diagnosable. Callers map every
/// variant here to the same HTTP status, so this is a message-shape change only.
fn db_error(operation: &str, err: DbErr) -> VectorError {
    VectorError::StoreFailed(format!("{operation}: {err}"))
}

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

/// Helper to time a cloud API call and produce an audit entry.
///
/// Usage:
/// ```ignore
/// let timer = AuditTimer::start("openai", "gpt-4o-mini", "completion", user_id);
/// let result = call_openai_api().await;
/// let entry = timer.finish(input_tokens, output_tokens, &result);
/// audit_logger.log(&entry).await?;
/// ```
pub struct AuditTimer {
    start: std::time::Instant,
    provider: String,
    model: String,
    request_type: String,
    user_id: Option<String>,
}

impl AuditTimer {
    /// Start timing a cloud API call.
    pub fn start(provider: &str, model: &str, request_type: &str, user_id: Option<String>) -> Self {
        Self {
            start: std::time::Instant::now(),
            provider: provider.to_string(),
            model: model.to_string(),
            request_type: request_type.to_string(),
            user_id,
        }
    }

    /// Finish timing and produce an audit entry for a successful call.
    pub fn finish_ok(
        self,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) -> CloudApiAuditEntry {
        CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: self.provider,
            model: self.model,
            input_tokens,
            output_tokens,
            latency_ms: self.start.elapsed().as_millis() as i64,
            user_id: self.user_id,
            request_type: self.request_type,
            status: "200".to_string(),
            error_message: None,
        }
    }

    /// Finish timing and produce an audit entry for a failed call.
    pub fn finish_error(self, error: &str) -> CloudApiAuditEntry {
        warn!(
            provider = %self.provider,
            model = %self.model,
            error = %error,
            "Cloud API call failed"
        );
        CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: self.provider,
            model: self.model,
            input_tokens: None,
            output_tokens: None,
            latency_ms: self.start.elapsed().as_millis() as i64,
            user_id: self.user_id,
            request_type: self.request_type,
            status: "error".to_string(),
            error_message: Some(error.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    /// In-memory SQLite carrying migration 008, which owns this module's table.
    /// Replaying the real migration is what replaced the runtime DDL
    /// `ensure_table()` used to run — tests and production now agree on one
    /// schema definition, including `timestamp`'s no-zone `TIMESTAMP` type.
    async fn setup_db() -> Arc<Database> {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        let raw = include_str!("../../migrations/sqlite/008_cloud_api_audit.sql");
        // Strip line comments before splitting on ';'.
        let cleaned: String = raw
            .lines()
            .map(|l| {
                if let Some(idx) = l.find("--") {
                    &l[..idx]
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        for stmt in cleaned.split(';') {
            let s = stmt.trim();
            if !s.is_empty() {
                conn.execute_unprepared(s).await.expect("migrate");
            }
        }

        Arc::new(db)
    }

    fn sample_entry(provider: &str, latency_ms: i64) -> CloudApiAuditEntry {
        CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: provider.to_string(),
            model: "test".to_string(),
            input_tokens: None,
            output_tokens: None,
            latency_ms,
            user_id: None,
            request_type: "test".to_string(),
            status: "200".to_string(),
            error_message: None,
        }
    }

    /// The table comes from the migration; `ensure_table` is the retained no-op
    /// its composition-root caller still invokes, and stays safe to call twice.
    #[tokio::test]
    async fn test_ensure_table() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);
        logger.ensure_table().await.unwrap();
        logger.ensure_table().await.unwrap();

        let (entries, total) = logger.get_log(1, 10, None).await.unwrap();
        assert!(entries.is_empty());
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_log_and_retrieve() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        let entry = CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            input_tokens: Some(100),
            output_tokens: Some(50),
            latency_ms: 250,
            user_id: Some("user-123".to_string()),
            request_type: "completion".to_string(),
            status: "200".to_string(),
            error_message: None,
        };

        logger.log(&entry).await.unwrap();

        let (entries, total) = logger.get_log(1, 10, None).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].provider, "openai");
        assert_eq!(entries[0].model, "gpt-4o-mini");
        assert_eq!(entries[0].input_tokens, Some(100));
        assert_eq!(entries[0].latency_ms, 250);
        assert_eq!(entries[0].user_id.as_deref(), Some("user-123"));
    }

    #[tokio::test]
    async fn test_log_error_call() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        let entry = CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: 5000,
            user_id: None,
            request_type: "completion".to_string(),
            status: "429".to_string(),
            error_message: Some("Rate limited".to_string()),
        };

        logger.log(&entry).await.unwrap();

        let (entries, _) = logger.get_log(1, 10, None).await.unwrap();
        assert_eq!(entries[0].status, "429");
        assert_eq!(entries[0].error_message.as_deref(), Some("Rate limited"));
    }

    #[tokio::test]
    async fn test_filter_by_provider() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        for provider in &["openai", "openai", "anthropic"] {
            logger.log(&sample_entry(provider, 100)).await.unwrap();
        }

        let (entries, total) = logger.get_log(1, 10, Some("openai")).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);

        let (entries, total) = logger.get_log(1, 10, Some("anthropic")).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
    }

    /// The provider filter and the page window are one query: the count reports
    /// the filtered total (not the table total) and the window applies within the
    /// filter, so page 2 of a 3-row provider slice holds exactly one row.
    #[tokio::test]
    async fn test_filter_and_pagination_compose() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        for provider in &["openai", "openai", "openai", "anthropic", "anthropic"] {
            logger.log(&sample_entry(provider, 100)).await.unwrap();
        }

        let (page1, total) = logger.get_log(1, 2, Some("openai")).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(page1.len(), 2);
        assert!(page1.iter().all(|e| e.provider == "openai"));

        let (page2, total) = logger.get_log(2, 2, Some("openai")).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].provider, "openai");
    }

    #[tokio::test]
    async fn test_summary() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        for i in 0..5 {
            let entry = CloudApiAuditEntry {
                id: None,
                timestamp: Utc::now(),
                provider: if i < 3 { "openai" } else { "anthropic" }.to_string(),
                model: "test".to_string(),
                input_tokens: Some(100),
                output_tokens: Some(50),
                latency_ms: 200 + i * 10,
                user_id: None,
                request_type: "embedding".to_string(),
                status: if i == 4 { "error" } else { "200" }.to_string(),
                error_message: None,
            };
            logger.log(&entry).await.unwrap();
        }

        let summary = logger.get_summary(None).await.unwrap();
        assert_eq!(summary.total_calls, 5);
        assert_eq!(summary.total_input_tokens, 500);
        assert_eq!(summary.total_output_tokens, 250);
        assert_eq!(summary.error_count, 1);
        assert_eq!(summary.by_provider.len(), 2);
    }

    /// ADR-035 §2.7's decode class, pinned: `CAST(AVG(latency_ms) AS double
    /// precision)` produces a float on SQLite (affinity from the `DOUB` in the
    /// type name), the same value PostgreSQL's `float8` cast yields. Seeded
    /// latencies average to an exact value so a truncating decode would fail here.
    #[tokio::test]
    async fn test_summary_avg_latency_decodes_as_float() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        for latency in [100, 150, 300, 350] {
            logger.log(&sample_entry("openai", latency)).await.unwrap();
        }
        for latency in [10, 21] {
            logger
                .log(&sample_entry("anthropic", latency))
                .await
                .unwrap();
        }

        let summary = logger.get_summary(None).await.unwrap();
        // (100+150+300+350+10+21)/6 = 155.1666… — a value no integer decode can
        // round-trip.
        assert!((summary.avg_latency_ms - 155.166_666_666_666_66).abs() < 1e-9);

        let openai = summary
            .by_provider
            .iter()
            .find(|p| p.provider == "openai")
            .expect("openai stats");
        assert!((openai.avg_latency_ms - 225.0).abs() < 1e-9);

        let anthropic = summary
            .by_provider
            .iter()
            .find(|p| p.provider == "anthropic")
            .expect("anthropic stats");
        // 31/2 = 15.5: the halves that integer division would swallow.
        assert!((anthropic.avg_latency_ms - 15.5).abs() < 1e-9);
    }

    /// The other half of ADR-035 §2.7, pinned without a live PostgreSQL: one
    /// query definition emits the portable `CAST(... AS double precision)` for
    /// both backends — never the PostgreSQL-only `::float8` the pre-port arm
    /// used — while the placeholder style still differs, which is exactly the
    /// division of labour ADR-036 hands to the library.
    #[test]
    fn test_summary_sql_casts_avg_on_both_backends() {
        use sea_orm::{DbBackend, QueryTrait};

        let since = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc)
            .naive_utc();

        for backend in [DbBackend::Sqlite, DbBackend::Postgres] {
            for sql in [
                totals_query(since).build(backend).sql,
                by_provider_query(since).build(backend).sql,
            ] {
                assert!(sql.contains("CAST(AVG("), "{backend:?}: {sql}");
                assert!(sql.contains("AS double precision)"), "{backend:?}: {sql}");
                assert!(!sql.contains("::float8"), "{backend:?}: {sql}");
            }
        }

        // The cutoff parameter is spelled per backend by the library, not by
        // this call site — the whole point of the port.
        assert!(totals_query(since)
            .build(DbBackend::Postgres)
            .sql
            .contains("$1"));
        assert!(!totals_query(since)
            .build(DbBackend::Sqlite)
            .sql
            .contains("$1"));
    }

    /// The grouped summary must stay parameter-free in its SELECT list.
    ///
    /// A literal written as `Expr::val` inside an aggregate renders as a bind
    /// parameter, and PostgreSQL treats two occurrences of the same literal as
    /// two *different* parameters — enough for it to reject a grouped query
    /// whose SELECT and GROUP BY are supposed to match (SQLite accepts it, so
    /// the break only shows on PostgreSQL). Here the literals are inlined text
    /// via `Expr::cust` and the grouping is a bare column, so the only parameter
    /// in the statement is the `since` cutoff in the WHERE clause. Asserting on
    /// `Statement.sql` rather than `to_string()` is what makes this visible:
    /// `Display` injects the values, hiding the placeholders entirely.
    #[test]
    fn test_grouped_summary_binds_only_the_cutoff() {
        use sea_orm::{DbBackend, QueryTrait};

        let since = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc)
            .naive_utc();

        let stmt = by_provider_query(since).build(DbBackend::Postgres);
        let sql = &stmt.sql;

        assert!(sql.contains("$1"), "{sql}");
        assert!(
            !sql.contains("$2"),
            "a second bind means a literal leaked into the SELECT list: {sql}"
        );
        assert_eq!(stmt.values.as_ref().map_or(0, |v| v.0.len()), 1, "{sql}");

        // The grouping key is the column itself, so there is no SELECT-list
        // expression that has to be repeated (and matched) in GROUP BY.
        assert!(
            sql.contains(r#"GROUP BY "cloud_api_audit_log"."provider""#),
            "{sql}"
        );
    }

    /// With no rows in range the aggregate is NULL, which reads as the zero
    /// summary rather than erroring the decode.
    #[tokio::test]
    async fn test_summary_empty_table() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        let summary = logger.get_summary(None).await.unwrap();
        assert_eq!(summary.total_calls, 0);
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.avg_latency_ms, 0.0);
        assert!(summary.by_provider.is_empty());
    }

    /// The `since` cutoff binds as a naive UTC instant against the no-zone
    /// column, so rows outside the window are excluded on both backends.
    #[tokio::test]
    async fn test_summary_since_cutoff() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        let mut old = sample_entry("openai", 100);
        old.timestamp = Utc::now() - chrono::Duration::days(60);
        logger.log(&old).await.unwrap();
        logger.log(&sample_entry("openai", 200)).await.unwrap();

        // Default window is 30 days: the 60-day-old row falls outside it.
        let recent = logger.get_summary(None).await.unwrap();
        assert_eq!(recent.total_calls, 1);
        assert!((recent.avg_latency_ms - 200.0).abs() < 1e-9);

        let all = logger
            .get_summary(Some(Utc::now() - chrono::Duration::days(365)))
            .await
            .unwrap();
        assert_eq!(all.total_calls, 2);
    }

    /// pl-timestamp-write-tz: the value stored in the no-zone column is exactly
    /// `.naive_utc()` of the written instant — no session-TimeZone shift — and it
    /// reads back as the same `DateTime<Utc>`.
    #[tokio::test]
    async fn test_timestamp_stored_as_naive_utc() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db.clone());

        let written = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut entry = sample_entry("openai", 100);
        entry.timestamp = written;
        logger.log(&entry).await.unwrap();

        let stored = cloud_api_audit_log::Entity::find()
            .one(&db.sea_orm())
            .await
            .unwrap()
            .expect("row");
        assert_eq!(stored.timestamp, written.naive_utc());

        let (entries, _) = logger.get_log(1, 10, None).await.unwrap();
        assert_eq!(entries[0].timestamp, written);
    }

    #[test]
    fn test_audit_timer_ok() {
        let timer = AuditTimer::start("openai", "gpt-4o-mini", "completion", None);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let entry = timer.finish_ok(Some(100), Some(50));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.status, "200");
        assert!(entry.latency_ms >= 10);
        assert!(entry.error_message.is_none());
    }

    #[test]
    fn test_audit_timer_error() {
        let timer = AuditTimer::start("anthropic", "claude", "classification", Some("u1".into()));
        let entry = timer.finish_error("Rate limited");
        assert_eq!(entry.provider, "anthropic");
        assert_eq!(entry.status, "error");
        assert_eq!(entry.user_id.as_deref(), Some("u1"));
        assert_eq!(entry.error_message.as_deref(), Some("Rate limited"));
    }

    #[tokio::test]
    async fn test_pagination() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);

        for _ in 0..7 {
            logger.log(&sample_entry("openai", 100)).await.unwrap();
        }

        let (p1, total) = logger.get_log(1, 3, None).await.unwrap();
        assert_eq!(total, 7);
        assert_eq!(p1.len(), 3);

        let (p3, _) = logger.get_log(3, 3, None).await.unwrap();
        assert_eq!(p3.len(), 1);
    }
}
