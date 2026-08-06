//! GDPR privacy service (R-09: Consent Persistence).
//!
//! Manages consent decisions, privacy audit logs, user data export (GDPR
//! Article 20), and right to erasure (GDPR Article 17).
//!
//! Single-code-path SeaORM (ADR-036): the `consent_decisions`,
//! `privacy_audit_log`, `emails` and `ai_consent` entities own encode/decode per
//! backend, so every statement below is one query that runs unchanged against
//! SQLite and PostgreSQL. Two hand-rolled things are gone with the port: the
//! `parse_datetime()` helper this file applied to `TIMESTAMPTZ` columns it read
//! back as `String` (ADR-035 §2.6's decode class, now library-owned), and the
//! runtime `CREATE TABLE` DDL that duplicated — and could silently diverge from
//! — migration 010.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, NullOrdering};
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, Order, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use super::error::VectorError;
use crate::db::entities::{ai_consent, consent_decisions, emails, privacy_audit_log};
use crate::db::Database;

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
    conn: DatabaseConnection,
}

impl PrivacyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { conn: db.sea_orm() }
    }

    /// Retained no-op: migrations own this schema.
    ///
    /// Migration 010 creates `consent_decisions` and `privacy_audit_log` for both
    /// backends. The runtime `CREATE TABLE` this used to run was SQLite-shaped DDL
    /// (`DATETIME` columns, `AUTOINCREMENT`, `datetime('now')` defaults) that
    /// PostgreSQL either rejects or silently interprets differently — a second
    /// schema definition free to drift from the migration it duplicated. ADR-036
    /// keeps one source of truth. The signature stays until `vectors/mod.rs` is
    /// ported and its call site drops.
    pub async fn ensure_tables(&self) -> Result<(), VectorError> {
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

        consent_decisions::Entity::insert(consent_decisions::ActiveModel {
            id: Set(id.clone()),
            consent_type: Set(consent_type.to_owned()),
            // INTEGER 0/1 flag, not BOOLEAN — the entity mirrors the DDL.
            granted: Set(granted as i32),
            granted_at: Set(granted_at),
            revoked_at: Set(revoked_at),
            ip_address: Set(ip_address.map(String::from)),
            user_agent: Set(user_agent.map(String::from)),
            created_at: Set(Some(now)),
        })
        .exec_without_returning(&self.conn)
        .await
        .map_err(|e| db_error("record consent", e))?;

        // Also update the effective consent state: if a previous record for the
        // same type exists, mark it as superseded via revoked_at.
        if granted {
            consent_decisions::Entity::update_many()
                .col_expr(consent_decisions::Column::RevokedAt, Expr::value(now))
                .filter(consent_decisions::Column::ConsentType.eq(consent_type))
                .filter(consent_decisions::Column::Id.ne(id.as_str()))
                .filter(consent_decisions::Column::Granted.eq(1))
                .filter(consent_decisions::Column::RevokedAt.is_null())
                .exec(&self.conn)
                .await
                .map_err(|e| db_error("supersede consent", e))?;
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
        let row = consent_decisions::Entity::find()
            .filter(consent_decisions::Column::ConsentType.eq(consent_type))
            // NULLS LAST is explicit because the two backends disagree on the
            // default: SQLite sorts NULLs last under DESC, PostgreSQL first. Every
            // row `record_consent` writes has a `created_at`, so this only matters
            // for rows written before that was true — pinning it keeps "latest"
            // meaning the same thing on both.
            .order_by_with_nulls(
                consent_decisions::Column::CreatedAt,
                Order::Desc,
                NullOrdering::Last,
            )
            .one(&self.conn)
            .await
            .map_err(|e| db_error("get consent", e))?;

        Ok(row.map(to_consent_decision))
    }

    /// List all current consent decisions (latest per type).
    pub async fn list_consents(&self) -> Result<Vec<ConsentDecision>, VectorError> {
        // The correlated `MAX(created_at)` self-join this replaces needed no
        // escape hatch: "latest per type" is one ordered read plus a first-per-key
        // scan, and the scan is strictly better defined than the join was — the
        // join emitted *every* row tied on `created_at` (duplicate types) and
        // dropped a type entirely when its rows had a NULL `created_at`
        // (`created_at = NULL` is never true). The table holds one row per consent
        // action, so reading it whole is not a scale concern.
        let rows = consent_decisions::Entity::find()
            .order_by_asc(consent_decisions::Column::ConsentType)
            .order_by_with_nulls(
                consent_decisions::Column::CreatedAt,
                Order::Desc,
                NullOrdering::Last,
            )
            .all(&self.conn)
            .await
            .map_err(|e| db_error("list consents", e))?;

        let mut latest = Vec::new();
        let mut seen: Option<String> = None;
        for model in rows {
            if seen.as_deref() != Some(model.consent_type.as_str()) {
                seen = Some(model.consent_type.clone());
                latest.push(to_consent_decision(model));
            }
        }

        Ok(latest)
    }

    // -- Audit logging -----------------------------------------------------

    /// Append an event to the privacy audit log.
    pub async fn log_access(&self, event: AuditEvent) -> Result<(), VectorError> {
        let details_json = event
            .details
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default());

        privacy_audit_log::Entity::insert(privacy_audit_log::ActiveModel {
            event_type: Set(event.event_type),
            resource_type: Set(event.resource_type),
            resource_id: Set(event.resource_id),
            actor: Set(Some(event.actor)),
            details: Set(details_json),
            // `id` is generated by the DB (AUTOINCREMENT / GENERATED AS IDENTITY)
            // and `created_at` takes the column default, as before the port.
            ..Default::default()
        })
        .exec_without_returning(&self.conn)
        .await
        .map_err(|e| db_error("log privacy event", e))?;

        Ok(())
    }

    /// Get a paginated view of the privacy audit log (newest first).
    pub async fn get_audit_log(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<PrivacyAuditPage, VectorError> {
        let offset = u64::from(page.saturating_sub(1)) * u64::from(per_page);

        let total = privacy_audit_log::Entity::find()
            .count(&self.conn)
            .await
            .map_err(|e| db_error("count privacy audit log", e))? as i64;

        let rows = privacy_audit_log::Entity::find()
            .order_by_with_nulls(
                privacy_audit_log::Column::CreatedAt,
                Order::Desc,
                NullOrdering::Last,
            )
            .limit(u64::from(per_page))
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(|e| db_error("read privacy audit log", e))?;

        let entries = rows
            .into_iter()
            .map(|m| PrivacyAuditEntry {
                // `id` is INTEGER/INT4 in both dialects (i32); the public entry
                // type keeps its i64 width.
                id: i64::from(m.id),
                event_type: m.event_type,
                resource_type: m.resource_type,
                resource_id: m.resource_id,
                actor: m.actor.unwrap_or_else(|| "user".to_string()),
                details: m.details.and_then(|d| serde_json::from_str(&d).ok()),
                created_at: m.created_at.unwrap_or_else(Utc::now),
            })
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

        // Export emails. `received_at` is plain TIMESTAMP (no zone) — it decodes
        // as NaiveDateTime and is rendered as RFC3339 UTC, the settled policy for
        // every timestamp this codebase hands to a client.
        let email_rows: Vec<(
            String,
            String,
            String,
            chrono::NaiveDateTime,
            Option<String>,
        )> = emails::Entity::find()
            .select_only()
            .column(emails::Column::Id)
            .column(emails::Column::FromAddr)
            .column(emails::Column::Subject)
            .column(emails::Column::ReceivedAt)
            .column(emails::Column::Category)
            .order_by_desc(emails::Column::ReceivedAt)
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(|e| db_error("export emails", e))?;

        let emails: Vec<ExportedEmail> = email_rows
            .into_iter()
            .map(
                |(id, from_addr, subject, received_at, category)| ExportedEmail {
                    id,
                    from_addr,
                    subject,
                    received_at: format_timestamp(received_at),
                    // NULL category reads as the column's own default rather
                    // than erroring the whole batch (the lenient unification
                    // this phase settled on).
                    category: Some(category.unwrap_or_else(|| "Uncategorized".to_string())),
                },
            )
            .collect();

        // Collect audit summary.
        let audit_summary = self.build_audit_summary().await?;

        // Settings (export any user-facing settings from ai_consent). A missing
        // table stays non-fatal, as before the port.
        let ai_consents = ai_consent::Entity::find()
            .all(&self.conn)
            .await
            .unwrap_or_default();

        let settings = serde_json::json!({
            "ai_consent": ai_consents.iter().map(|c| {
                serde_json::json!({
                    "provider": c.provider,
                    "consented_at": format_timestamp(c.consented_at),
                    "revoked_at": c.revoked_at.map(format_timestamp),
                    "acknowledgment": c.acknowledgment,
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

        // Count before deletion. Counting stays best-effort (a failure reports 0
        // rather than aborting the erasure), as before the port.
        let email_count = emails::Entity::find().count(&self.conn).await.unwrap_or(0) as i64;
        let consent_count = consent_decisions::Entity::find()
            .count(&self.conn)
            .await
            .unwrap_or(0) as i64;
        let audit_count = privacy_audit_log::Entity::find()
            .count(&self.conn)
            .await
            .unwrap_or(0) as i64;

        // Delete user data (emails, consent). Deletes stay best-effort for the
        // same reason the counts are.
        emails::Entity::delete_many().exec(&self.conn).await.ok();
        consent_decisions::Entity::delete_many()
            .exec(&self.conn)
            .await
            .ok();
        ai_consent::Entity::delete_many()
            .exec(&self.conn)
            .await
            .ok();

        info!(
            emails = email_count,
            consents = consent_count,
            "GDPR erasure completed — audit log retained"
        );

        Ok(ErasureReport {
            erased_at: Utc::now(),
            emails_deleted: email_count as u64,
            // `vector_store` is a phantom table: no migration in either dialect
            // creates it. The COUNT and DELETE that used to target it always
            // errored, and both were written to swallow the error — so the
            // reported figure was always 0 and nothing was ever deleted. The
            // statements are gone; the observable result is unchanged. Vectors
            // live in RuVector (ADR-003), which erasure reaches through its own
            // service, not through SQL.
            vectors_deleted: 0,
            consent_records_deleted: consent_count as u64,
            audit_entries_retained: audit_count as u64,
        })
    }

    // -- Helpers -----------------------------------------------------------

    async fn build_audit_summary(&self) -> Result<AuditSummary, VectorError> {
        let count_of = |event_type: Option<&'static str>| {
            let mut query = privacy_audit_log::Entity::find();
            if let Some(event_type) = event_type {
                query = query.filter(privacy_audit_log::Column::EventType.eq(event_type));
            }
            query.count(&self.conn)
        };

        Ok(AuditSummary {
            total_events: count_of(None).await.unwrap_or(0) as i64,
            data_access_count: count_of(Some("data_access")).await.unwrap_or(0) as i64,
            data_export_count: count_of(Some("data_export")).await.unwrap_or(0) as i64,
            consent_change_count: count_of(Some("consent_change")).await.unwrap_or(0) as i64,
        })
    }
}

fn to_consent_decision(model: consent_decisions::Model) -> ConsentDecision {
    ConsentDecision {
        id: model.id,
        consent_type: model.consent_type,
        granted: model.granted != 0,
        granted_at: model.granted_at,
        revoked_at: model.revoked_at,
        ip_address: model.ip_address,
        user_agent: model.user_agent,
        created_at: model.created_at.unwrap_or_else(Utc::now),
    }
}

/// Render a no-zone `TIMESTAMP` column as RFC3339 UTC for export payloads.
fn format_timestamp(ts: chrono::NaiveDateTime) -> String {
    ts.and_utc().to_rfc3339()
}

/// Map a SeaORM error onto this module's error type.
///
/// `VectorError::Db` exists, but this module keeps `StoreFailed` with an
/// operation-name prefix: the prefix keeps the message diagnosable, and the
/// variant is part of this module's observable error shape (callers map every
/// variant to the same HTTP status, so switching would buy nothing).
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

    /// In-memory SQLite carrying the migrations this module's entities span:
    /// 001 (`emails`), 002 (`ai_consent`) and 010 (`consent_decisions`,
    /// `privacy_audit_log`). Replaying the real migrations is what replaced the
    /// runtime DDL `ensure_tables()` used to run — tests and production now agree
    /// on one schema definition.
    async fn setup_db() -> Arc<Database> {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/002_ai_consent.sql"),
            include_str!("../../migrations/sqlite/010_gdpr_consent.sql"),
        ] {
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
        }

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

    /// `consent_decisions`' timestamps are TIMESTAMPTZ: what goes in as a
    /// `DateTime<Utc>` comes back as the same instant, not a re-parsed string.
    /// The pre-port code read these columns as `String` and hand-parsed them,
    /// which silently yielded `Utc::now()` whenever the shape didn't match.
    #[tokio::test]
    async fn test_consent_timestamps_round_trip_as_utc() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        let before = Utc::now();
        let written = svc
            .record_consent("cloud_ai", true, None, None)
            .await
            .unwrap();
        let after = Utc::now();

        let read_back = svc.get_consent("cloud_ai").await.unwrap().unwrap();
        let granted_at = read_back.granted_at.expect("granted_at persisted");

        assert_eq!(granted_at, written.granted_at.unwrap());
        assert!(granted_at >= before && granted_at <= after);
        assert!(read_back.created_at >= before && read_back.created_at <= after);
        assert!(read_back.revoked_at.is_none());
    }

    /// Erasure clears emails and consent while the audit log survives — and the
    /// vector count is the phantom-table 0 the pre-port code also always reported.
    #[tokio::test]
    async fn test_erase_user_data_retains_audit_log() {
        let db = setup_db().await;
        let svc = PrivacyService::new(db);

        svc.record_consent("cloud_ai", true, None, None)
            .await
            .unwrap();

        let report = svc.erase_user_data().await.unwrap();
        assert_eq!(report.consent_records_deleted, 1);
        assert_eq!(report.vectors_deleted, 0);
        assert!(report.audit_entries_retained >= 2); // consent_change + data_delete

        assert!(svc.list_consents().await.unwrap().is_empty());
        assert!(svc.get_audit_log(1, 10).await.unwrap().total >= 2);
    }
}
