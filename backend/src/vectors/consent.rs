//! Consent management for cloud AI providers (ADR-012).
//!
//! Tracks user consent for sending email data to external AI services
//! and maintains an audit log of all cloud API calls.

use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::error::VectorError;
use crate::db::{audited_sql, Database};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A record of user consent for a specific AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub provider: String,
    pub consented_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub acknowledgment: String,
}

/// An entry in the cloud AI audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub input_token_count: Option<i64>,
    pub output_token_count: Option<i64>,
    pub input_hash: Option<String>,
    pub latency_ms: Option<i64>,
}

/// Paginated audit log response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

/// ai_consent row, SQLite decode (consented_at/revoked_at as DateTime<Utc>).
type ConsentRowSqlite = (String, DateTime<Utc>, Option<DateTime<Utc>>, String);
/// ai_consent row, Postgres decode — TIMESTAMP (no tz) needs NaiveDateTime (ADR-035).
type ConsentRowPostgres = (String, NaiveDateTime, Option<NaiveDateTime>, String);

/// ai_audit_log row, SQLite decode.
type AuditRowSqlite = (
    i32,
    DateTime<Utc>,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);
/// ai_audit_log row, Postgres decode — id/token counts/latency are INTEGER/INT4 (i32),
/// timestamp is TIMESTAMP (no tz, decodes as NaiveDateTime) — ADR-035.
type AuditRowPostgres = (
    i32,
    NaiveDateTime,
    String,
    String,
    String,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<i32>,
);

// ---------------------------------------------------------------------------
// ConsentManager
// ---------------------------------------------------------------------------

/// Manages user consent for cloud AI providers and maintains audit logs.
pub struct ConsentManager {
    db: Arc<Database>,
}

impl ConsentManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Grant consent for a specific cloud AI provider.
    pub async fn grant_consent(
        &self,
        provider: &str,
        acknowledgment: &str,
    ) -> Result<(), VectorError> {
        info!(provider = provider, "Granting AI consent");

        // ai_consent.consented_at is TIMESTAMP (no tz) in both dialects. Binding a
        // DateTime<Utc> into it works fine on both backends (Postgres accepts the
        // timestamptz->timestamp assignment cast) — it's only the DECODE direction
        // that needs NaiveDateTime, per get_all_consent() below (ADR-035).
        let sql = self.db.adapt(
            "INSERT INTO ai_consent (provider, consented_at, acknowledgment) \
             VALUES (?, ?, ?) \
             ON CONFLICT(provider) DO UPDATE SET \
                consented_at = excluded.consented_at, \
                revoked_at = NULL, \
                acknowledgment = excluded.acknowledgment",
        );
        match self.db.as_ref() {
            Database::Sqlite(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(provider)
                    .bind(Utc::now())
                    .bind(acknowledgment)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(provider)
                    .bind(Utc::now())
                    .bind(acknowledgment)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Revoke consent for a specific cloud AI provider.
    pub async fn revoke_consent(&self, provider: &str) -> Result<(), VectorError> {
        info!(provider = provider, "Revoking AI consent");

        let sql = self.db.adapt(
            "UPDATE ai_consent SET revoked_at = ? WHERE provider = ? AND revoked_at IS NULL",
        );
        let affected = match self.db.as_ref() {
            Database::Sqlite(pool) => sqlx::query(audited_sql(&sql))
                .bind(Utc::now())
                .bind(provider)
                .execute(pool)
                .await?
                .rows_affected(),
            Database::Postgres(pool) => sqlx::query(audited_sql(&sql))
                .bind(Utc::now())
                .bind(provider)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        if affected == 0 {
            return Err(VectorError::ConfigError(format!(
                "No active consent found for provider '{provider}'"
            )));
        }

        Ok(())
    }

    /// Check whether active (non-revoked) consent exists for a provider.
    pub async fn has_consent(&self, provider: &str) -> Result<bool, VectorError> {
        let sql = self
            .db
            .adapt("SELECT COUNT(*) FROM ai_consent WHERE provider = ? AND revoked_at IS NULL");
        let row: (i64,) = match self.db.as_ref() {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(provider)
                    .fetch_one(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(provider)
                    .fetch_one(pool)
                    .await?
            }
        };

        Ok(row.0 > 0)
    }

    /// Get all consent records.
    pub async fn get_all_consent(&self) -> Result<Vec<ConsentRecord>, VectorError> {
        let sql = "SELECT provider, consented_at, revoked_at, acknowledgment \
                   FROM ai_consent ORDER BY consented_at DESC";
        let records = match self.db.as_ref() {
            Database::Sqlite(pool) => {
                let rows: Vec<ConsentRowSqlite> = sqlx::query_as(sql).fetch_all(pool).await?;
                rows.into_iter()
                    .map(
                        |(provider, consented_at, revoked_at, acknowledgment)| ConsentRecord {
                            provider,
                            consented_at,
                            revoked_at,
                            acknowledgment,
                        },
                    )
                    .collect()
            }
            Database::Postgres(pool) => {
                // consented_at/revoked_at are TIMESTAMP (no tz) — decode as NaiveDateTime,
                // not DateTime<Utc>, then widen (ADR-035).
                let rows: Vec<ConsentRowPostgres> = sqlx::query_as(sql).fetch_all(pool).await?;
                rows.into_iter()
                    .map(
                        |(provider, consented_at, revoked_at, acknowledgment)| ConsentRecord {
                            provider,
                            consented_at: consented_at.and_utc(),
                            revoked_at: revoked_at.map(|dt| dt.and_utc()),
                            acknowledgment,
                        },
                    )
                    .collect()
            }
        };

        Ok(records)
    }

    /// Log a cloud API call to the audit table.
    pub async fn log_cloud_call(&self, entry: &AuditEntry) -> Result<(), VectorError> {
        debug!(
            provider = %entry.provider,
            model = %entry.model,
            endpoint = %entry.endpoint,
            "Logging cloud AI call"
        );

        let sql = self.db.adapt(
            "INSERT INTO ai_audit_log \
             (timestamp, provider, model, endpoint, input_token_count, output_token_count, input_hash, latency_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        );
        match self.db.as_ref() {
            Database::Sqlite(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(entry.timestamp)
                    .bind(&entry.provider)
                    .bind(&entry.model)
                    .bind(&entry.endpoint)
                    .bind(entry.input_token_count)
                    .bind(entry.output_token_count)
                    .bind(&entry.input_hash)
                    .bind(entry.latency_ms)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(entry.timestamp)
                    .bind(&entry.provider)
                    .bind(&entry.model)
                    .bind(&entry.endpoint)
                    .bind(entry.input_token_count)
                    .bind(entry.output_token_count)
                    .bind(&entry.input_hash)
                    .bind(entry.latency_ms)
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Get a paginated view of the audit log (newest first).
    pub async fn get_audit_log(&self, page: u32, per_page: u32) -> Result<AuditPage, VectorError> {
        let offset = (page.saturating_sub(1)) as i64 * per_page as i64;
        let limit = per_page as i64;

        let count_sql = "SELECT COUNT(*) FROM ai_audit_log";
        let (total,): (i64,) = match self.db.as_ref() {
            Database::Sqlite(pool) => sqlx::query_as(count_sql).fetch_one(pool).await?,
            Database::Postgres(pool) => sqlx::query_as(count_sql).fetch_one(pool).await?,
        };

        let sql = self.db.adapt(
            "SELECT id, timestamp, provider, model, endpoint, \
                    input_token_count, output_token_count, input_hash, latency_ms \
             FROM ai_audit_log ORDER BY timestamp DESC LIMIT ? OFFSET ?",
        );
        // id is INTEGER/INT4 in both dialects — i32, not i64 (ADR-035).
        let entries: Vec<AuditEntry> = match self.db.as_ref() {
            Database::Sqlite(pool) => {
                let rows: Vec<AuditRowSqlite> = sqlx::query_as(audited_sql(&sql))
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await?;
                rows.into_iter()
                    .map(
                        |(
                            id,
                            timestamp,
                            provider,
                            model,
                            endpoint,
                            input_tc,
                            output_tc,
                            hash,
                            latency,
                        )| {
                            AuditEntry {
                                id: Some(id as i64),
                                timestamp,
                                provider,
                                model,
                                endpoint,
                                input_token_count: input_tc,
                                output_token_count: output_tc,
                                input_hash: hash,
                                latency_ms: latency,
                            }
                        },
                    )
                    .collect()
            }
            Database::Postgres(pool) => {
                // input_token_count/output_token_count/latency_ms are also INTEGER/INT4.
                let rows: Vec<AuditRowPostgres> = sqlx::query_as(audited_sql(&sql))
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await?;
                rows.into_iter()
                    .map(
                        |(
                            id,
                            timestamp,
                            provider,
                            model,
                            endpoint,
                            input_tc,
                            output_tc,
                            hash,
                            latency,
                        )| {
                            AuditEntry {
                                id: Some(id as i64),
                                timestamp: timestamp.and_utc(),
                                provider,
                                model,
                                endpoint,
                                input_token_count: input_tc.map(|v| v as i64),
                                output_token_count: output_tc.map(|v| v as i64),
                                input_hash: hash,
                                latency_ms: latency.map(|v| v as i64),
                            }
                        },
                    )
                    .collect()
            }
        };

        Ok(AuditPage {
            entries,
            page,
            per_page,
            total,
        })
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

        // Create tables directly for tests (migrations won't auto-run on :memory:).
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ai_consent (
                provider TEXT PRIMARY KEY,
                consented_at TEXT NOT NULL,
                revoked_at TEXT,
                acknowledgment TEXT NOT NULL
            )",
        )
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ai_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                input_token_count INTEGER,
                output_token_count INTEGER,
                input_hash TEXT,
                latency_ms INTEGER
            )",
        )
        .execute(db.pool())
        .await
        .unwrap();

        Arc::new(db)
    }

    #[tokio::test]
    async fn test_consent_grant_and_check() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        assert!(!mgr.has_consent("openai").await.unwrap());

        mgr.grant_consent("openai", "I acknowledge data will be sent to OpenAI")
            .await
            .unwrap();

        assert!(mgr.has_consent("openai").await.unwrap());
    }

    #[tokio::test]
    async fn test_consent_revoke() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        mgr.grant_consent("anthropic", "I acknowledge data sharing")
            .await
            .unwrap();
        assert!(mgr.has_consent("anthropic").await.unwrap());

        mgr.revoke_consent("anthropic").await.unwrap();
        assert!(!mgr.has_consent("anthropic").await.unwrap());
    }

    #[tokio::test]
    async fn test_consent_revoke_nonexistent() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        let result = mgr.revoke_consent("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_consent_grant_idempotent() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        mgr.grant_consent("openai", "First acknowledgment")
            .await
            .unwrap();
        mgr.grant_consent("openai", "Updated acknowledgment")
            .await
            .unwrap();

        let all = mgr.get_all_consent().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].acknowledgment, "Updated acknowledgment");
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        let entry = AuditEntry {
            id: None,
            timestamp: Utc::now(),
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            endpoint: "/v1/chat/completions".to_string(),
            input_token_count: Some(100),
            output_token_count: Some(10),
            input_hash: Some("abc123".to_string()),
            latency_ms: Some(250),
        };

        mgr.log_cloud_call(&entry).await.unwrap();

        let page = mgr.get_audit_log(1, 10).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].provider, "openai");
        assert_eq!(page.entries[0].model, "gpt-4o-mini");
        assert_eq!(page.entries[0].input_token_count, Some(100));
    }

    #[tokio::test]
    async fn test_audit_log_pagination() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        for i in 0..5 {
            let entry = AuditEntry {
                id: None,
                timestamp: Utc::now(),
                provider: "openai".to_string(),
                model: format!("model-{i}"),
                endpoint: "/v1/chat/completions".to_string(),
                input_token_count: None,
                output_token_count: None,
                input_hash: None,
                latency_ms: None,
            };
            mgr.log_cloud_call(&entry).await.unwrap();
        }

        let page1 = mgr.get_audit_log(1, 2).await.unwrap();
        assert_eq!(page1.total, 5);
        assert_eq!(page1.entries.len(), 2);

        let page3 = mgr.get_audit_log(3, 2).await.unwrap();
        assert_eq!(page3.entries.len(), 1);
    }
}
