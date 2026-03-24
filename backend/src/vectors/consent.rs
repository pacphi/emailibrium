//! Consent management for cloud AI providers (ADR-012).
//!
//! Tracks user consent for sending email data to external AI services
//! and maintains an audit log of all cloud API calls.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::db::Database;
use super::error::VectorError;

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

        sqlx::query(
            "INSERT INTO ai_consent (provider, consented_at, acknowledgment) \
             VALUES (?, ?, ?) \
             ON CONFLICT(provider) DO UPDATE SET \
                consented_at = excluded.consented_at, \
                revoked_at = NULL, \
                acknowledgment = excluded.acknowledgment",
        )
        .bind(provider)
        .bind(Utc::now())
        .bind(acknowledgment)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Revoke consent for a specific cloud AI provider.
    pub async fn revoke_consent(&self, provider: &str) -> Result<(), VectorError> {
        info!(provider = provider, "Revoking AI consent");

        let result = sqlx::query(
            "UPDATE ai_consent SET revoked_at = ? WHERE provider = ? AND revoked_at IS NULL",
        )
        .bind(Utc::now())
        .bind(provider)
        .execute(&self.db.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(VectorError::ConfigError(format!(
                "No active consent found for provider '{provider}'"
            )));
        }

        Ok(())
    }

    /// Check whether active (non-revoked) consent exists for a provider.
    pub async fn has_consent(&self, provider: &str) -> Result<bool, VectorError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM ai_consent WHERE provider = ? AND revoked_at IS NULL",
        )
        .bind(provider)
        .fetch_one(&self.db.pool)
        .await?;

        Ok(row.0 > 0)
    }

    /// Get all consent records.
    pub async fn get_all_consent(&self) -> Result<Vec<ConsentRecord>, VectorError> {
        let rows = sqlx::query_as::<_, (String, DateTime<Utc>, Option<DateTime<Utc>>, String)>(
            "SELECT provider, consented_at, revoked_at, acknowledgment FROM ai_consent ORDER BY consented_at DESC",
        )
        .fetch_all(&self.db.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(provider, consented_at, revoked_at, acknowledgment)| ConsentRecord {
                provider,
                consented_at,
                revoked_at,
                acknowledgment,
            })
            .collect())
    }

    /// Log a cloud API call to the audit table.
    pub async fn log_cloud_call(&self, entry: &AuditEntry) -> Result<(), VectorError> {
        debug!(
            provider = %entry.provider,
            model = %entry.model,
            endpoint = %entry.endpoint,
            "Logging cloud AI call"
        );

        sqlx::query(
            "INSERT INTO ai_audit_log \
             (timestamp, provider, model, endpoint, input_token_count, output_token_count, input_hash, latency_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.timestamp)
        .bind(&entry.provider)
        .bind(&entry.model)
        .bind(&entry.endpoint)
        .bind(entry.input_token_count)
        .bind(entry.output_token_count)
        .bind(&entry.input_hash)
        .bind(entry.latency_ms)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Get a paginated view of the audit log (newest first).
    pub async fn get_audit_log(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<AuditPage, VectorError> {
        let offset = (page.saturating_sub(1)) as i64 * per_page as i64;
        let limit = per_page as i64;

        let (total,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM ai_audit_log")
                .fetch_one(&self.db.pool)
                .await?;

        let rows = sqlx::query_as::<_, (i64, DateTime<Utc>, String, String, String, Option<i64>, Option<i64>, Option<String>, Option<i64>)>(
            "SELECT id, timestamp, provider, model, endpoint, \
                    input_token_count, output_token_count, input_hash, latency_ms \
             FROM ai_audit_log ORDER BY timestamp DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.db.pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(
                |(id, timestamp, provider, model, endpoint, input_tc, output_tc, hash, latency)| {
                    AuditEntry {
                        id: Some(id),
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
            .collect();

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
        .execute(&db.pool)
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
        .execute(&db.pool)
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
