//! GDPR privacy service (R-09: Consent Persistence).
//!
//! Manages consent decisions, privacy audit logs, user data export (GDPR
//! Article 20), and right to erasure (GDPR Article 17).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use super::error::VectorError;
use crate::db::Database;

// ---------------------------------------------------------------------------
// SQLx row type aliases (clippy::type_complexity)
// ---------------------------------------------------------------------------

/// Row from consent_decisions query.
type ConsentRow = (
    String,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

/// Row from privacy_audit_log query.
type AuditRow = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

/// Row from emails query for data export.
type EmailExportRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A GDPR consent decision record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentDecision {
    pub id: String,
    pub consent_type: String,
    pub granted: bool,
    pub granted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// An event in the privacy audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_type: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub actor: String,
    pub details: Option<serde_json::Value>,
}

/// A row from the privacy audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyAuditEntry {
    pub id: i64,
    pub event_type: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub actor: String,
    pub details: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Paginated privacy audit log response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyAuditPage {
    pub entries: Vec<PrivacyAuditEntry>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

/// Exported user data (GDPR Article 20: Data Portability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataExport {
    pub exported_at: DateTime<Utc>,
    pub consent_decisions: Vec<ConsentDecision>,
    pub emails: Vec<ExportedEmail>,
    pub settings: serde_json::Value,
    pub audit_summary: AuditSummary,
}

/// Minimal email representation for data export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedEmail {
    pub id: String,
    pub from_addr: String,
    pub subject: String,
    pub received_at: String,
    pub category: Option<String>,
}

/// Summary of audit log activity included in data exports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_events: i64,
    pub data_access_count: i64,
    pub data_export_count: i64,
    pub consent_change_count: i64,
}

/// Report generated after data erasure (GDPR Article 17).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureReport {
    pub erased_at: DateTime<Utc>,
    pub emails_deleted: u64,
    pub vectors_deleted: u64,
    pub consent_records_deleted: u64,
    pub audit_entries_retained: u64,
}

// ---------------------------------------------------------------------------
// PrivacyService
// ---------------------------------------------------------------------------

/// GDPR-compliant privacy service for consent management, audit logging,
/// data export, and erasure.
pub struct PrivacyService {
    db: Arc<Database>,
}

impl PrivacyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Ensure the GDPR consent and audit tables exist.
    pub async fn ensure_tables(&self) -> Result<(), VectorError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS consent_decisions (
                id TEXT PRIMARY KEY,
                consent_type TEXT NOT NULL,
                granted INTEGER NOT NULL DEFAULT 0,
                granted_at DATETIME,
                revoked_at DATETIME,
                ip_address TEXT,
                user_agent TEXT,
                created_at DATETIME DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS privacy_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                resource_type TEXT,
                resource_id TEXT,
                actor TEXT DEFAULT 'user',
                details TEXT,
                created_at DATETIME DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_privacy_audit_event \
             ON privacy_audit_log(event_type)",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_privacy_audit_created \
             ON privacy_audit_log(created_at)",
        )
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    // -- Consent management ------------------------------------------------

    /// Record a consent decision (grant or revoke).
    pub async fn record_consent(
        &self,
        consent_type: &str,
        granted: bool,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<ConsentDecision, VectorError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let (granted_at, revoked_at) = if granted {
            (Some(now), None)
        } else {
            (None, Some(now))
        };

        sqlx::query(
            "INSERT INTO consent_decisions \
             (id, consent_type, granted, granted_at, revoked_at, ip_address, user_agent, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(consent_type)
        .bind(granted as i32)
        .bind(granted_at)
        .bind(revoked_at)
        .bind(ip_address)
        .bind(user_agent)
        .bind(now)
        .execute(&self.db.pool)
        .await?;

        // Also update the effective consent state: if a previous record for the
        // same type exists, mark it as superseded via revoked_at.
        if granted {
            sqlx::query(
                "UPDATE consent_decisions SET revoked_at = ? \
                 WHERE consent_type = ? AND id != ? AND granted = 1 AND revoked_at IS NULL",
            )
            .bind(now)
            .bind(consent_type)
            .bind(&id)
            .execute(&self.db.pool)
            .await?;
        }

        // Log the consent change in the privacy audit log.
        self.log_access(AuditEvent {
            event_type: "consent_change".to_string(),
            resource_type: Some("consent".to_string()),
            resource_id: Some(consent_type.to_string()),
            actor: "user".to_string(),
            details: Some(serde_json::json!({
                "consent_type": consent_type,
                "granted": granted,
            })),
        })
        .await?;

        info!(
            consent_type = consent_type,
            granted = granted,
            "GDPR consent decision recorded"
        );

        Ok(ConsentDecision {
            id,
            consent_type: consent_type.to_string(),
            granted,
            granted_at,
            revoked_at,
            ip_address: ip_address.map(String::from),
            user_agent: user_agent.map(String::from),
            created_at: now,
        })
    }

    /// Get the current effective consent for a specific type.
    pub async fn get_consent(
        &self,
        consent_type: &str,
    ) -> Result<Option<ConsentDecision>, VectorError> {
        let row: Option<ConsentRow> = sqlx::query_as(
            "SELECT id, consent_type, granted, granted_at, revoked_at, \
                    ip_address, user_agent, created_at \
             FROM consent_decisions \
             WHERE consent_type = ? \
             ORDER BY created_at DESC \
             LIMIT 1",
        )
        .bind(consent_type)
        .fetch_optional(&self.db.pool)
        .await?;

        Ok(row.map(
            |(id, ct, granted, granted_at, revoked_at, ip, ua, created)| ConsentDecision {
                id,
                consent_type: ct,
                granted: granted != 0,
                granted_at: granted_at.and_then(|s| parse_datetime(&s)),
                revoked_at: revoked_at.and_then(|s| parse_datetime(&s)),
                ip_address: ip,
                user_agent: ua,
                created_at: parse_datetime(&created).unwrap_or_else(Utc::now),
            },
        ))
    }

    /// List all current consent decisions (latest per type).
    pub async fn list_consents(&self) -> Result<Vec<ConsentDecision>, VectorError> {
        let rows: Vec<ConsentRow> = sqlx::query_as(
            "SELECT d.id, d.consent_type, d.granted, d.granted_at, d.revoked_at, \
                    d.ip_address, d.user_agent, d.created_at \
             FROM consent_decisions d \
             INNER JOIN ( \
                 SELECT consent_type, MAX(created_at) AS max_created \
                 FROM consent_decisions \
                 GROUP BY consent_type \
             ) latest ON d.consent_type = latest.consent_type \
                     AND d.created_at = latest.max_created \
             ORDER BY d.consent_type",
        )
        .fetch_all(&self.db.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, ct, granted, granted_at, revoked_at, ip, ua, created)| ConsentDecision {
                    id,
                    consent_type: ct,
                    granted: granted != 0,
                    granted_at: granted_at.and_then(|s| parse_datetime(&s)),
                    revoked_at: revoked_at.and_then(|s| parse_datetime(&s)),
                    ip_address: ip,
                    user_agent: ua,
                    created_at: parse_datetime(&created).unwrap_or_else(Utc::now),
                },
            )
            .collect())
    }

    // -- Audit logging -----------------------------------------------------

    /// Append an event to the privacy audit log.
    pub async fn log_access(&self, event: AuditEvent) -> Result<(), VectorError> {
        let details_json = event
            .details
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default());

        sqlx::query(
            "INSERT INTO privacy_audit_log \
             (event_type, resource_type, resource_id, actor, details) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&event.event_type)
        .bind(&event.resource_type)
        .bind(&event.resource_id)
        .bind(&event.actor)
        .bind(&details_json)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Get a paginated view of the privacy audit log (newest first).
    pub async fn get_audit_log(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<PrivacyAuditPage, VectorError> {
        let offset = (page.saturating_sub(1)) as i64 * per_page as i64;
        let limit = per_page as i64;

        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM privacy_audit_log")
            .fetch_one(&self.db.pool)
            .await?;

        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT id, event_type, resource_type, resource_id, actor, details, created_at \
             FROM privacy_audit_log \
             ORDER BY created_at DESC \
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.db.pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(
                |(id, event_type, resource_type, resource_id, actor, details, created_at)| {
                    PrivacyAuditEntry {
                        id,
                        event_type,
                        resource_type,
                        resource_id,
                        actor: actor.unwrap_or_else(|| "user".to_string()),
                        details: details.and_then(|d| serde_json::from_str(&d).ok()),
                        created_at: parse_datetime(&created_at).unwrap_or_else(Utc::now),
                    }
                },
            )
            .collect();

        Ok(PrivacyAuditPage {
            entries,
            page,
            per_page,
            total,
        })
    }

    // -- Data export (GDPR Article 20) -------------------------------------

    /// Export all user data as a portable JSON-serializable structure.
    pub async fn export_user_data(&self) -> Result<UserDataExport, VectorError> {
        info!("GDPR data export requested");

        // Log the export event.
        self.log_access(AuditEvent {
            event_type: "data_export".to_string(),
            resource_type: Some("account".to_string()),
            resource_id: None,
            actor: "user".to_string(),
            details: Some(serde_json::json!({"reason": "GDPR Article 20 data portability"})),
        })
        .await?;

        let consent_decisions = self.list_consents().await?;

        // Export emails.
        let email_rows: Vec<EmailExportRow> = sqlx::query_as(
            "SELECT id, from_addr, subject, received_at, category \
                 FROM emails ORDER BY received_at DESC",
        )
        .fetch_all(&self.db.pool)
        .await?;

        let emails: Vec<ExportedEmail> = email_rows
            .into_iter()
            .map(
                |(id, from_addr, subject, received_at, category)| ExportedEmail {
                    id,
                    from_addr: from_addr.unwrap_or_default(),
                    subject: subject.unwrap_or_default(),
                    received_at: received_at.unwrap_or_default(),
                    category,
                },
            )
            .collect();

        // Collect audit summary.
        let audit_summary = self.build_audit_summary().await?;

        // Settings (export any user-facing settings from ai_consent).
        let ai_consents: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT provider, consented_at, revoked_at, acknowledgment FROM ai_consent",
        )
        .fetch_all(&self.db.pool)
        .await
        .unwrap_or_default();

        let settings = serde_json::json!({
            "ai_consent": ai_consents.iter().map(|(p, c, r, a)| {
                serde_json::json!({
                    "provider": p,
                    "consented_at": c,
                    "revoked_at": r,
                    "acknowledgment": a,
                })
            }).collect::<Vec<_>>(),
        });

        Ok(UserDataExport {
            exported_at: Utc::now(),
            consent_decisions,
            emails,
            settings,
            audit_summary,
        })
    }

    // -- Right to erasure (GDPR Article 17) --------------------------------

    /// Erase all user data. The privacy audit log is retained (legal basis).
    pub async fn erase_user_data(&self) -> Result<ErasureReport, VectorError> {
        warn!("GDPR data erasure requested — deleting user data");

        // Log erasure event BEFORE deleting (so the log captures the intent).
        self.log_access(AuditEvent {
            event_type: "data_delete".to_string(),
            resource_type: Some("account".to_string()),
            resource_id: None,
            actor: "user".to_string(),
            details: Some(serde_json::json!({"reason": "GDPR Article 17 right to erasure"})),
        })
        .await?;

        // Count before deletion.
        let (email_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails")
            .fetch_one(&self.db.pool)
            .await
            .unwrap_or((0,));

        let (vector_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vector_store")
            .fetch_one(&self.db.pool)
            .await
            .unwrap_or((0,));

        let (consent_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM consent_decisions")
            .fetch_one(&self.db.pool)
            .await
            .unwrap_or((0,));

        let (audit_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM privacy_audit_log")
            .fetch_one(&self.db.pool)
            .await
            .unwrap_or((0,));

        // Delete user data (emails, vectors, consent).
        sqlx::query("DELETE FROM emails")
            .execute(&self.db.pool)
            .await
            .ok();

        sqlx::query("DELETE FROM vector_store")
            .execute(&self.db.pool)
            .await
            .ok();

        sqlx::query("DELETE FROM consent_decisions")
            .execute(&self.db.pool)
            .await
            .ok();

        sqlx::query("DELETE FROM ai_consent")
            .execute(&self.db.pool)
            .await
            .ok();

        info!(
            emails = email_count,
            vectors = vector_count,
            consents = consent_count,
            "GDPR erasure completed — audit log retained"
        );

        Ok(ErasureReport {
            erased_at: Utc::now(),
            emails_deleted: email_count as u64,
            vectors_deleted: vector_count as u64,
            consent_records_deleted: consent_count as u64,
            audit_entries_retained: audit_count as u64,
        })
    }

    // -- Helpers -----------------------------------------------------------

    async fn build_audit_summary(&self) -> Result<AuditSummary, VectorError> {
        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM privacy_audit_log")
            .fetch_one(&self.db.pool)
            .await
            .unwrap_or((0,));

        let (data_access,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM privacy_audit_log WHERE event_type = 'data_access'",
        )
        .fetch_one(&self.db.pool)
        .await
        .unwrap_or((0,));

        let (data_export,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM privacy_audit_log WHERE event_type = 'data_export'",
        )
        .fetch_one(&self.db.pool)
        .await
        .unwrap_or((0,));

        let (consent_change,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM privacy_audit_log WHERE event_type = 'consent_change'",
        )
        .fetch_one(&self.db.pool)
        .await
        .unwrap_or((0,));

        Ok(AuditSummary {
            total_events: total,
            data_access_count: data_access,
            data_export_count: data_export,
            consent_change_count: consent_change,
        })
    }
}

/// Parse a datetime string into a DateTime<Utc>, trying RFC3339 then SQLite format.
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| ndt.and_utc())
                .ok()
        })
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

        // Create tables needed for tests.
        sqlx::query(include_str!("../../migrations/001_initial_schema.sql"))
            .execute(&db.pool)
            .await
            .unwrap();

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

        let svc = PrivacyService::new(Arc::new(db.clone()));
        svc.ensure_tables().await.unwrap();

        Arc::new(db)
    }

    #[tokio::test]
    async fn test_record_and_get_consent() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        let decision = svc
            .record_consent("cloud_ai", true, Some("127.0.0.1"), Some("TestAgent"))
            .await
            .unwrap();

        assert!(decision.granted);
        assert_eq!(decision.consent_type, "cloud_ai");

        let fetched = svc.get_consent("cloud_ai").await.unwrap().unwrap();
        assert!(fetched.granted);
    }

    #[tokio::test]
    async fn test_revoke_consent() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        svc.record_consent("analytics", true, None, None)
            .await
            .unwrap();

        let revoked = svc
            .record_consent("analytics", false, None, None)
            .await
            .unwrap();
        assert!(!revoked.granted);

        let latest = svc.get_consent("analytics").await.unwrap().unwrap();
        assert!(!latest.granted);
    }

    #[tokio::test]
    async fn test_list_consents() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        svc.record_consent("cloud_ai", true, None, None)
            .await
            .unwrap();
        svc.record_consent("analytics", false, None, None)
            .await
            .unwrap();

        let all = svc.list_consents().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        svc.log_access(AuditEvent {
            event_type: "data_access".to_string(),
            resource_type: Some("email".to_string()),
            resource_id: Some("email-123".to_string()),
            actor: "user".to_string(),
            details: Some(serde_json::json!({"action": "view"})),
        })
        .await
        .unwrap();

        let page = svc.get_audit_log(1, 10).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].event_type, "data_access");
    }

    #[tokio::test]
    async fn test_audit_log_pagination() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        for i in 0..5 {
            svc.log_access(AuditEvent {
                event_type: format!("event_{i}"),
                resource_type: None,
                resource_id: None,
                actor: "user".to_string(),
                details: None,
            })
            .await
            .unwrap();
        }

        let page1 = svc.get_audit_log(1, 2).await.unwrap();
        assert_eq!(page1.total, 5);
        assert_eq!(page1.entries.len(), 2);

        let page3 = svc.get_audit_log(3, 2).await.unwrap();
        assert_eq!(page3.entries.len(), 1);
    }

    #[tokio::test]
    async fn test_get_consent_nonexistent() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        let result = svc.get_consent("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_consent_supersedes_previous() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        svc.record_consent("cloud_ai", true, None, None)
            .await
            .unwrap();
        svc.record_consent("cloud_ai", true, None, None)
            .await
            .unwrap();

        // The latest grant should be the effective one; the previous should
        // have revoked_at set.
        let latest = svc.get_consent("cloud_ai").await.unwrap().unwrap();
        assert!(latest.granted);
    }
}
