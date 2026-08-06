//! Insight engine for subscription and recurring sender detection (S2-06).
//!
//! Analyses the email corpus stored in SQLite to detect subscription patterns,
//! recurring senders, and produce an inbox health report.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Statement,
};
use serde::Serialize;

use crate::db::entities::{connected_accounts, emails};
use crate::db::Database;

use super::error::VectorError;
use super::store::VectorStoreBackend;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Detected recurrence pattern for a subscription.
///
/// Serializes as a simple lowercase string for the frontend
/// (e.g., "daily", "weekly", "irregular").
#[derive(Debug, Clone, PartialEq)]
pub enum RecurrencePattern {
    Daily,
    Weekly,
    BiWeekly,
    Monthly,
    Quarterly,
    Irregular { avg_interval_days: f32 },
}

impl serde::Serialize for RecurrencePattern {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::BiWeekly => "biweekly",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Irregular { .. } => "irregular",
        })
    }
}

impl std::fmt::Display for RecurrencePattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecurrencePattern::Daily => write!(f, "daily"),
            RecurrencePattern::Weekly => write!(f, "weekly"),
            RecurrencePattern::BiWeekly => write!(f, "biweekly"),
            RecurrencePattern::Monthly => write!(f, "monthly"),
            RecurrencePattern::Quarterly => write!(f, "quarterly"),
            RecurrencePattern::Irregular { avg_interval_days } => {
                write!(f, "irregular ({avg_interval_days:.1} day avg)")
            }
        }
    }
}

/// Category of a detected subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionCategory {
    Newsletter,
    Marketing,
    Notification,
    Receipt,
    Social,
    Unknown,
}

impl std::fmt::Display for SubscriptionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscriptionCategory::Newsletter => write!(f, "newsletter"),
            SubscriptionCategory::Marketing => write!(f, "marketing"),
            SubscriptionCategory::Notification => write!(f, "notification"),
            SubscriptionCategory::Receipt => write!(f, "receipt"),
            SubscriptionCategory::Social => write!(f, "social"),
            SubscriptionCategory::Unknown => write!(f, "unknown"),
        }
    }
}

/// Suggested action for a subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestedAction {
    Keep,
    Unsubscribe,
    Archive,
    Digest,
}

impl std::fmt::Display for SuggestedAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuggestedAction::Keep => write!(f, "keep"),
            SuggestedAction::Unsubscribe => write!(f, "unsubscribe"),
            SuggestedAction::Archive => write!(f, "archive"),
            SuggestedAction::Digest => write!(f, "digest"),
        }
    }
}

/// Insight about a detected subscription.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInsight {
    pub sender_address: String,
    pub sender_domain: String,
    pub frequency: RecurrencePattern,
    pub email_count: u64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub has_unsubscribe: bool,
    pub category: SubscriptionCategory,
    pub suggested_action: SuggestedAction,
    /// Per-sender read rate (0.0 to 1.0).
    pub read_rate: f64,
    /// RFC 2369 List-Unsubscribe header from the most recent email (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_unsubscribe: Option<String>,
    /// RFC 8058 List-Unsubscribe-Post header from the most recent email (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_unsubscribe_post: Option<String>,
}

/// Insight about a recurring sender.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringSenderInsight {
    pub sender: String,
    pub email_count: u64,
    pub avg_interval_days: f32,
    pub category: String,
}

/// A top sender entry for the inbox report.
#[derive(Debug, Clone, Serialize)]
pub struct TopSender {
    pub sender: String,
    pub count: u64,
}

/// Aggregated inbox report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxReport {
    pub total_emails: u64,
    pub category_breakdown: HashMap<String, u64>,
    pub top_senders: Vec<TopSender>,
    pub subscription_count: u64,
    pub estimated_reading_hours: f32,
    /// Overall read rate (0.0 to 1.0).
    pub read_rate: f64,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Analyzes the email corpus to produce actionable insights about
/// subscriptions, recurring senders, and inbox health.
///
/// Persistence is single-code-path SeaORM (ADR-036).
pub struct InsightEngine {
    conn: DatabaseConnection,
    #[allow(dead_code)]
    store: Arc<dyn VectorStoreBackend>,
}

/// Row for individual email timestamps per sender (used by interval analysis).
struct EmailTimestampRow {
    received_at: chrono::NaiveDateTime,
    body_text: Option<String>,
}

impl InsightEngine {
    /// Create a new insight engine.
    pub fn new(db: Arc<Database>, store: Arc<dyn VectorStoreBackend>) -> Self {
        Self {
            conn: db.sea_orm(),
            store,
        }
    }

    /// Detect subscription patterns by grouping emails by sender.
    ///
    /// Excludes the user's own account email addresses (which appear as senders
    /// for sent mail) to avoid false-positive subscription detections.
    pub async fn detect_subscriptions(&self) -> Result<Vec<SubscriptionInsight>, VectorError> {
        // Collect all connected account email addresses to exclude from results.
        // Best-effort like the pre-port query: a missing table yields an empty set.
        let own_addresses: Vec<(String,)> = connected_accounts::Entity::find()
            .select_only()
            .column(connected_accounts::Column::EmailAddress)
            .distinct()
            .filter(connected_accounts::Column::Status.eq("connected"))
            .into_tuple()
            .all(&self.conn)
            .await
            .unwrap_or_default();

        let own_set: std::collections::HashSet<String> = own_addresses
            .into_iter()
            .map(|(addr,)| addr.to_lowercase())
            .collect();

        // (from_addr, count, first_received, last_received). `COUNT(*)` is
        // repeated in HAVING/ORDER BY — PostgreSQL rejects the select alias
        // there (ADR-035 catalog).
        let groups: Vec<(String, i64, chrono::NaiveDateTime, chrono::NaiveDateTime)> =
            emails::Entity::find()
                .select_only()
                .column(emails::Column::FromAddr)
                .column_as(Expr::cust("COUNT(*)"), "cnt")
                .column_as(emails::Column::ReceivedAt.min(), "first_seen")
                .column_as(emails::Column::ReceivedAt.max(), "last_seen")
                .group_by(emails::Column::FromAddr)
                .having(Expr::cust("COUNT(*)").gte(3))
                .order_by_desc(Expr::cust("COUNT(*)"))
                .into_tuple()
                .all(&self.conn)
                .await
                .map_err(VectorError::Db)?;

        let mut insights = Vec::new();

        for (from_addr, cnt, first_seen_ts, last_seen_ts) in &groups {
            let sender = from_addr;

            // Skip the user's own email addresses (sent mail false positives).
            let sender_email = extract_email_address(sender).to_lowercase();
            if own_set.contains(&sender_email) {
                continue;
            }

            let domain = extract_domain(sender);

            // Fetch individual timestamps for interval analysis
            let timestamp_rows: Vec<(chrono::NaiveDateTime, Option<String>)> =
                emails::Entity::find()
                    .select_only()
                    .column(emails::Column::ReceivedAt)
                    .column(emails::Column::BodyText)
                    .filter(emails::Column::FromAddr.eq(sender.as_str()))
                    .order_by_asc(emails::Column::ReceivedAt)
                    .into_tuple()
                    .all(&self.conn)
                    .await
                    .map_err(VectorError::Db)?;

            let timestamps: Vec<EmailTimestampRow> = timestamp_rows
                .into_iter()
                .map(|(received_at, body_text)| EmailTimestampRow {
                    received_at,
                    body_text,
                })
                .collect();

            // Check for List-Unsubscribe header in DB first (most reliable),
            // then fall back to body text keyword matching for older emails
            // that were ingested before header capture was added.
            let header_row: Option<(Option<String>, Option<String>)> = emails::Entity::find()
                .select_only()
                .column(emails::Column::ListUnsubscribe)
                .column(emails::Column::ListUnsubscribePost)
                .filter(emails::Column::FromAddr.eq(sender.as_str()))
                .filter(emails::Column::ListUnsubscribe.is_not_null())
                .order_by_desc(emails::Column::ReceivedAt)
                .into_tuple()
                .one(&self.conn)
                .await
                .map_err(VectorError::Db)?;

            let (list_unsub_header, list_unsub_post) = header_row.unwrap_or((None, None));

            let has_unsubscribe = list_unsub_header.is_some()
                || timestamps.iter().any(|row| {
                    row.body_text.as_ref().is_some_and(|body: &String| {
                        let lower = body.to_lowercase();
                        lower.contains("unsubscribe")
                            || lower.contains("opt out")
                            || lower.contains("opt-out")
                    })
                });

            // Compute inter-arrival intervals in days
            let intervals = compute_intervals(&timestamps);
            let frequency = classify_frequency(&intervals);
            let category = classify_subscription_category(&domain, has_unsubscribe);

            let first_seen = first_seen_ts.and_utc();
            let last_seen = last_seen_ts.and_utc();

            let email_count = *cnt as u64;
            let suggested_action =
                suggest_action(&frequency, &category, email_count, has_unsubscribe);

            // Per-sender read rate — one portable aggregate expression
            // (`CASE WHEN` over BOOLEAN, FLOAT cast) valid on both dialects.
            let read_rate_row: Option<(f64,)> = emails::Entity::find()
                .select_only()
                .expr_as(
                    Expr::cust(
                        "COALESCE(CAST(COUNT(CASE WHEN is_read THEN 1 END) AS FLOAT) \
                         / NULLIF(COUNT(*), 0), 0.0)",
                    ),
                    "read_rate",
                )
                .filter(emails::Column::FromAddr.eq(sender.as_str()))
                .into_tuple()
                .one(&self.conn)
                .await
                .map_err(VectorError::Db)?;

            insights.push(SubscriptionInsight {
                sender_address: sender.clone(),
                sender_domain: domain,
                frequency,
                email_count,
                first_seen,
                last_seen,
                has_unsubscribe,
                category,
                suggested_action,
                read_rate: read_rate_row.map(|(r,)| r).unwrap_or(0.0),
                list_unsubscribe: list_unsub_header,
                list_unsubscribe_post: list_unsub_post,
            });
        }

        Ok(insights)
    }

    /// Analyze recurring senders and their communication patterns.
    pub async fn analyze_recurring_senders(
        &self,
    ) -> Result<Vec<RecurringSenderInsight>, VectorError> {
        // `COUNT(*)` repeated in HAVING/ORDER BY — PostgreSQL rejects the
        // select alias there (ADR-035 catalog).
        let groups: Vec<(String, i64, chrono::NaiveDateTime, chrono::NaiveDateTime)> =
            emails::Entity::find()
                .select_only()
                .column(emails::Column::FromAddr)
                .column_as(Expr::cust("COUNT(*)"), "cnt")
                .column_as(emails::Column::ReceivedAt.min(), "first_seen")
                .column_as(emails::Column::ReceivedAt.max(), "last_seen")
                .group_by(emails::Column::FromAddr)
                .having(Expr::cust("COUNT(*)").gte(2))
                .order_by_desc(Expr::cust("COUNT(*)"))
                .into_tuple()
                .all(&self.conn)
                .await
                .map_err(VectorError::Db)?;

        let mut results = Vec::new();

        for (from_addr, cnt, first_seen_ts, last_seen_ts) in &groups {
            let first = first_seen_ts.and_utc();
            let last = last_seen_ts.and_utc();
            let span_days = (last - first).num_seconds() as f32 / 86400.0;
            let avg_interval = if *cnt > 1 {
                span_days / (*cnt as f32 - 1.0)
            } else {
                0.0
            };

            // Get the most common category for this sender
            let category_row: Option<(Option<String>,)> = emails::Entity::find()
                .select_only()
                .column(emails::Column::Category)
                .filter(emails::Column::FromAddr.eq(from_addr.as_str()))
                .group_by(emails::Column::Category)
                .order_by_desc(Expr::cust("COUNT(*)"))
                .into_tuple()
                .one(&self.conn)
                .await
                .map_err(VectorError::Db)?;

            let category = category_row
                .and_then(|r| r.0)
                .unwrap_or_else(|| "Uncategorized".to_string());

            results.push(RecurringSenderInsight {
                sender: from_addr.clone(),
                email_count: *cnt as u64,
                avg_interval_days: avg_interval,
                category,
            });
        }

        Ok(results)
    }

    /// Generate an aggregated inbox report.
    pub async fn generate_report(&self) -> Result<InboxReport, VectorError> {
        // Total emails
        let total_row: Option<(i64,)> = emails::Entity::find()
            .select_only()
            .expr_as(Expr::cust("COUNT(*)"), "cnt")
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;
        let total_emails = total_row.map(|(c,)| c).unwrap_or(0) as u64;

        // Category breakdown. `COUNT(*)` repeated in ORDER BY for portability
        // with the HAVING-alias fix elsewhere.
        let categories: Vec<(Option<String>, i64)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Category)
            .column_as(Expr::cust("COUNT(*)"), "cnt")
            .group_by(emails::Column::Category)
            .order_by_desc(Expr::cust("COUNT(*)"))
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let category_breakdown: HashMap<String, u64> = categories
            .into_iter()
            .map(|(cat, cnt)| {
                (
                    cat.unwrap_or_else(|| "Uncategorized".to_string()),
                    cnt as u64,
                )
            })
            .collect();

        // Top senders
        let senders: Vec<(String, i64)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::FromAddr)
            .column_as(Expr::cust("COUNT(*)"), "cnt")
            .group_by(emails::Column::FromAddr)
            .order_by_desc(Expr::cust("COUNT(*)"))
            .limit(10)
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let top_senders: Vec<TopSender> = senders
            .into_iter()
            .map(|(addr, cnt)| TopSender {
                sender: addr,
                count: cnt as u64,
            })
            .collect();

        // Subscription count (senders with 3+ emails).
        // Raw-SQL escape hatch (ADR-036 §5): a COUNT over a derived table has
        // no query-builder form here; the `AS sub` alias is required by
        // PostgreSQL (ADR-035 catalog) and accepted by SQLite.
        let sub_row = self
            .conn
            .query_one_raw(Statement::from_sql_and_values(
                self.conn.get_database_backend(),
                r#"SELECT COUNT(*) AS cnt FROM (
                   SELECT from_addr FROM emails
                   GROUP BY from_addr
                   HAVING COUNT(*) >= 3
               ) AS sub"#,
                [],
            ))
            .await
            .map_err(VectorError::Db)?;
        let subscription_count = match sub_row {
            Some(row) => row.try_get::<i64>("", "cnt").map_err(VectorError::Db)? as u64,
            None => 0,
        };

        // Estimated reading hours: ~30 seconds per email (conservative)
        let estimated_reading_hours = total_emails as f32 * 30.0 / 3600.0;

        // Overall read rate
        let read_rate_row: Option<(f64,)> = emails::Entity::find()
            .select_only()
            .expr_as(
                Expr::cust(
                    "COALESCE(CAST(COUNT(CASE WHEN is_read THEN 1 END) AS FLOAT) \
                     / NULLIF(COUNT(*), 0), 0.0)",
                ),
                "read_rate",
            )
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;
        let read_rate = read_rate_row.map(|(r,)| r).unwrap_or(0.0);

        Ok(InboxReport {
            total_emails,
            category_breakdown,
            top_senders,
            subscription_count,
            estimated_reading_hours,
            read_rate,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract domain from an email address.
fn extract_domain(email: &str) -> String {
    email
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_lowercase())
        .unwrap_or_else(|| email.to_lowercase())
}

/// Extract the bare email address from a "Name <addr>" or plain "addr" string.
fn extract_email_address(from: &str) -> String {
    if let Some(start) = from.find('<') {
        if let Some(end) = from[start..].find('>') {
            return from[start + 1..start + end].trim().to_string();
        }
    }
    from.trim().to_string()
}

/// Compute inter-arrival intervals in days from a list of timestamp rows.
///
/// Timestamps decode leniently on the SQLite path (RFC3339 first, then the
/// naive `%Y-%m-%d %H:%M:%S` shapes) — the same order the pre-port string
/// parser used — so legacy rows keep their values.
fn compute_intervals(timestamps: &[EmailTimestampRow]) -> Vec<f32> {
    if timestamps.len() < 2 {
        return vec![];
    }

    let dates: Vec<DateTime<Utc>> = timestamps.iter().map(|r| r.received_at.and_utc()).collect();

    dates
        .windows(2)
        .map(|w| (w[1] - w[0]).num_seconds() as f32 / 86400.0)
        .collect()
}

/// Classify frequency from inter-arrival intervals.
fn classify_frequency(intervals: &[f32]) -> RecurrencePattern {
    if intervals.is_empty() {
        return RecurrencePattern::Irregular {
            avg_interval_days: 0.0,
        };
    }

    let mean = intervals.iter().sum::<f32>() / intervals.len() as f32;

    if mean <= 0.0 {
        return RecurrencePattern::Irregular {
            avg_interval_days: 0.0,
        };
    }

    let variance =
        intervals.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / intervals.len() as f32;
    let stddev = variance.sqrt();

    // Check regularity: stddev < 30% of mean
    let is_regular = stddev < 0.3 * mean;

    if !is_regular {
        return RecurrencePattern::Irregular {
            avg_interval_days: mean,
        };
    }

    // Classify by mean interval
    if mean < 1.5 {
        RecurrencePattern::Daily
    } else if mean < 10.0 {
        RecurrencePattern::Weekly
    } else if mean < 20.0 {
        RecurrencePattern::BiWeekly
    } else if mean < 45.0 {
        RecurrencePattern::Monthly
    } else {
        RecurrencePattern::Quarterly
    }
}

/// Classify subscription category.
///
/// Returns `Unknown` unless the embedding pipeline has categorized the
/// sender's emails. Domain-based heuristics were removed because they
/// produced unreliable results.
fn classify_subscription_category(_domain: &str, _has_unsubscribe: bool) -> SubscriptionCategory {
    // Category is determined by the embedding pipeline, not heuristics.
    // The frontend hides the category column when all values are "unknown".
    SubscriptionCategory::Unknown
}

/// Suggest an action based on subscription characteristics.
fn suggest_action(
    frequency: &RecurrencePattern,
    category: &SubscriptionCategory,
    email_count: u64,
    has_unsubscribe: bool,
) -> SuggestedAction {
    match category {
        SubscriptionCategory::Marketing => {
            if has_unsubscribe {
                SuggestedAction::Unsubscribe
            } else {
                SuggestedAction::Archive
            }
        }
        SubscriptionCategory::Newsletter => match frequency {
            RecurrencePattern::Daily if email_count > 30 => SuggestedAction::Digest,
            _ => SuggestedAction::Keep,
        },
        SubscriptionCategory::Notification => SuggestedAction::Archive,
        SubscriptionCategory::Social => SuggestedAction::Archive,
        SubscriptionCategory::Receipt => SuggestedAction::Keep,
        SubscriptionCategory::Unknown => {
            if email_count > 20 && has_unsubscribe {
                SuggestedAction::Unsubscribe
            } else {
                SuggestedAction::Keep
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors::store::InMemoryVectorStore;

    async fn test_db() -> Database {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
        ] {
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
                    conn.execute_unprepared(s).await.unwrap();
                }
            }
        }
        db
    }

    fn make_engine(db: Arc<Database>) -> InsightEngine {
        let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
        InsightEngine::new(db, store)
    }

    async fn insert_emails_from_sender(
        db: &Database,
        sender: &str,
        count: usize,
        interval_days: f64,
        body_text: &str,
    ) {
        let base = chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        for i in 0..count {
            let id = format!("{}-{}", sender.replace('@', "-"), i);
            let received_at =
                base + chrono::Duration::seconds((interval_days * i as f64 * 86400.0) as i64);
            let received_str = received_at.format("%Y-%m-%d %H:%M:%S").to_string();

            db.sea_orm()
                .execute_raw(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    r#"INSERT INTO emails (id, account_id, provider, subject, from_addr, body_text, received_at)
                   VALUES (?, 'acct-1', 'test', ?, ?, ?, ?)"#,
                    [
                        id.as_str().into(),
                        format!("Email from {}", sender).into(),
                        sender.into(),
                        body_text.into(),
                        received_str.as_str().into(),
                    ],
                ))
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn test_detect_subscriptions_groups_by_sender() {
        let db = Arc::new(test_db().await);
        insert_emails_from_sender(&db, "news@example.com", 5, 7.0, "Click to unsubscribe").await;
        insert_emails_from_sender(&db, "rare@example.com", 1, 30.0, "Hello").await;

        let engine = make_engine(db);
        let subs = engine.detect_subscriptions().await.unwrap();

        // Only the sender with 3+ emails should appear
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].sender_address, "news@example.com");
        assert_eq!(subs[0].email_count, 5);
        assert!(subs[0].has_unsubscribe);
    }

    #[tokio::test]
    async fn test_frequency_detection_daily() {
        let intervals = vec![1.0, 1.0, 1.0, 1.0, 1.0];
        let freq = classify_frequency(&intervals);
        assert_eq!(freq, RecurrencePattern::Daily);
    }

    #[tokio::test]
    async fn test_frequency_detection_weekly() {
        let intervals = vec![7.0, 7.0, 7.0, 7.0];
        let freq = classify_frequency(&intervals);
        assert_eq!(freq, RecurrencePattern::Weekly);
    }

    #[tokio::test]
    async fn test_frequency_detection_monthly() {
        let intervals = vec![30.0, 31.0, 30.0, 31.0];
        let freq = classify_frequency(&intervals);
        assert_eq!(freq, RecurrencePattern::Monthly);
    }

    #[tokio::test]
    async fn test_frequency_detection_irregular() {
        let intervals = vec![1.0, 30.0, 2.0, 60.0];
        let freq = classify_frequency(&intervals);
        match freq {
            RecurrencePattern::Irregular { avg_interval_days } => {
                assert!(avg_interval_days > 0.0);
            }
            other => panic!("Expected Irregular, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_inbox_report_aggregation() {
        let db = Arc::new(test_db().await);
        insert_emails_from_sender(&db, "alice@example.com", 5, 7.0, "Hello").await;
        insert_emails_from_sender(&db, "bob@example.com", 3, 14.0, "World").await;
        insert_emails_from_sender(&db, "carol@example.com", 2, 30.0, "Test").await;

        let engine = make_engine(db);
        let report = engine.generate_report().await.unwrap();

        assert_eq!(report.total_emails, 10);
        assert!(report.top_senders.len() <= 10);
        // alice has 5 emails, should be first
        assert_eq!(report.top_senders[0].sender, "alice@example.com");
        assert_eq!(report.top_senders[0].count, 5);
        // Subscription count: senders with 3+ emails = alice (5) + bob (3) = 2
        assert_eq!(report.subscription_count, 2);
        assert!(report.estimated_reading_hours > 0.0);
    }

    #[tokio::test]
    async fn test_subscription_category_always_unknown_without_embeddings() {
        // Category classification is deferred to the embedding pipeline.
        // Without embeddings, all categories should be Unknown.
        assert_eq!(
            classify_subscription_category("facebook.com", false),
            SubscriptionCategory::Unknown
        );
        assert_eq!(
            classify_subscription_category("newsletter.example.com", true),
            SubscriptionCategory::Unknown
        );
        assert_eq!(
            classify_subscription_category("example.com", false),
            SubscriptionCategory::Unknown
        );
    }

    #[tokio::test]
    async fn test_analyze_recurring_senders() {
        let db = Arc::new(test_db().await);
        insert_emails_from_sender(&db, "daily@example.com", 10, 1.0, "Daily").await;
        insert_emails_from_sender(&db, "once@example.com", 1, 0.0, "Once").await;

        let engine = make_engine(db);
        let senders = engine.analyze_recurring_senders().await.unwrap();

        // Only senders with 2+ emails
        assert_eq!(senders.len(), 1);
        assert_eq!(senders[0].sender, "daily@example.com");
        assert_eq!(senders[0].email_count, 10);
        assert!(senders[0].avg_interval_days > 0.0);
    }
}
