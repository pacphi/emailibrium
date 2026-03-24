//! Audit logging middleware for cloud API calls (ADR-008, ADR-012, item #39).
//!
//! Wraps cloud API call functions with timing and logging, storing every
//! call in the `cloud_api_audit_log` table with provider, model, tokens,
//! latency, status, and user context.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::error::VectorError;
use crate::db::Database;

/// Row tuple for per-provider audit statistics.
type ProviderStatsRow = (String, i64, Option<i64>, Option<i64>, Option<f64>, i64);

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

// ---------------------------------------------------------------------------
// AuditLogger
// ---------------------------------------------------------------------------

/// Audit logger for cloud API calls.
///
/// Stores detailed logs of every cloud API interaction for compliance,
/// debugging, and cost tracking.
pub struct CloudApiAuditLogger {
    db: Arc<Database>,
}

impl CloudApiAuditLogger {
    /// Create a new audit logger.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Ensure the audit log table exists.
    pub async fn ensure_table(&self) -> Result<(), VectorError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cloud_api_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                latency_ms INTEGER NOT NULL,
                user_id TEXT,
                request_type TEXT NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cloud_audit_timestamp ON cloud_api_audit_log(timestamp)",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cloud_audit_provider ON cloud_api_audit_log(provider)",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cloud_audit_user ON cloud_api_audit_log(user_id)",
        )
        .execute(&self.db.pool)
        .await?;

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

        sqlx::query(
            "INSERT INTO cloud_api_audit_log
             (timestamp, provider, model, input_tokens, output_tokens, latency_ms,
              user_id, request_type, status, error_message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.timestamp)
        .bind(&entry.provider)
        .bind(&entry.model)
        .bind(entry.input_tokens)
        .bind(entry.output_tokens)
        .bind(entry.latency_ms)
        .bind(&entry.user_id)
        .bind(&entry.request_type)
        .bind(&entry.status)
        .bind(&entry.error_message)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Get paginated audit log entries (newest first).
    pub async fn get_log(
        &self,
        page: u32,
        per_page: u32,
        provider_filter: Option<&str>,
    ) -> Result<(Vec<CloudApiAuditEntry>, i64), VectorError> {
        let offset = (page.saturating_sub(1)) as i64 * per_page as i64;
        let limit = per_page as i64;

        let total: (i64,) = if let Some(provider) = provider_filter {
            sqlx::query_as("SELECT COUNT(*) FROM cloud_api_audit_log WHERE provider = ?")
                .bind(provider)
                .fetch_one(&self.db.pool)
                .await?
        } else {
            sqlx::query_as("SELECT COUNT(*) FROM cloud_api_audit_log")
                .fetch_one(&self.db.pool)
                .await?
        };

        let rows = if let Some(provider) = provider_filter {
            sqlx::query_as::<
                _,
                (
                    i64,
                    DateTime<Utc>,
                    String,
                    String,
                    Option<i64>,
                    Option<i64>,
                    i64,
                    Option<String>,
                    String,
                    String,
                    Option<String>,
                ),
            >(
                "SELECT id, timestamp, provider, model, input_tokens, output_tokens,
                        latency_ms, user_id, request_type, status, error_message
                 FROM cloud_api_audit_log WHERE provider = ?
                 ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            )
            .bind(provider)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db.pool)
            .await?
        } else {
            sqlx::query_as::<
                _,
                (
                    i64,
                    DateTime<Utc>,
                    String,
                    String,
                    Option<i64>,
                    Option<i64>,
                    i64,
                    Option<String>,
                    String,
                    String,
                    Option<String>,
                ),
            >(
                "SELECT id, timestamp, provider, model, input_tokens, output_tokens,
                        latency_ms, user_id, request_type, status, error_message
                 FROM cloud_api_audit_log
                 ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db.pool)
            .await?
        };

        let entries = rows
            .into_iter()
            .map(
                |(
                    id,
                    timestamp,
                    provider,
                    model,
                    input_tokens,
                    output_tokens,
                    latency_ms,
                    user_id,
                    request_type,
                    status,
                    error_message,
                )| {
                    CloudApiAuditEntry {
                        id: Some(id),
                        timestamp,
                        provider,
                        model,
                        input_tokens,
                        output_tokens,
                        latency_ms,
                        user_id,
                        request_type,
                        status,
                        error_message,
                    }
                },
            )
            .collect();

        Ok((entries, total.0))
    }

    /// Get summary statistics, optionally filtered by time range.
    pub async fn get_summary(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<AuditSummary, VectorError> {
        let since_ts = since.unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));

        let totals: (i64, Option<i64>, Option<i64>, Option<f64>, i64) = sqlx::query_as(
            "SELECT
                COUNT(*),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                AVG(latency_ms),
                COALESCE(SUM(CASE WHEN status != '200' AND status != 'ok' THEN 1 ELSE 0 END), 0)
             FROM cloud_api_audit_log WHERE timestamp >= ?",
        )
        .bind(since_ts)
        .fetch_one(&self.db.pool)
        .await?;

        let provider_rows: Vec<ProviderStatsRow> = sqlx::query_as(
            "SELECT
                    provider,
                    COUNT(*),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    AVG(latency_ms),
                    COALESCE(SUM(CASE WHEN status != '200' AND status != 'ok' THEN 1 ELSE 0 END), 0)
                 FROM cloud_api_audit_log
                 WHERE timestamp >= ?
                 GROUP BY provider
                 ORDER BY COUNT(*) DESC",
        )
        .bind(since_ts)
        .fetch_all(&self.db.pool)
        .await?;

        let by_provider = provider_rows
            .into_iter()
            .map(
                |(provider, call_count, input_tokens, output_tokens, avg_latency, error_count)| {
                    ProviderStats {
                        provider,
                        call_count,
                        total_input_tokens: input_tokens.unwrap_or(0),
                        total_output_tokens: output_tokens.unwrap_or(0),
                        avg_latency_ms: avg_latency.unwrap_or(0.0),
                        error_count,
                    }
                },
            )
            .collect();

        Ok(AuditSummary {
            total_calls: totals.0,
            total_input_tokens: totals.1.unwrap_or(0),
            total_output_tokens: totals.2.unwrap_or(0),
            avg_latency_ms: totals.3.unwrap_or(0.0),
            error_count: totals.4,
            by_provider,
        })
    }
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

    async fn setup_db() -> Arc<Database> {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");
        Arc::new(db)
    }

    #[tokio::test]
    async fn test_ensure_table() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);
        logger.ensure_table().await.unwrap();
        // Second call should be idempotent.
        logger.ensure_table().await.unwrap();
    }

    #[tokio::test]
    async fn test_log_and_retrieve() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);
        logger.ensure_table().await.unwrap();

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
        logger.ensure_table().await.unwrap();

        let entry = CloudApiAuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
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
        logger.ensure_table().await.unwrap();

        for provider in &["openai", "openai", "anthropic"] {
            let entry = CloudApiAuditEntry {
                id: None,
                timestamp: Utc::now(),
                provider: provider.to_string(),
                model: "test".to_string(),
                input_tokens: None,
                output_tokens: None,
                latency_ms: 100,
                user_id: None,
                request_type: "test".to_string(),
                status: "200".to_string(),
                error_message: None,
            };
            logger.log(&entry).await.unwrap();
        }

        let (entries, total) = logger.get_log(1, 10, Some("openai")).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);

        let (entries, total) = logger.get_log(1, 10, Some("anthropic")).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_summary() {
        let db = setup_db().await;
        let logger = CloudApiAuditLogger::new(db);
        logger.ensure_table().await.unwrap();

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
        logger.ensure_table().await.unwrap();

        for _ in 0..7 {
            let entry = CloudApiAuditEntry {
                id: None,
                timestamp: Utc::now(),
                provider: "openai".to_string(),
                model: "test".to_string(),
                input_tokens: None,
                output_tokens: None,
                latency_ms: 100,
                user_id: None,
                request_type: "test".to_string(),
                status: "200".to_string(),
                error_message: None,
            };
            logger.log(&entry).await.unwrap();
        }

        let (p1, total) = logger.get_log(1, 3, None).await.unwrap();
        assert_eq!(total, 7);
        assert_eq!(p1.len(), 3);

        let (p3, _) = logger.get_log(3, 3, None).await.unwrap();
        assert_eq!(p3.len(), 1);
    }
}
