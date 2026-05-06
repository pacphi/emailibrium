//! Cleanup audit log (Phase D, ADR-030 §Security, DDD-005/ADR-017).
//!
//! Per-operation append-only audit trail. One row is written for every
//! terminal outcome of a [`PlannedOperation`] during apply: `applied`,
//! `failed`, or `skipped`. Required for GDPR right-to-explanation; deletion
//! cascades from `cleanup_plans` (migration 024 ON DELETE CASCADE), giving
//! right-to-erasure for free.
//!
//! ## Security contract — ADR-030 §Security
//!
//! "No plan content is logged; only counts."
//!
//! This module's [`CleanupAuditEntry`] and the underlying schema
//! deliberately exclude:
//! - email_id (the (plan_id, seq) tuple lets authorised investigators
//!   join into the encrypted `cleanup_plan_operations` table).
//! - email body / subject / sender content.
//! - rule body / matchers.
//! - folder paths, label names.
//!
//! What we DO log per row: ids (plan_id, job_id, user_id, account_id),
//! seq, the action *kind* (e.g. "archive"), the source *kind*
//! (e.g. "subscription"), the outcome, optional skip_reason, and an
//! optional error code+message (which itself MUST NOT contain PII; see
//! [`crate::middleware::log_scrub`]).

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

use crate::cleanup::domain::operation::{
    ErrorCode, PlanAction, PlanSource, PlannedOperation, PlannedOperationRow, SkipReason,
};
use crate::cleanup::domain::plan::{JobId, PlanId};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("invalid uuid: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("invalid value: {0}")]
    Invalid(String),
}

/// Outcome of a single planned operation. Mirrors
/// `OperationStatus`'s terminal variants but is its own enum so we don't
/// accidentally write a `Pending` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditOutcome {
    Applied,
    Failed,
    Skipped,
}

impl AuditOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

/// One row of the cleanup audit log.
///
/// Per ADR-030 §Security this struct intentionally does NOT contain email
/// ids, email content, rule bodies, folder paths, or sample ids. Cross-ref
/// the encrypted `cleanup_plan_operations` row via `(plan_id, seq)` for
/// investigation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupAuditEntry {
    pub plan_id: PlanId,
    pub job_id: JobId,
    pub user_id: String,
    pub account_id: String,
    pub seq: u64,
    /// "materialized" or "predicate".
    pub op_kind: &'static str,
    /// camelCase PlanAction tag, e.g. "archive", "addLabel", "move".
    pub action_type: String,
    /// PlanSource tag: "subscription" | "cluster" | "rule" |
    /// "archiveStrategy" | "manual".
    pub source_type: String,
    pub outcome: AuditOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<SkipReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
    pub timestamp_ms: i64,
}

impl CleanupAuditEntry {
    /// Build an entry for a materialized row outcome. Used by the apply
    /// dispatch path. Pulls only id-level metadata from the row — no
    /// email_id, no target name, no source body.
    pub fn from_materialized(
        plan_id: PlanId,
        job_id: JobId,
        user_id: &str,
        row: &PlannedOperationRow,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            plan_id,
            job_id,
            user_id: user_id.to_string(),
            account_id: row.account_id.clone(),
            seq: row.seq,
            op_kind: "materialized",
            action_type: action_kind(&row.action).to_string(),
            source_type: source_kind(&row.source).to_string(),
            outcome,
            skip_reason: row.skip_reason,
            error: row.error.clone(),
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Build an entry for any [`PlannedOperation`] (materialized or
    /// predicate). Phase D only emits these for materialized rows; the
    /// helper exists for completeness.
    pub fn from_op(
        plan_id: PlanId,
        job_id: JobId,
        user_id: &str,
        op: &PlannedOperation,
        outcome: AuditOutcome,
        skip_reason: Option<SkipReason>,
        error: Option<ErrorCode>,
    ) -> Self {
        let (op_kind, action, source) = match op {
            PlannedOperation::Materialized(r) => ("materialized", &r.action, &r.source),
            PlannedOperation::Predicate(p) => ("predicate", &p.action, &p.source),
        };
        Self {
            plan_id,
            job_id,
            user_id: user_id.to_string(),
            account_id: op.account_id().to_string(),
            seq: op.seq(),
            op_kind,
            action_type: action_kind(action).to_string(),
            source_type: source_kind(source).to_string(),
            outcome,
            skip_reason,
            error,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }
}

/// camelCase tag for a [`PlanAction`].
fn action_kind(a: &PlanAction) -> &'static str {
    match a {
        PlanAction::Archive => "archive",
        PlanAction::AddLabel { .. } => "addLabel",
        PlanAction::Move { .. } => "move",
        PlanAction::Delete { .. } => "delete",
        PlanAction::Unsubscribe { .. } => "unsubscribe",
        PlanAction::MarkRead => "markRead",
        PlanAction::Star { .. } => "star",
    }
}

/// camelCase tag for a [`PlanSource`].
fn source_kind(s: &PlanSource) -> &'static str {
    match s {
        PlanSource::Subscription { .. } => "subscription",
        PlanSource::Cluster { .. } => "cluster",
        PlanSource::Rule { .. } => "rule",
        PlanSource::ArchiveStrategy { .. } => "archiveStrategy",
        PlanSource::Manual => "manual",
    }
}

// ---------------------------------------------------------------------------
// Writer trait + SQLite impl
// ---------------------------------------------------------------------------

#[async_trait]
pub trait CleanupAuditWriter: Send + Sync {
    /// Write a single audit entry. Idempotent on
    /// `(plan_id, job_id, seq, outcome)` via `INSERT OR IGNORE`.
    async fn write(&self, entry: CleanupAuditEntry) -> Result<(), AuditError>;

    /// Surface entries for a single plan, ordered by (timestamp, seq).
    /// The GDPR right-to-explanation surface.
    async fn list_for_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupAuditEntry>, AuditError>;

    /// Surface entries for a user, newest first, capped at `limit`.
    async fn list_for_user(
        &self,
        user_id: &str,
        limit: u32,
    ) -> Result<Vec<CleanupAuditEntry>, AuditError>;
}

pub struct SqliteCleanupAuditWriter {
    pool: SqlitePool,
}

impl SqliteCleanupAuditWriter {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CleanupAuditWriter for SqliteCleanupAuditWriter {
    async fn write(&self, entry: CleanupAuditEntry) -> Result<(), AuditError> {
        let plan_id_b: &[u8] = entry.plan_id.as_bytes();
        let job_id_b: &[u8] = entry.job_id.as_bytes();
        let user_id_b = entry.user_id.as_bytes();
        let account_id_b = entry.account_id.as_bytes();
        let skip = entry.skip_reason.map(|r| r.as_str());
        let (err_code, err_msg) = match &entry.error {
            Some(e) => (Some(e.code.as_str()), Some(e.message.as_str())),
            None => (None, None),
        };

        sqlx::query(
            "INSERT OR IGNORE INTO cleanup_audit_log
             (timestamp, plan_id, job_id, user_id, account_id, seq, op_kind,
              action_type, source_type, outcome, skip_reason, error_code, error_message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.timestamp_ms)
        .bind(plan_id_b)
        .bind(job_id_b)
        .bind(user_id_b)
        .bind(account_id_b)
        .bind(entry.seq as i64)
        .bind(entry.op_kind)
        .bind(&entry.action_type)
        .bind(&entry.source_type)
        .bind(entry.outcome.as_str())
        .bind(skip)
        .bind(err_code)
        .bind(err_msg)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_for_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        let rows: Vec<AuditRow> = sqlx::query_as::<_, AuditRow>(
            "SELECT timestamp, plan_id, job_id, user_id, account_id, seq, op_kind,
                    action_type, source_type, outcome, skip_reason, error_code, error_message
             FROM cleanup_audit_log
             WHERE plan_id = ?
             ORDER BY timestamp ASC, seq ASC",
        )
        .bind(plan_id.as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(|r| r.try_into()).collect()
    }

    async fn list_for_user(
        &self,
        user_id: &str,
        limit: u32,
    ) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        let rows: Vec<AuditRow> = sqlx::query_as::<_, AuditRow>(
            "SELECT timestamp, plan_id, job_id, user_id, account_id, seq, op_kind,
                    action_type, source_type, outcome, skip_reason, error_code, error_message
             FROM cleanup_audit_log
             WHERE user_id = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(user_id.as_bytes())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(|r| r.try_into()).collect()
    }
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    timestamp: i64,
    plan_id: Vec<u8>,
    job_id: Vec<u8>,
    user_id: Vec<u8>,
    account_id: Vec<u8>,
    seq: i64,
    op_kind: String,
    action_type: String,
    source_type: String,
    outcome: String,
    skip_reason: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl TryFrom<AuditRow> for CleanupAuditEntry {
    type Error = AuditError;

    fn try_from(r: AuditRow) -> Result<Self, Self::Error> {
        let plan_id = Uuid::from_slice(&r.plan_id)
            .map_err(|e| AuditError::Invalid(format!("plan_id: {e}")))?;
        let job_id =
            Uuid::from_slice(&r.job_id).map_err(|e| AuditError::Invalid(format!("job_id: {e}")))?;
        let user_id = String::from_utf8(r.user_id)
            .map_err(|e| AuditError::Invalid(format!("user_id utf8: {e}")))?;
        let account_id = String::from_utf8(r.account_id)
            .map_err(|e| AuditError::Invalid(format!("account_id utf8: {e}")))?;
        let outcome = AuditOutcome::from_str_opt(&r.outcome)
            .ok_or_else(|| AuditError::Invalid(format!("outcome: {}", r.outcome)))?;
        let skip_reason = match r.skip_reason {
            Some(s) => Some(
                SkipReason::from_str_opt(&s)
                    .ok_or_else(|| AuditError::Invalid(format!("skip_reason: {s}")))?,
            ),
            None => None,
        };
        let error = match (r.error_code, r.error_message) {
            (Some(c), Some(m)) => Some(ErrorCode {
                code: c,
                message: m,
            }),
            (Some(c), None) => Some(ErrorCode {
                code: c,
                message: String::new(),
            }),
            _ => None,
        };
        let op_kind: &'static str = match r.op_kind.as_str() {
            "materialized" => "materialized",
            "predicate" => "predicate",
            other => return Err(AuditError::Invalid(format!("op_kind: {other}"))),
        };
        Ok(CleanupAuditEntry {
            plan_id,
            job_id,
            user_id,
            account_id,
            seq: r.seq as u64,
            op_kind,
            action_type: r.action_type,
            source_type: r.source_type,
            outcome,
            skip_reason,
            error,
            timestamp_ms: r.timestamp,
        })
    }
}

// ---------------------------------------------------------------------------
// No-op writer (used when audit table is unavailable, e.g. in unit tests
// that don't care about audit). Never errors.
// ---------------------------------------------------------------------------

pub struct NoopCleanupAuditWriter;

#[async_trait]
impl CleanupAuditWriter for NoopCleanupAuditWriter {
    async fn write(&self, _entry: CleanupAuditEntry) -> Result<(), AuditError> {
        Ok(())
    }
    async fn list_for_plan(&self, _plan_id: PlanId) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        Ok(Vec::new())
    }
    async fn list_for_user(
        &self,
        _user_id: &str,
        _limit: u32,
    ) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::operation::{
        MoveKind, OperationStatus, PlanAction, PlanSource, PlannedOperationRow, RiskLevel,
    };
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        // Apply migration 024 (referenced by ON DELETE CASCADE) plus 025.
        for path in [
            "../../../migrations/024_cleanup_planning.sql",
            "../../../migrations/025_cleanup_audit_log.sql",
        ] {
            // include_str! requires literal paths.
            let raw = match path {
                "../../../migrations/024_cleanup_planning.sql" => {
                    include_str!("../../migrations/024_cleanup_planning.sql")
                }
                _ => include_str!("../../migrations/025_cleanup_audit_log.sql"),
            };
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
                    sqlx::query(s).execute(&pool).await.expect("migrate");
                }
            }
        }
        pool
    }

    fn sample_row(seq: u64) -> PlannedOperationRow {
        PlannedOperationRow {
            seq,
            account_id: "acct-a".into(),
            email_id: Some(format!("e{seq}")),
            action: PlanAction::Archive,
            source: PlanSource::Subscription {
                sender: "news@example.com".into(),
            },
            target: None,
            reverse_op: None,
            risk: RiskLevel::Low,
            status: OperationStatus::Pending,
            skip_reason: None,
            applied_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn audit_write_then_list_for_plan() {
        let pool = fresh_pool().await;
        let writer = SqliteCleanupAuditWriter::new(pool);
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        for seq in 1..=3 {
            let row = sample_row(seq);
            let entry = CleanupAuditEntry::from_materialized(
                plan_id,
                job_id,
                "user-1",
                &row,
                AuditOutcome::Applied,
            );
            writer.write(entry).await.expect("write");
        }

        let rows = writer.list_for_plan(plan_id).await.expect("list");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.outcome == AuditOutcome::Applied));
        assert_eq!(rows[0].user_id, "user-1");
        assert_eq!(rows[0].account_id, "acct-a");
        assert_eq!(rows[0].action_type, "archive");
        assert_eq!(rows[0].source_type, "subscription");
        assert_eq!(rows[0].op_kind, "materialized");
    }

    #[tokio::test]
    async fn audit_write_idempotent_on_duplicate_seq() {
        let pool = fresh_pool().await;
        let writer = SqliteCleanupAuditWriter::new(pool);
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();
        let row = sample_row(7);
        let mk = || {
            CleanupAuditEntry::from_materialized(
                plan_id,
                job_id,
                "user-1",
                &row,
                AuditOutcome::Applied,
            )
        };
        writer.write(mk()).await.expect("first");
        writer
            .write(mk())
            .await
            .expect("second (should be ignored)");
        writer.write(mk()).await.expect("third (also ignored)");
        let rows = writer.list_for_plan(plan_id).await.expect("list");
        assert_eq!(
            rows.len(),
            1,
            "UNIQUE (plan_id, job_id, seq, outcome) prevents duplicate audit rows"
        );

        // But the SAME (seq) with a DIFFERENT outcome IS a separate row —
        // a row could legitimately be retried with a different outcome.
        let mut row2 = sample_row(7);
        row2.error = Some(ErrorCode {
            code: "x".into(),
            message: "y".into(),
        });
        let failed_entry = CleanupAuditEntry::from_materialized(
            plan_id,
            job_id,
            "user-1",
            &row2,
            AuditOutcome::Failed,
        );
        writer.write(failed_entry).await.expect("failed write");
        let rows = writer.list_for_plan(plan_id).await.expect("list");
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn audit_excludes_email_content() {
        // Compile-time + runtime guard: the entry struct + the SELECT we
        // issue must not surface email_id, body, target name, etc.
        let pool = fresh_pool().await;
        let writer = SqliteCleanupAuditWriter::new(pool.clone());
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();
        let row = PlannedOperationRow {
            email_id: Some("super-secret-email-id".into()),
            ..sample_row(1)
        };
        let entry = CleanupAuditEntry::from_materialized(
            plan_id,
            job_id,
            "user-1",
            &row,
            AuditOutcome::Applied,
        );
        writer.write(entry).await.expect("write");

        // Schema introspection: verify no columns leak email content.
        #[derive(sqlx::FromRow, Debug)]
        struct ColInfo {
            name: String,
        }
        let cols: Vec<ColInfo> =
            sqlx::query_as::<_, ColInfo>("SELECT name FROM pragma_table_info('cleanup_audit_log')")
                .fetch_all(&pool)
                .await
                .expect("pragma");
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        for forbidden in [
            "email_id",
            "email",
            "subject",
            "body",
            "folder",
            "folder_path",
            "target_name",
            "target_id",
            "rule_body",
            "sample_ids",
            "sample_email_ids",
            "sender",
        ] {
            assert!(
                !names.iter().any(|n| n == &forbidden),
                "audit table must not contain column `{forbidden}` (ADR-030 §Security)"
            );
        }

        // And the in-memory entry struct: serializing it must not leak
        // the email id we passed in.
        let json = serde_json::to_string(
            &writer
                .list_for_plan(plan_id)
                .await
                .expect("list")
                .into_iter()
                .next()
                .expect("one row"),
        )
        .expect("json");
        assert!(
            !json.contains("super-secret-email-id"),
            "audit JSON leaked email_id: {json}"
        );
    }

    #[tokio::test]
    async fn audit_records_skip_reason() {
        let pool = fresh_pool().await;
        let writer = SqliteCleanupAuditWriter::new(pool);
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        let mut entry = CleanupAuditEntry::from_materialized(
            plan_id,
            job_id,
            "user-1",
            &sample_row(1),
            AuditOutcome::Skipped,
        );
        entry.skip_reason = Some(SkipReason::StateDrift);
        writer.write(entry).await.expect("write");

        let rows = writer.list_for_plan(plan_id).await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].outcome, AuditOutcome::Skipped);
        assert_eq!(rows[0].skip_reason, Some(SkipReason::StateDrift));
    }

    #[tokio::test]
    async fn audit_action_type_camelcase() {
        let pool = fresh_pool().await;
        let writer = SqliteCleanupAuditWriter::new(pool);
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        let mut row = sample_row(1);
        row.action = PlanAction::AddLabel {
            kind: MoveKind::Label,
        };
        let entry = CleanupAuditEntry::from_materialized(
            plan_id,
            job_id,
            "user-1",
            &row,
            AuditOutcome::Applied,
        );
        writer.write(entry).await.expect("write");
        let rows = writer.list_for_plan(plan_id).await.expect("list");
        assert_eq!(rows[0].action_type, "addLabel");
    }
}
