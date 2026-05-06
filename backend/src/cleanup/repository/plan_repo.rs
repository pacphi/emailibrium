//! `CleanupPlanRepository` trait + `SqliteCleanupPlanRepo` impl.
//!
//! See migration `024_cleanup_planning.sql` for schema. JSON-typed columns are
//! stored as plaintext TEXT (see migration header for the encryption-debt note).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::cleanup::domain::operation::{
    AccountStateEtag, OperationStatus, PlanStatus, PlannedOperation, Provider,
};
use crate::cleanup::domain::plan::{CleanupPlan, CleanupPlanSummary, PlanId};
use crate::cleanup::domain::ports::RepoError;

/// On-disk envelope for the `totals_json` column. Carries `PlanTotals`
/// alongside `account_providers` (Item #4) inside the same TEXT blob to
/// avoid a schema migration. The deserialised side is forgiving:
/// historical rows with a bare `PlanTotals` JSON still load (account_providers
/// defaults to empty).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedTotals<'a> {
    #[serde(flatten)]
    totals: &'a crate::cleanup::domain::plan::PlanTotals,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    account_providers: &'a std::collections::BTreeMap<String, Provider>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedTotalsOwned {
    #[serde(flatten)]
    totals: crate::cleanup::domain::plan::PlanTotals,
    #[serde(default)]
    account_providers: std::collections::BTreeMap<String, Provider>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsFilter {
    pub risk: Option<String>,
    pub action: Option<String>,
    pub account_id: Option<String>,
}

#[async_trait]
pub trait CleanupPlanRepository: Send + Sync {
    async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError>;
    async fn load(&self, user_id: &str, id: PlanId) -> Result<Option<CleanupPlan>, RepoError>;
    async fn list_by_user(
        &self,
        user_id: &str,
        status: Option<PlanStatus>,
        limit: u32,
    ) -> Result<Vec<CleanupPlanSummary>, RepoError>;
    async fn list_operations(
        &self,
        id: PlanId,
        filter: OpsFilter,
        cursor: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<PlannedOperation>, Option<u64>), RepoError>;
    async fn sample_operations(
        &self,
        id: PlanId,
        source_kind: &str,
        n: u32,
    ) -> Result<Vec<String>, RepoError>;
    async fn replace_account_rows(
        &self,
        id: PlanId,
        account_id: &str,
        new_rows: Vec<PlannedOperation>,
    ) -> Result<(), RepoError>;
    /// Append operation rows to an existing plan. Caller is responsible for
    /// assigning seq values that don't collide with existing rows. Used by
    /// the apply-time predicate expander to write materialized children.
    /// Default implementation falls back to `replace_account_rows` is NOT
    /// possible since that's destructive — implementations MUST insert.
    async fn append_operations(
        &self,
        id: PlanId,
        rows: Vec<PlannedOperation>,
    ) -> Result<(), RepoError>;
    /// Highest seq currently stored on the plan. Used to allocate a
    /// reservation block before predicate expansion writes new rows.
    async fn max_seq(&self, id: PlanId) -> Result<u64, RepoError>;
    async fn update_operation_status(
        &self,
        id: PlanId,
        seq: u64,
        status: OperationStatus,
        ts: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Update a predicate row's lifecycle status. Distinct from
    /// `update_operation_status` because predicate rows use
    /// [`PredicateStatus`](crate::cleanup::domain::operation::PredicateStatus),
    /// not `OperationStatus`.
    async fn update_predicate_status(
        &self,
        id: PlanId,
        seq: u64,
        status: crate::cleanup::domain::operation::PredicateStatus,
    ) -> Result<(), RepoError>;
    async fn cancel(&self, id: PlanId) -> Result<(), RepoError>;
    async fn expire_due(&self, now: DateTime<Utc>) -> Result<u32, RepoError>;
    async fn purge_older_than(&self, cutoff: DateTime<Utc>) -> Result<u32, RepoError>;
}

pub struct SqliteCleanupPlanRepo {
    pool: SqlitePool,
}

impl SqliteCleanupPlanRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CleanupPlanRepository for SqliteCleanupPlanRepo {
    async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await?;
        // Plan envelope
        sqlx::query(
            r#"INSERT OR REPLACE INTO cleanup_plans
               (id, user_id, created_at, valid_until, plan_hash, status,
                totals_json, risk_json, warnings_json)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(plan.id.as_bytes().to_vec())
        .bind(plan.user_id.as_bytes().to_vec())
        .bind(plan.created_at.timestamp_millis())
        .bind(plan.valid_until.timestamp_millis())
        .bind(plan.plan_hash.to_vec())
        .bind(plan.status.as_str())
        .bind(
            serde_json::to_string(&PersistedTotals {
                totals: &plan.totals,
                account_providers: &plan.account_providers,
            })
            .map_err(|e| RepoError::Internal(e.to_string()))?,
        )
        .bind(serde_json::to_string(&plan.risk).map_err(|e| RepoError::Internal(e.to_string()))?)
        .bind(
            serde_json::to_string(&plan.warnings)
                .map_err(|e| RepoError::Internal(e.to_string()))?,
        )
        .execute(&mut *tx)
        .await?;

        // Account etags
        sqlx::query("DELETE FROM cleanup_plan_account_etags WHERE plan_id = ?")
            .bind(plan.id.as_bytes().to_vec())
            .execute(&mut *tx)
            .await?;
        for (account_id, etag) in &plan.account_state_etags {
            let kind = etag.kind_str();
            let value =
                serde_json::to_string(etag).map_err(|e| RepoError::Internal(e.to_string()))?;
            sqlx::query(
                r#"INSERT INTO cleanup_plan_account_etags
                   (plan_id, account_id, etag_kind, etag_value)
                   VALUES (?, ?, ?, ?)"#,
            )
            .bind(plan.id.as_bytes().to_vec())
            .bind(account_id.as_bytes().to_vec())
            .bind(kind)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }

        // Operations
        sqlx::query("DELETE FROM cleanup_plan_operations WHERE plan_id = ?")
            .bind(plan.id.as_bytes().to_vec())
            .execute(&mut *tx)
            .await?;
        for op in &plan.operations {
            insert_operation(&mut tx, plan.id, op).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn load(&self, user_id: &str, id: PlanId) -> Result<Option<CleanupPlan>, RepoError> {
        // Phase A: simplified loader — only the envelope + ops in seq order.
        // Phase C will optimise.
        let row: Option<(Vec<u8>, i64, i64, Vec<u8>, String, String, String, String)> =
            sqlx::query_as(
                r#"SELECT user_id, created_at, valid_until, plan_hash, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans WHERE id = ? AND user_id = ?"#,
            )
            .bind(id.as_bytes().to_vec())
            .bind(user_id.as_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await?;
        let Some((_uid, created_ms, valid_ms, hash_bytes, status_s, totals_s, risk_s, warn_s)) =
            row
        else {
            return Ok(None);
        };

        let mut plan_hash = [0u8; 32];
        if hash_bytes.len() == 32 {
            plan_hash.copy_from_slice(&hash_bytes);
        }

        let status = PlanStatus::from_str_opt(&status_s)
            .ok_or_else(|| RepoError::Internal(format!("bad plan status: {status_s}")))?;
        let persisted: PersistedTotalsOwned =
            serde_json::from_str(&totals_s).map_err(|e| RepoError::Internal(e.to_string()))?;
        let totals = persisted.totals;
        let account_providers = persisted.account_providers;
        let risk = serde_json::from_str(&risk_s).map_err(|e| RepoError::Internal(e.to_string()))?;
        let warnings =
            serde_json::from_str(&warn_s).map_err(|e| RepoError::Internal(e.to_string()))?;

        // Etags
        let etag_rows: Vec<(Vec<u8>, String, Option<String>)> = sqlx::query_as(
            r#"SELECT account_id, etag_kind, etag_value
               FROM cleanup_plan_account_etags WHERE plan_id = ?"#,
        )
        .bind(id.as_bytes().to_vec())
        .fetch_all(&self.pool)
        .await?;
        let mut etags = std::collections::BTreeMap::new();
        for (acct_b, _kind, val_opt) in etag_rows {
            let acct = String::from_utf8(acct_b).unwrap_or_default();
            let etag: AccountStateEtag = match val_opt {
                Some(s) => {
                    serde_json::from_str(&s).map_err(|e| RepoError::Internal(e.to_string()))?
                }
                None => AccountStateEtag::None,
            };
            etags.insert(acct, etag);
        }

        // Operations (full list; Phase B will paginate)
        let (operations, _) = self
            .list_operations(id, OpsFilter::default(), None, u32::MAX)
            .await?;

        let account_ids: Vec<String> = etags.keys().cloned().collect();

        Ok(Some(CleanupPlan {
            id,
            user_id: user_id.to_string(),
            account_ids,
            created_at: DateTime::from_timestamp_millis(created_ms).unwrap_or_else(Utc::now),
            valid_until: DateTime::from_timestamp_millis(valid_ms).unwrap_or_else(Utc::now),
            plan_hash,
            account_state_etags: etags,
            account_providers,
            status,
            totals,
            risk,
            warnings,
            operations,
        }))
    }

    async fn list_by_user(
        &self,
        user_id: &str,
        status: Option<PlanStatus>,
        limit: u32,
    ) -> Result<Vec<CleanupPlanSummary>, RepoError> {
        let limit = limit.clamp(1, 100) as i64;
        let rows: Vec<(Vec<u8>, i64, i64, String, String, String, String)> = match status {
            Some(s) => {
                sqlx::query_as(
                    r#"SELECT id, created_at, valid_until, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans
                   WHERE user_id = ? AND status = ?
                   ORDER BY created_at DESC LIMIT ?"#,
                )
                .bind(user_id.as_bytes().to_vec())
                .bind(s.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as(
                    r#"SELECT id, created_at, valid_until, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans
                   WHERE user_id = ?
                   ORDER BY created_at DESC LIMIT ?"#,
                )
                .bind(user_id.as_bytes().to_vec())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };
        let mut out = Vec::with_capacity(rows.len());
        for (id_b, created, valid, status_s, totals_s, risk_s, warn_s) in rows {
            let id =
                uuid::Uuid::from_slice(&id_b).map_err(|e| RepoError::Internal(e.to_string()))?;
            let totals = serde_json::from_str::<PersistedTotalsOwned>(&totals_s)
                .map_err(|e| RepoError::Internal(e.to_string()))?
                .totals;
            let risk =
                serde_json::from_str(&risk_s).map_err(|e| RepoError::Internal(e.to_string()))?;
            let warnings: Vec<serde_json::Value> =
                serde_json::from_str(&warn_s).map_err(|e| RepoError::Internal(e.to_string()))?;
            out.push(CleanupPlanSummary {
                id,
                created_at: DateTime::from_timestamp_millis(created).unwrap_or_else(Utc::now),
                valid_until: DateTime::from_timestamp_millis(valid).unwrap_or_else(Utc::now),
                status: PlanStatus::from_str_opt(&status_s)
                    .ok_or_else(|| RepoError::Internal(format!("bad status: {status_s}")))?,
                totals,
                risk,
                warnings_count: warnings.len() as u64,
            });
        }
        Ok(out)
    }

    async fn list_operations(
        &self,
        id: PlanId,
        filter: OpsFilter,
        cursor: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<PlannedOperation>, Option<u64>), RepoError> {
        let limit = limit.clamp(1, 1000) as i64;
        let cursor_i = cursor.map(|c| c as i64).unwrap_or(0);

        // Build the SQL filter clauses dynamically based on which filters are set.
        // SQLite supports `? IS NULL OR col = ?` style but sqlx requires explicit
        // parameter binding; dynamic SQL is cleaner and avoids double-binding.
        let mut sql = String::from(
            "SELECT seq, COALESCE(payload_json, '') AS payload \
             FROM cleanup_plan_operations \
             WHERE plan_id = ? AND seq > ?",
        );
        if filter.account_id.is_some() {
            sql.push_str(" AND account_id = ?");
        }
        if filter.risk.is_some() {
            sql.push_str(" AND risk = ?");
        }
        if filter.action.is_some() {
            // action column stores the PlanAction discriminant string.
            sql.push_str(" AND action = ?");
        }
        sql.push_str(" ORDER BY seq ASC LIMIT ?");

        let mut q = sqlx::query(&sql)
            .bind(id.as_bytes().to_vec())
            .bind(cursor_i);
        if let Some(ref a) = filter.account_id {
            q = q.bind(a);
        }
        if let Some(ref r) = filter.risk {
            q = q.bind(r);
        }
        if let Some(ref act) = filter.action {
            q = q.bind(act);
        }
        q = q.bind(limit);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .or_else(|_| Ok::<_, RepoError>(Vec::new()))?;

        let mut ops = Vec::with_capacity(rows.len());
        let mut last_seq: Option<u64> = cursor;
        for r in &rows {
            let seq: i64 = r.get("seq");
            let payload: String = r.get("payload");
            if payload.is_empty() {
                continue;
            }
            if let Ok(op) = serde_json::from_str::<PlannedOperation>(&payload) {
                ops.push(op);
            }
            last_seq = Some(seq as u64);
        }
        Ok((ops, last_seq))
    }

    async fn sample_operations(
        &self,
        id: PlanId,
        source_kind: &str,
        n: u32,
    ) -> Result<Vec<String>, RepoError> {
        let row = sqlx::query(
            r#"SELECT sample_ids_json
               FROM cleanup_plan_operations
               WHERE plan_id = ? AND op_kind = 'predicate' AND source_kind = ?
               LIMIT 1"#,
        )
        .bind(id.as_bytes().to_vec())
        .bind(source_kind)
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = row else {
            return Ok(Vec::new());
        };

        let json_s: Option<String> = r.get("sample_ids_json");
        let all_ids: Vec<String> = json_s
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        Ok(all_ids.into_iter().take(n as usize).collect())
    }

    async fn replace_account_rows(
        &self,
        id: PlanId,
        account_id: &str,
        new_rows: Vec<PlannedOperation>,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"DELETE FROM cleanup_plan_operations
               WHERE plan_id = ? AND account_id = ?"#,
        )
        .bind(id.as_bytes().to_vec())
        .bind(account_id.as_bytes().to_vec())
        .execute(&mut *tx)
        .await?;
        for op in &new_rows {
            insert_operation(&mut tx, id, op).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn update_operation_status(
        &self,
        id: PlanId,
        seq: u64,
        status: OperationStatus,
        ts: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            r#"UPDATE cleanup_plan_operations
               SET status = ?, applied_at = ?
               WHERE plan_id = ? AND seq = ?"#,
        )
        .bind(status.as_str())
        .bind(ts.timestamp_millis())
        .bind(id.as_bytes().to_vec())
        .bind(seq as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_predicate_status(
        &self,
        id: PlanId,
        seq: u64,
        status: crate::cleanup::domain::operation::PredicateStatus,
    ) -> Result<(), RepoError> {
        sqlx::query(
            r#"UPDATE cleanup_plan_operations
               SET status = ?
               WHERE plan_id = ? AND seq = ? AND op_kind = 'predicate'"#,
        )
        .bind(status.as_str())
        .bind(id.as_bytes().to_vec())
        .bind(seq as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn append_operations(
        &self,
        id: PlanId,
        rows: Vec<PlannedOperation>,
    ) -> Result<(), RepoError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for op in &rows {
            insert_operation(&mut tx, id, op).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn max_seq(&self, id: PlanId) -> Result<u64, RepoError> {
        let row: Option<(Option<i64>,)> =
            sqlx::query_as(r#"SELECT MAX(seq) FROM cleanup_plan_operations WHERE plan_id = ?"#)
                .bind(id.as_bytes().to_vec())
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(m,)| m).map(|v| v.max(0) as u64).unwrap_or(0))
    }

    async fn cancel(&self, id: PlanId) -> Result<(), RepoError> {
        sqlx::query("UPDATE cleanup_plans SET status = 'cancelled' WHERE id = ?")
            .bind(id.as_bytes().to_vec())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<u32, RepoError> {
        let r = sqlx::query(
            r#"UPDATE cleanup_plans SET status = 'expired'
               WHERE valid_until < ? AND status IN ('ready', 'draft')"#,
        )
        .bind(now.timestamp_millis())
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() as u32)
    }

    async fn purge_older_than(&self, cutoff: DateTime<Utc>) -> Result<u32, RepoError> {
        let r = sqlx::query("DELETE FROM cleanup_plans WHERE valid_until < ?")
            .bind(cutoff.timestamp_millis())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() as u32)
    }
}

async fn insert_operation(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan_id: PlanId,
    op: &PlannedOperation,
) -> Result<(), RepoError> {
    let payload = serde_json::to_string(op).map_err(|e| RepoError::Internal(e.to_string()))?;
    let (seq, op_kind, account_id, email_id_opt, risk_s, status_s, action_s, source_kind) = match op
    {
        PlannedOperation::Materialized(r) => (
            r.seq as i64,
            "materialized",
            r.account_id.clone(),
            r.email_id.clone(),
            r.risk.as_str().to_string(),
            r.status.as_str().to_string(),
            serde_json::to_string(&r.action).unwrap_or_default(),
            source_kind_str(&r.source),
        ),
        PlannedOperation::Predicate(p) => (
            p.seq as i64,
            "predicate",
            p.account_id.clone(),
            None,
            p.risk.as_str().to_string(),
            p.status.as_str().to_string(),
            serde_json::to_string(&p.action).unwrap_or_default(),
            source_kind_str(&p.source),
        ),
    };
    sqlx::query(
        r#"INSERT INTO cleanup_plan_operations
           (plan_id, seq, op_kind, account_id, email_id, action, source_kind,
            risk, status, payload_json)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(plan_id.as_bytes().to_vec())
    .bind(seq)
    .bind(op_kind)
    .bind(account_id.as_bytes().to_vec())
    .bind(email_id_opt.map(|s| s.as_bytes().to_vec()))
    .bind(action_s)
    .bind(source_kind)
    .bind(risk_s)
    .bind(status_s)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn source_kind_str(s: &crate::cleanup::domain::operation::PlanSource) -> &'static str {
    use crate::cleanup::domain::operation::PlanSource as S;
    match s {
        S::Subscription { .. } => "subscription",
        S::Cluster { .. } => "cluster",
        S::Rule { .. } => "rule",
        S::ArchiveStrategy { .. } => "strategy",
        S::Manual => "manual",
    }
}

// ---------------------------------------------------------------------------
// Integration test: repo round-trip against an in-memory SQLite pool
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::operation::{
        AccountStateEtag, OperationStatus, PlanAction, PlanSource, PlanStatus, PlannedOperation,
        PlannedOperationRow, Provider, RiskLevel,
    };
    use crate::cleanup::domain::plan::{CleanupPlan, PlanTotals, RiskRollup};
    use chrono::{Duration, Utc};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let raw = include_str!("../../../migrations/024_cleanup_planning.sql");
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
                sqlx::query(s).execute(&pool).await.expect("migrate");
            }
        }
        pool
    }

    fn sample_plan(user_id: &str) -> CleanupPlan {
        let now = Utc::now();
        let mut etags = std::collections::BTreeMap::new();
        etags.insert(
            "acct-a".to_string(),
            AccountStateEtag::GmailHistory {
                history_id: "100".into(),
            },
        );
        let op = PlannedOperation::Materialized(PlannedOperationRow {
            seq: 1,
            account_id: "acct-a".into(),
            email_id: Some("e1".into()),
            action: PlanAction::Archive,
            source: PlanSource::Manual,
            target: None,
            reverse_op: None,
            risk: RiskLevel::Low,
            status: OperationStatus::Pending,
            skip_reason: None,
            applied_at: None,
            error: None,
        });
        CleanupPlan {
            id: Uuid::now_v7(),
            user_id: user_id.into(),
            account_ids: vec!["acct-a".into()],
            created_at: now,
            valid_until: now + Duration::minutes(30),
            plan_hash: [0u8; 32],
            account_state_etags: etags,
            account_providers: std::collections::BTreeMap::new(),
            status: PlanStatus::Ready,
            totals: PlanTotals::default(),
            risk: RiskRollup {
                low: 1,
                medium: 0,
                high: 0,
            },
            warnings: vec![],
            operations: vec![op],
        }
    }

    #[tokio::test]
    async fn plan_envelope_carries_per_account_provider() {
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-providers";
        let mut plan = sample_plan(user);
        plan.account_providers
            .insert("acct-a".into(), Provider::Outlook);
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");
        let loaded = repo.load(user, plan_id).await.expect("load").unwrap();
        assert_eq!(loaded.account_providers.len(), 1);
        assert_eq!(
            loaded.account_providers.get("acct-a"),
            Some(&Provider::Outlook)
        );
    }

    #[tokio::test]
    async fn save_load_round_trip() {
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-1";
        let plan = sample_plan(user);
        let plan_id = plan.id;

        repo.save(&plan).await.expect("save");
        let loaded = repo
            .load(user, plan_id)
            .await
            .expect("load")
            .expect("present");
        assert_eq!(loaded.id, plan_id);
        assert_eq!(loaded.user_id, user);
        assert_eq!(loaded.status, PlanStatus::Ready);
        assert_eq!(loaded.operations.len(), 1);
        assert_eq!(loaded.account_state_etags.len(), 1);
    }

    #[tokio::test]
    async fn list_by_user_filters_by_status() {
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-2";
        let p1 = sample_plan(user);
        let p2 = sample_plan(user);
        repo.save(&p1).await.expect("s1");
        repo.save(&p2).await.expect("s2");

        let summaries = repo
            .list_by_user(user, Some(PlanStatus::Ready), 100)
            .await
            .expect("list");
        assert_eq!(summaries.len(), 2);
        let none = repo
            .list_by_user(user, Some(PlanStatus::Applied), 100)
            .await
            .expect("list");
        assert_eq!(none.len(), 0);
    }

    #[tokio::test]
    async fn cancel_transitions_status() {
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-3";
        let plan = sample_plan(user);
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");
        repo.cancel(plan_id).await.expect("cancel");
        let loaded = repo.load(user, plan_id).await.expect("load").unwrap();
        assert_eq!(loaded.status, PlanStatus::Cancelled);
    }

    #[tokio::test]
    async fn list_operations_paginates_by_seq() {
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-4";
        let mut plan = sample_plan(user);
        // Add a second op with seq=2.
        plan.operations
            .push(PlannedOperation::Materialized(PlannedOperationRow {
                seq: 2,
                account_id: "acct-a".into(),
                email_id: Some("e2".into()),
                action: PlanAction::Archive,
                source: PlanSource::Manual,
                target: None,
                reverse_op: None,
                risk: RiskLevel::Low,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            }));
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        let (page1, cursor1) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 1)
            .await
            .expect("list");
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0].seq(), 1);
        assert_eq!(cursor1, Some(1));

        let (page2, _cursor2) = repo
            .list_operations(plan_id, OpsFilter::default(), cursor1, 10)
            .await
            .expect("list");
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].seq(), 2);
    }
}
