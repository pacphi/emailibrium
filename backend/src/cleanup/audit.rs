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
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::cleanup::domain::operation::{
    ErrorCode, PlanAction, PlanSource, PlannedOperation, PlannedOperationRow, SkipReason,
};
use crate::cleanup::domain::plan::{JobId, PlanId};
use crate::db::entities::cleanup_audit_log as audit_log;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("db: {0}")]
    Db(#[from] sea_orm::DbErr),
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
// Writer trait + SeaORM impl
// ---------------------------------------------------------------------------

#[async_trait]
pub trait CleanupAuditWriter: Send + Sync {
    /// Write a single audit entry. Idempotent on
    /// `(plan_id, job_id, seq, outcome)`: a duplicate write is a silent no-op.
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

/// Single-code-path SeaORM audit writer (ADR-036) — the same bodies run against
/// SQLite and PostgreSQL. See `cleanup/repository/plan_repo.rs` for the exemplar
/// this follows.
pub struct SeaOrmCleanupAuditWriter {
    conn: DatabaseConnection,
}

/// Pre-port name, kept so `main.rs`'s construction site keeps compiling.
/// DELETE IN PHASE 3G together with the last caller that spells it this way.
pub type SqliteCleanupAuditWriter = SeaOrmCleanupAuditWriter;

impl SeaOrmCleanupAuditWriter {
    /// Takes the [`Database`] handle the composition root already holds and keeps
    /// a SeaORM handle over the SAME pool (ADR-036 §2.1).
    pub fn new(db: Database) -> Self {
        Self { conn: db.sea_orm() }
    }
}

#[async_trait]
impl CleanupAuditWriter for SeaOrmCleanupAuditWriter {
    async fn write(&self, entry: CleanupAuditEntry) -> Result<(), AuditError> {
        let (err_code, err_msg) = match entry.error {
            Some(e) => (Some(e.code), Some(e.message)),
            None => (None, None),
        };
        // `seq as i32` matches the INTEGER/INT4 column the entity declares — the
        // pre-port cast, preserved: plan-side inserts already reject seqs beyond
        // INT4 range, so an out-of-range audit seq is unreachable.
        let row = audit_log::ActiveModel {
            timestamp: Set(entry.timestamp_ms),
            plan_id: Set(entry.plan_id.as_bytes().to_vec()),
            job_id: Set(entry.job_id.as_bytes().to_vec()),
            user_id: Set(entry.user_id.into_bytes()),
            account_id: Set(entry.account_id.into_bytes()),
            seq: Set(entry.seq as i32),
            op_kind: Set(entry.op_kind.to_owned()),
            action_type: Set(entry.action_type),
            source_type: Set(entry.source_type),
            outcome: Set(entry.outcome.as_str().to_owned()),
            skip_reason: Set(entry.skip_reason.map(|r| r.as_str().to_owned())),
            error_code: Set(err_code),
            error_message: Set(err_msg),
            ..Default::default()
        };
        // One `OnConflict … DO NOTHING` for both backends, against the same
        // UNIQUE(plan_id, job_id, seq, outcome) constraint. Formerly SQLite
        // `INSERT OR IGNORE` versus a hand-written PostgreSQL upsert — one of
        // ADR-035's genuinely-different-SQL-text divergence classes its §2.3
        // translation algorithm could not cover, now library-owned.
        //
        // `exec_without_returning` (not `exec`) because a conflicting insert has
        // no row to return: `exec` reports that as `DbErr::RecordNotInserted`,
        // which we would have to swallow — and swallowing it would also swallow
        // unrelated zero-row causes. This variant reports rows-affected instead,
        // so "duplicate write is a silent no-op" needs no error suppression.
        audit_log::Entity::insert(row)
            .on_conflict(
                OnConflict::columns([
                    audit_log::Column::PlanId,
                    audit_log::Column::JobId,
                    audit_log::Column::Seq,
                    audit_log::Column::Outcome,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_without_returning(&self.conn)
            .await?;
        Ok(())
    }

    async fn list_for_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        let rows = audit_log::Entity::find()
            .filter(audit_log::Column::PlanId.eq(plan_id.as_bytes().to_vec()))
            .order_by_asc(audit_log::Column::Timestamp)
            .order_by_asc(audit_log::Column::Seq)
            .all(&self.conn)
            .await?;
        rows.into_iter().map(|r| r.try_into()).collect()
    }

    async fn list_for_user(
        &self,
        user_id: &str,
        limit: u32,
    ) -> Result<Vec<CleanupAuditEntry>, AuditError> {
        let rows = audit_log::Entity::find()
            .filter(audit_log::Column::UserId.eq(user_id.as_bytes().to_vec()))
            .order_by_desc(audit_log::Column::Timestamp)
            .limit(u64::from(limit))
            .all(&self.conn)
            .await?;
        rows.into_iter().map(|r| r.try_into()).collect()
    }
}

impl TryFrom<audit_log::Model> for CleanupAuditEntry {
    type Error = AuditError;

    fn try_from(r: audit_log::Model) -> Result<Self, Self::Error> {
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
    use sqlx::SqlitePool;

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
                    include_str!("../../migrations/sqlite/024_cleanup_planning.sql")
                }
                _ => include_str!("../../migrations/sqlite/025_cleanup_audit_log.sql"),
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
                    sqlx::query(crate::db::audited_sql(s))
                        .execute(&pool)
                        .await
                        .expect("migrate");
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
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
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
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
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
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool.clone()));
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
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
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
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
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

    #[tokio::test]
    async fn audit_list_for_plan_is_scoped_to_its_plan() {
        // The GDPR right-to-explanation surface must not leak one plan's
        // outcomes into another's — the WHERE clause is load-bearing, and a
        // dropped filter would still return rows and still look "green" to
        // every single-plan test above.
        let pool = fresh_pool().await;
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
        let plan_a = Uuid::now_v7();
        let plan_b = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        for seq in 1..=2 {
            writer
                .write(CleanupAuditEntry::from_materialized(
                    plan_a,
                    job_id,
                    "user-a",
                    &sample_row(seq),
                    AuditOutcome::Applied,
                ))
                .await
                .expect("write a");
        }
        writer
            .write(CleanupAuditEntry::from_materialized(
                plan_b,
                job_id,
                "user-b",
                &sample_row(9),
                AuditOutcome::Applied,
            ))
            .await
            .expect("write b");

        let a_rows = writer.list_for_plan(plan_a).await.expect("list a");
        assert_eq!(a_rows.len(), 2);
        assert!(a_rows.iter().all(|r| r.plan_id == plan_a));
        assert_eq!(a_rows.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![1, 2]);

        let b_rows = writer.list_for_plan(plan_b).await.expect("list b");
        assert_eq!(b_rows.len(), 1);
        assert_eq!(b_rows[0].plan_id, plan_b);
        assert_eq!(b_rows[0].seq, 9);
    }

    #[tokio::test]
    async fn audit_list_for_user_is_scoped_newest_first_and_capped() {
        let pool = fresh_pool().await;
        let writer = SeaOrmCleanupAuditWriter::new(Database::Sqlite(pool));
        let plan_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        // Explicit, distinct timestamps: the ORDER BY is on `timestamp`, and
        // three writes in one test can otherwise land on the same millisecond.
        for (seq, ts) in [(1u64, 1_000i64), (2, 2_000), (3, 3_000)] {
            let mut entry = CleanupAuditEntry::from_materialized(
                plan_id,
                job_id,
                "user-mine",
                &sample_row(seq),
                AuditOutcome::Applied,
            );
            entry.timestamp_ms = ts;
            writer.write(entry).await.expect("write mine");
        }
        let mut other = CleanupAuditEntry::from_materialized(
            plan_id,
            job_id,
            "user-theirs",
            &sample_row(4),
            AuditOutcome::Applied,
        );
        other.timestamp_ms = 9_000;
        writer.write(other).await.expect("write theirs");

        let all = writer.list_for_user("user-mine", 100).await.expect("list");
        assert_eq!(all.len(), 3, "another user's rows must not be returned");
        assert!(all.iter().all(|r| r.user_id == "user-mine"));
        assert_eq!(
            all.iter().map(|r| r.timestamp_ms).collect::<Vec<_>>(),
            vec![3_000, 2_000, 1_000],
            "newest first"
        );

        let capped = writer.list_for_user("user-mine", 2).await.expect("list");
        assert_eq!(capped.len(), 2);
        assert_eq!(capped[0].timestamp_ms, 3_000);
    }

    /// Live-PostgreSQL verification of the audit writer (ADR-036 exemplar
    /// proof, matching `plan_repo`/`job_repo`). The load-bearing case is the
    /// idempotent write: `ON CONFLICT … DO NOTHING` against a
    /// `GENERATED ALWAYS AS IDENTITY` primary key is the one construct in this
    /// file with no SQLite-side equivalent to fall back on. Skips (trivially
    /// passing) when `EMAILIBRIUM_TEST_PG_URL` is unset. Reproduction:
    ///
    /// ```sh
    /// docker run -d --rm --name emailibrium-pg-test -p 55434:5432 \
    ///   -e POSTGRES_PASSWORD=test -e POSTGRES_DB=emailibrium_test postgres:16-alpine
    /// EMAILIBRIUM_TEST_PG_URL='postgres://postgres:test@localhost:55434/emailibrium_test' \
    ///   cargo test --features vectors cleanup::audit -- --nocapture
    /// docker rm -f emailibrium-pg-test
    /// ```
    #[tokio::test]
    async fn postgres_audit_round_trip() {
        let Ok(url) = std::env::var("EMAILIBRIUM_TEST_PG_URL") else {
            eprintln!("skipping postgres_audit_round_trip: EMAILIBRIUM_TEST_PG_URL unset");
            return;
        };
        let db = Database::connect(&url).await.expect("pg connect");
        db.run_migrations().await.expect("pg migrations");
        let writer = SeaOrmCleanupAuditWriter::new(db);

        // Unique ids so reruns against a persistent database stay clean.
        let user = format!("pg-audit-user-{}", Uuid::new_v4());
        let plan_a = Uuid::now_v7();
        let plan_b = Uuid::now_v7();
        let job_id = Uuid::now_v7();

        let applied = |plan_id, user_id: &str, seq| {
            CleanupAuditEntry::from_materialized(
                plan_id,
                job_id,
                user_id,
                &sample_row(seq),
                AuditOutcome::Applied,
            )
        };

        for seq in 1..=3 {
            writer
                .write(applied(plan_a, &user, seq))
                .await
                .expect("write");
        }
        // Idempotency: re-writing the same (plan, job, seq, outcome) is a
        // silent no-op, not an error and not a duplicate row.
        writer
            .write(applied(plan_a, &user, 2))
            .await
            .expect("duplicate write must be a silent no-op");
        let rows = writer.list_for_plan(plan_a).await.expect("list a");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(rows[0].user_id, user);
        assert_eq!(rows[0].account_id, "acct-a");

        // Same seq, DIFFERENT outcome is a distinct row (the unique key
        // includes `outcome`).
        let mut failed_row = sample_row(2);
        failed_row.error = Some(ErrorCode {
            code: "x".into(),
            message: "y".into(),
        });
        writer
            .write(CleanupAuditEntry::from_materialized(
                plan_a,
                job_id,
                &user,
                &failed_row,
                AuditOutcome::Failed,
            ))
            .await
            .expect("failed write");
        assert_eq!(writer.list_for_plan(plan_a).await.expect("list a").len(), 4);

        // Plan scoping.
        writer
            .write(applied(plan_b, &user, 9))
            .await
            .expect("write b");
        let b_rows = writer.list_for_plan(plan_b).await.expect("list b");
        assert_eq!(b_rows.len(), 1);
        assert_eq!(b_rows[0].seq, 9);

        // User scoping + cap. This user's rows span both plans (4 + 1).
        let mine = writer.list_for_user(&user, 100).await.expect("list user");
        assert_eq!(mine.len(), 5);
        assert!(mine.iter().all(|r| r.user_id == user));
        let capped = writer.list_for_user(&user, 2).await.expect("list capped");
        assert_eq!(capped.len(), 2);
    }
}
