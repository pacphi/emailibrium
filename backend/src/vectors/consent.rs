//! Consent management for cloud AI providers (ADR-012).
//!
//! Tracks user consent for sending email data to external AI services
//! and maintains an audit log of all cloud API calls.
//!
//! Single-code-path SeaORM (ADR-036): `ai_consent.consented_at`/`revoked_at` and
//! `ai_audit_log.timestamp` are plain `TIMESTAMP` (no zone) in both dialects, so
//! the entities declare them `NaiveDateTime` and every write here binds
//! `.naive_utc()`. That closes the write-side half of `pl-timestamp-write-tz`:
//! the pre-port code bound `DateTime<Utc>`, which PostgreSQL assignment-casts
//! through the session `TimeZone` GUC, shifting the stored value by the session's
//! offset on any non-UTC connection. A naive bind cannot shift. The read side was
//! already correct but needed a per-backend decode arm (ADR-035 §2.6); the entity
//! owns that now, so the four hand-written row-tuple aliases are gone.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, Order, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::error::VectorError;
use crate::db::entities::{ai_audit_log, ai_consent};
use crate::db::Database;

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
    conn: DatabaseConnection,
}

impl ConsentManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Grant consent for a specific cloud AI provider.
    pub async fn grant_consent(
        &self,
        provider: &str,
        acknowledgment: &str,
    ) -> Result<(), VectorError> {
        info!(provider = provider, "Granting AI consent");

        // pl-timestamp-write-tz: `consented_at` is TIMESTAMP (no zone), so the
        // bind is the naive UTC instant. Binding a `DateTime<Utc>` here — what
        // this call site did before the port — lets PostgreSQL's
        // timestamptz->timestamp assignment cast rewrite the value through the
        // session's TimeZone.
        let now = Utc::now().naive_utc();

        // The re-grant path resets `revoked_at` to NULL: the insert binds NULL,
        // and the conflict branch copies the excluded row's columns, so both
        // paths land the same state through one statement (spike check #4).
        ai_consent::Entity::insert(ai_consent::ActiveModel {
            provider: Set(provider.to_owned()),
            consented_at: Set(now),
            revoked_at: Set(None),
            acknowledgment: Set(acknowledgment.to_owned()),
        })
        .on_conflict(
            OnConflict::column(ai_consent::Column::Provider)
                .update_columns([
                    ai_consent::Column::ConsentedAt,
                    ai_consent::Column::RevokedAt,
                    ai_consent::Column::Acknowledgment,
                ])
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await
        .map_err(|e| db_error("grant consent", e))?;

        Ok(())
    }

    /// Revoke consent for a specific cloud AI provider.
    pub async fn revoke_consent(&self, provider: &str) -> Result<(), VectorError> {
        info!(provider = provider, "Revoking AI consent");

        // pl-timestamp-write-tz: naive UTC bind into the no-zone column.
        let now = Utc::now().naive_utc();

        let affected = ai_consent::Entity::update_many()
            .col_expr(
                ai_consent::Column::RevokedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(ai_consent::Column::Provider.eq(provider))
            .filter(ai_consent::Column::RevokedAt.is_null())
            .exec(&self.conn)
            .await
            .map_err(|e| db_error("revoke consent", e))?
            .rows_affected;

        if affected == 0 {
            return Err(VectorError::ConfigError(format!(
                "No active consent found for provider '{provider}'"
            )));
        }

        Ok(())
    }

    /// Check whether active (non-revoked) consent exists for a provider.
    pub async fn has_consent(&self, provider: &str) -> Result<bool, VectorError> {
        let count = ai_consent::Entity::find()
            .filter(ai_consent::Column::Provider.eq(provider))
            .filter(ai_consent::Column::RevokedAt.is_null())
            .count(&self.conn)
            .await
            .map_err(|e| db_error("check consent", e))?;

        Ok(count > 0)
    }

    /// Get all consent records.
    pub async fn get_all_consent(&self) -> Result<Vec<ConsentRecord>, VectorError> {
        let rows = ai_consent::Entity::find()
            .order_by_desc(ai_consent::Column::ConsentedAt)
            .all(&self.conn)
            .await
            .map_err(|e| db_error("read consent records", e))?;

        Ok(rows
            .into_iter()
            .map(|m| ConsentRecord {
                provider: m.provider,
                // Stored naive, always UTC by construction (see the write path).
                consented_at: m.consented_at.and_utc(),
                revoked_at: m.revoked_at.map(|dt| dt.and_utc()),
                acknowledgment: m.acknowledgment,
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

        ai_audit_log::Entity::insert(ai_audit_log::ActiveModel {
            // pl-timestamp-write-tz: naive UTC bind into the no-zone column.
            timestamp: Set(entry.timestamp.naive_utc()),
            provider: Set(entry.provider.clone()),
            model: Set(entry.model.clone()),
            endpoint: Set(entry.endpoint.clone()),
            // The counters are real 4-byte INTEGERs on PostgreSQL; saturate
            // rather than wrap on the (not-in-practice) overflow.
            input_token_count: Set(entry.input_token_count.map(narrow_to_i32)),
            output_token_count: Set(entry.output_token_count.map(narrow_to_i32)),
            input_hash: Set(entry.input_hash.clone()),
            latency_ms: Set(entry.latency_ms.map(narrow_to_i32)),
            ..Default::default()
        })
        .exec_without_returning(&self.conn)
        .await
        .map_err(|e| db_error("log cloud call", e))?;

        Ok(())
    }

    /// Get a paginated view of the audit log (newest first).
    pub async fn get_audit_log(&self, page: u32, per_page: u32) -> Result<AuditPage, VectorError> {
        let offset = u64::from(page.saturating_sub(1)) * u64::from(per_page);

        let total = ai_audit_log::Entity::find()
            .count(&self.conn)
            .await
            .map_err(|e| db_error("count cloud audit log", e))? as i64;

        let rows = ai_audit_log::Entity::find()
            .order_by(ai_audit_log::Column::Timestamp, Order::Desc)
            .limit(u64::from(per_page))
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(|e| db_error("read cloud audit log", e))?;

        let entries = rows
            .into_iter()
            .map(|m| AuditEntry {
                // id and the counters are INTEGER/INT4 (i32) in both dialects;
                // the public entry type keeps its i64 widths.
                id: Some(i64::from(m.id)),
                timestamp: m.timestamp.and_utc(),
                provider: m.provider,
                model: m.model,
                endpoint: m.endpoint,
                input_token_count: m.input_token_count.map(i64::from),
                output_token_count: m.output_token_count.map(i64::from),
                input_hash: m.input_hash,
                latency_ms: m.latency_ms.map(i64::from),
            })
            .collect();

        Ok(AuditPage {
            entries,
            page,
            per_page,
            total,
        })
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;
    use sqlx::sqlite::SqlitePoolOptions;

    /// In-memory SQLite carrying migration 002, which owns both tables this
    /// module reads and writes. Replaying the real migration (rather than the
    /// hand-written `CREATE TABLE`s these tests used to carry) is what makes the
    /// timestamp columns genuinely `TIMESTAMP` here, matching production.
    async fn setup_db() -> Arc<Database> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let db = Database::Sqlite(pool);
        let conn = db.sea_orm();
        let raw = include_str!("../../migrations/sqlite/002_ai_consent.sql");
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

    /// Re-granting after a revoke clears `revoked_at` — the conflict branch
    /// copies the excluded row's NULL rather than leaving the old revocation in
    /// place, which would keep `has_consent` false forever.
    #[tokio::test]
    async fn test_regrant_clears_revocation() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db);

        mgr.grant_consent("openai", "ack").await.unwrap();
        mgr.revoke_consent("openai").await.unwrap();
        assert!(mgr.get_all_consent().await.unwrap()[0].revoked_at.is_some());

        mgr.grant_consent("openai", "ack again").await.unwrap();

        let all = mgr.get_all_consent().await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].revoked_at.is_none());
        assert!(mgr.has_consent("openai").await.unwrap());
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

    /// pl-timestamp-write-tz, `ai_consent`: the value stored in the no-zone
    /// column is exactly `.naive_utc()` of the written instant. A `DateTime<Utc>`
    /// bind would land the session-local wall clock on PostgreSQL instead.
    #[tokio::test]
    async fn test_consent_timestamp_stored_as_naive_utc() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db.clone());

        let before = Utc::now().naive_utc();
        mgr.grant_consent("openai", "ack").await.unwrap();
        let after = Utc::now().naive_utc();

        let stored = ai_consent::Entity::find()
            .one(&db.sea_orm())
            .await
            .unwrap()
            .expect("row");
        assert!(stored.consented_at >= before && stored.consented_at <= after);

        // And the widened read is the same instant, tagged UTC.
        let record = &mgr.get_all_consent().await.unwrap()[0];
        assert_eq!(record.consented_at, stored.consented_at.and_utc());
    }

    /// pl-timestamp-write-tz, `ai_audit_log`: a fixed instant round-trips
    /// unshifted (fixed rather than `Utc::now()` so the assertion is exact
    /// equality, not a window).
    #[tokio::test]
    async fn test_audit_timestamp_round_trips_unshifted() {
        let db = setup_db().await;
        let mgr = ConsentManager::new(db.clone());

        let written = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        mgr.log_cloud_call(&AuditEntry {
            id: None,
            timestamp: written,
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            endpoint: "/v1/chat/completions".to_string(),
            input_token_count: None,
            output_token_count: None,
            input_hash: None,
            latency_ms: None,
        })
        .await
        .unwrap();

        let stored = ai_audit_log::Entity::find()
            .one(&db.sea_orm())
            .await
            .unwrap()
            .expect("row");
        assert_eq!(stored.timestamp, written.naive_utc());

        let page = mgr.get_audit_log(1, 10).await.unwrap();
        assert_eq!(page.entries[0].timestamp, written);
    }
}
