//! `CleanupPlanRepository` trait + `SeaOrmCleanupPlanRepo` impl.
//!
//! Repository bodies are single-code-path SeaORM (ADR-036): the entities in
//! `crate::db::entities` own per-backend encode/decode, upserts go through
//! `OnConflict`, transactions through `TransactionTrait`, and the former SQL-side
//! SQL JSON-mutation payload sync is a read-modify-write inside a transaction
//! (ADR-036 §2.4). There is no per-backend dispatch here — the same bodies run
//! against SQLite and PostgreSQL.
//!
//! See migration `024_cleanup_planning.sql` for schema. JSON-typed columns are
//! stored as plaintext TEXT (see migration header for the encryption-debt note).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::cleanup::domain::operation::{
    AccountStateEtag, OperationStatus, PlanStatus, PlannedOperation, Provider,
};
use crate::cleanup::domain::plan::{CleanupPlan, CleanupPlanSummary, PlanId};
use crate::cleanup::domain::ports::RepoError;
use crate::db::entities::{
    cleanup_plan_account_etags as etags, cleanup_plan_operations as ops, cleanup_plans as plans,
};

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

pub struct SeaOrmCleanupPlanRepo {
    conn: DatabaseConnection,
}

impl SeaOrmCleanupPlanRepo {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CleanupPlanRepository for SeaOrmCleanupPlanRepo {
    async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError> {
        let totals_json = serde_json::to_string(&PersistedTotals {
            totals: &plan.totals,
            account_providers: &plan.account_providers,
        })
        .map_err(|e| RepoError::Internal(e.to_string()))?;
        let risk_json =
            serde_json::to_string(&plan.risk).map_err(|e| RepoError::Internal(e.to_string()))?;
        let warnings_json = serde_json::to_string(&plan.warnings)
            .map_err(|e| RepoError::Internal(e.to_string()))?;

        let txn = self.conn.begin().await?;

        // Upsert the envelope: `ON CONFLICT (id) DO UPDATE`, one code path for
        // both backends (formerly SQLite `INSERT OR REPLACE` + a hand-written
        // PostgreSQL upsert — ADR-035 §2.3's divergence class, now library-owned).
        let envelope = plans::ActiveModel {
            id: Set(plan.id.as_bytes().to_vec()),
            user_id: Set(plan.user_id.as_bytes().to_vec()),
            created_at: Set(plan.created_at.timestamp_millis()),
            valid_until: Set(plan.valid_until.timestamp_millis()),
            plan_hash: Set(plan.plan_hash.to_vec()),
            status: Set(plan.status.as_str().to_owned()),
            totals_json: Set(totals_json),
            risk_json: Set(risk_json),
            warnings_json: Set(warnings_json),
        };
        plans::Entity::insert(envelope)
            .on_conflict(
                OnConflict::column(plans::Column::Id)
                    .update_columns([
                        plans::Column::UserId,
                        plans::Column::CreatedAt,
                        plans::Column::ValidUntil,
                        plans::Column::PlanHash,
                        plans::Column::Status,
                        plans::Column::TotalsJson,
                        plans::Column::RiskJson,
                        plans::Column::WarningsJson,
                    ])
                    .to_owned(),
            )
            .exec(&txn)
            .await?;

        etags::Entity::delete_many()
            .filter(etags::Column::PlanId.eq(plan.id.as_bytes().to_vec()))
            .exec(&txn)
            .await?;
        for (account_id, etag) in &plan.account_state_etags {
            let value =
                serde_json::to_string(etag).map_err(|e| RepoError::Internal(e.to_string()))?;
            etags::ActiveModel {
                plan_id: Set(plan.id.as_bytes().to_vec()),
                account_id: Set(account_id.as_bytes().to_vec()),
                etag_kind: Set(etag.kind_str().to_owned()),
                etag_value: Set(Some(value)),
            }
            .insert(&txn)
            .await?;
        }

        ops::Entity::delete_many()
            .filter(ops::Column::PlanId.eq(plan.id.as_bytes().to_vec()))
            .exec(&txn)
            .await?;
        for op in &plan.operations {
            insert_operation(&txn, plan.id, op).await?;
        }

        txn.commit().await?;
        Ok(())
    }

    async fn load(&self, user_id: &str, id: PlanId) -> Result<Option<CleanupPlan>, RepoError> {
        // Phase A: simplified loader — only the envelope + ops in seq order.
        // Phase C will optimise.
        let row = plans::Entity::find_by_id(id.as_bytes().to_vec())
            .filter(plans::Column::UserId.eq(user_id.as_bytes().to_vec()))
            .one(&self.conn)
            .await?;
        let Some(envelope) = row else {
            return Ok(None);
        };

        let mut plan_hash = [0u8; 32];
        if envelope.plan_hash.len() == 32 {
            plan_hash.copy_from_slice(&envelope.plan_hash);
        }

        let status = PlanStatus::from_str_opt(&envelope.status)
            .ok_or_else(|| RepoError::Internal(format!("bad plan status: {}", envelope.status)))?;
        let persisted: PersistedTotalsOwned = serde_json::from_str(&envelope.totals_json)
            .map_err(|e| RepoError::Internal(e.to_string()))?;
        let totals = persisted.totals;
        let account_providers = persisted.account_providers;
        let risk = serde_json::from_str(&envelope.risk_json)
            .map_err(|e| RepoError::Internal(e.to_string()))?;
        let warnings = serde_json::from_str(&envelope.warnings_json)
            .map_err(|e| RepoError::Internal(e.to_string()))?;

        // Etags
        let etag_rows = etags::Entity::find()
            .filter(etags::Column::PlanId.eq(id.as_bytes().to_vec()))
            .all(&self.conn)
            .await?;
        let mut account_state_etags = std::collections::BTreeMap::new();
        for row in etag_rows {
            let acct = String::from_utf8(row.account_id).unwrap_or_default();
            let etag: AccountStateEtag = match row.etag_value {
                Some(s) => {
                    serde_json::from_str(&s).map_err(|e| RepoError::Internal(e.to_string()))?
                }
                None => AccountStateEtag::None,
            };
            account_state_etags.insert(acct, etag);
        }

        // Operations (full list; Phase B will paginate)
        let (operations, _) = self
            .list_operations(id, OpsFilter::default(), None, u32::MAX)
            .await?;

        let account_ids: Vec<String> = account_state_etags.keys().cloned().collect();

        Ok(Some(CleanupPlan {
            id,
            user_id: user_id.to_string(),
            account_ids,
            created_at: DateTime::from_timestamp_millis(envelope.created_at)
                .unwrap_or_else(Utc::now),
            valid_until: DateTime::from_timestamp_millis(envelope.valid_until)
                .unwrap_or_else(Utc::now),
            plan_hash,
            account_state_etags,
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
        let limit = u64::from(limit.clamp(1, 100));
        let mut query =
            plans::Entity::find().filter(plans::Column::UserId.eq(user_id.as_bytes().to_vec()));
        if let Some(s) = status {
            query = query.filter(plans::Column::Status.eq(s.as_str()));
        }
        let rows = query
            .order_by_desc(plans::Column::CreatedAt)
            .limit(limit)
            .all(&self.conn)
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id =
                uuid::Uuid::from_slice(&row.id).map_err(|e| RepoError::Internal(e.to_string()))?;
            let totals = serde_json::from_str::<PersistedTotalsOwned>(&row.totals_json)
                .map_err(|e| RepoError::Internal(e.to_string()))?
                .totals;
            let risk = serde_json::from_str(&row.risk_json)
                .map_err(|e| RepoError::Internal(e.to_string()))?;
            let warnings: Vec<serde_json::Value> = serde_json::from_str(&row.warnings_json)
                .map_err(|e| RepoError::Internal(e.to_string()))?;
            out.push(CleanupPlanSummary {
                id,
                created_at: DateTime::from_timestamp_millis(row.created_at)
                    .unwrap_or_else(Utc::now),
                valid_until: DateTime::from_timestamp_millis(row.valid_until)
                    .unwrap_or_else(Utc::now),
                status: PlanStatus::from_str_opt(&row.status)
                    .ok_or_else(|| RepoError::Internal(format!("bad status: {}", row.status)))?,
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
        let limit = u64::from(limit.clamp(1, 1000));
        // The seq column is INTEGER/INT4; a cursor beyond i32 can match nothing,
        // which saturating to i32::MAX preserves.
        let cursor_seq = cursor
            .map(|c| i32::try_from(c).unwrap_or(i32::MAX))
            .unwrap_or(0);

        // Optional filters compose on the typed query builder — the dynamic-SQL
        // string assembly (and its placeholder bookkeeping) is gone.
        let mut query = ops::Entity::find()
            .filter(ops::Column::PlanId.eq(id.as_bytes().to_vec()))
            .filter(ops::Column::Seq.gt(cursor_seq));
        if let Some(ref account) = filter.account_id {
            // account_id is stored as BLOB/BYTEA (bytes); compare as bytes so the
            // filter matches the byte column, not a TEXT one.
            query = query.filter(ops::Column::AccountId.eq(account.as_bytes().to_vec()));
        }
        if let Some(ref risk) = filter.risk {
            query = query.filter(ops::Column::Risk.eq(risk.as_str()));
        }
        if let Some(ref action) = filter.action {
            // action column stores the JSON-serialized PlanAction.
            query = query.filter(ops::Column::Action.eq(action.as_str()));
        }
        let rows = query
            .order_by_asc(ops::Column::Seq)
            .limit(limit)
            .all(&self.conn)
            .await?;

        let mut operations = Vec::with_capacity(rows.len());
        let mut last_seq: Option<u64> = cursor;
        for row in rows {
            let payload = row.payload_json.unwrap_or_default();
            if payload.is_empty() {
                continue;
            }
            if let Ok(op) = serde_json::from_str::<PlannedOperation>(&payload) {
                operations.push(op);
            }
            last_seq = Some(row.seq as u64);
        }
        Ok((operations, last_seq))
    }

    async fn sample_operations(
        &self,
        id: PlanId,
        source_kind: &str,
        n: u32,
    ) -> Result<Vec<String>, RepoError> {
        let row = ops::Entity::find()
            .filter(ops::Column::PlanId.eq(id.as_bytes().to_vec()))
            .filter(ops::Column::OpKind.eq("predicate"))
            .filter(ops::Column::SourceKind.eq(source_kind))
            .one(&self.conn)
            .await?;

        let Some(row) = row else {
            return Ok(Vec::new());
        };
        let all_ids: Vec<String> = row
            .sample_ids_json
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
        let txn = self.conn.begin().await?;
        ops::Entity::delete_many()
            .filter(ops::Column::PlanId.eq(id.as_bytes().to_vec()))
            .filter(ops::Column::AccountId.eq(account_id.as_bytes().to_vec()))
            .exec(&txn)
            .await?;
        for op in &new_rows {
            insert_operation(&txn, id, op).await?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn update_operation_status(
        &self,
        id: PlanId,
        seq: u64,
        status: OperationStatus,
        ts: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        // A seq beyond i32 cannot exist in the INTEGER/INT4 column — nothing to
        // update, mirroring the old UPDATE's zero-rows-affected no-op.
        let Ok(seq) = i32::try_from(seq) else {
            return Ok(());
        };
        // Keep payload_json's top-level `status` in sync so list_operations
        // returns current status without a separate column merge. Formerly the
        // backends' SQL JSON-mutation functions with per-backend SQL text; now
        // a read-modify-write in one transaction, one code path (ADR-036 §2.4).
        //
        // CONCURRENCY INVARIANT (ADR-036 §2.4): unlike the single UPDATE it
        // replaced, this read-modify-write has a lost-update window between the
        // SELECT and the UPDATE. It is safe because each (plan_id, seq) row has
        // exactly ONE writer — the apply orchestrator spawns one worker per
        // account and a row's account owns its seq (see orchestrator/apply.rs).
        // A future call site with concurrent same-row writers must add a row
        // lock (`lock_exclusive()` — SELECT … FOR UPDATE on PostgreSQL) first.
        let txn = self.conn.begin().await?;
        let Some(row) = ops::Entity::find_by_id((id.as_bytes().to_vec(), seq))
            .one(&txn)
            .await?
        else {
            txn.commit().await?;
            return Ok(());
        };
        let payload = payload_with_status(row.payload_json.as_deref(), status.as_str())?;
        let mut active: ops::ActiveModel = row.into();
        active.status = Set(status.as_str().to_owned());
        active.applied_at = Set(Some(ts.timestamp_millis()));
        active.payload_json = Set(payload);
        active.update(&txn).await?;
        txn.commit().await?;
        Ok(())
    }

    async fn update_predicate_status(
        &self,
        id: PlanId,
        seq: u64,
        status: crate::cleanup::domain::operation::PredicateStatus,
    ) -> Result<(), RepoError> {
        let Ok(seq) = i32::try_from(seq) else {
            return Ok(());
        };
        // Same single-writer-per-row concurrency invariant as
        // update_operation_status above (ADR-036 §2.4).
        let txn = self.conn.begin().await?;
        let Some(row) = ops::Entity::find_by_id((id.as_bytes().to_vec(), seq))
            .one(&txn)
            .await?
        else {
            txn.commit().await?;
            return Ok(());
        };
        // The old SQL's `AND op_kind = 'predicate'` guard: a materialized row is
        // silently left untouched.
        if row.op_kind != "predicate" {
            txn.commit().await?;
            return Ok(());
        }
        // `status.as_str()` goes into the payload verbatim, exactly as the old
        // SQL-side sync did — including "partially_applied", which diverges from
        // the camelCase the payload's serde round-trip expects. Pre-existing
        // behavior, recorded in the phase parking-lot, not silently changed here.
        let payload = payload_with_status(row.payload_json.as_deref(), status.as_str())?;
        let mut active: ops::ActiveModel = row.into();
        active.status = Set(status.as_str().to_owned());
        active.payload_json = Set(payload);
        active.update(&txn).await?;
        txn.commit().await?;
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
        let txn = self.conn.begin().await?;
        for op in &rows {
            insert_operation(&txn, id, op).await?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn max_seq(&self, id: PlanId) -> Result<u64, RepoError> {
        // MAX() over the INTEGER/INT4 seq column decodes as i32 on both backends
        // — the entity's declared width, not a hand-picked one (ADR-035's width
        // class, now library-owned).
        #[derive(FromQueryResult)]
        struct MaxSeqRow {
            max_seq: Option<i32>,
        }
        let row = ops::Entity::find()
            .select_only()
            .column_as(ops::Column::Seq.max(), "max_seq")
            .filter(ops::Column::PlanId.eq(id.as_bytes().to_vec()))
            .into_model::<MaxSeqRow>()
            .one(&self.conn)
            .await?;
        Ok(row
            .and_then(|r| r.max_seq)
            .map(|v| v.max(0) as u64)
            .unwrap_or(0))
    }

    async fn cancel(&self, id: PlanId) -> Result<(), RepoError> {
        plans::Entity::update_many()
            .col_expr(plans::Column::Status, Expr::value("cancelled"))
            .filter(plans::Column::Id.eq(id.as_bytes().to_vec()))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<u32, RepoError> {
        let res = plans::Entity::update_many()
            .col_expr(plans::Column::Status, Expr::value("expired"))
            .filter(plans::Column::ValidUntil.lt(now.timestamp_millis()))
            .filter(plans::Column::Status.is_in(["ready", "draft"]))
            .exec(&self.conn)
            .await?;
        Ok(res.rows_affected as u32)
    }

    async fn purge_older_than(&self, cutoff: DateTime<Utc>) -> Result<u32, RepoError> {
        let res = plans::Entity::delete_many()
            .filter(plans::Column::ValidUntil.lt(cutoff.timestamp_millis()))
            .exec(&self.conn)
            .await?;
        Ok(res.rows_affected as u32)
    }
}

/// Sync the top-level `status` key inside a serialized payload JSON object — the
/// read-modify-write replacement for the backends' SQL JSON-mutation functions
/// (ADR-036 §2.4). `None` payloads stay `None` (as the SQL sync of a NULL
/// payload did); non-object JSON is left unchanged; malformed JSON is an error
/// (as the SQL functions would have raised).
fn payload_with_status(payload: Option<&str>, status: &str) -> Result<Option<String>, RepoError> {
    let Some(raw) = payload else {
        return Ok(None);
    };
    let mut value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| RepoError::Internal(format!("malformed payload_json: {e}")))?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "status".to_owned(),
            serde_json::Value::String(status.to_owned()),
        );
    }
    Ok(Some(value.to_string()))
}

/// Insert one operation row. Generic over [`ConnectionTrait`] so this logic —
/// parsing a [`PlannedOperation`] into columns — is written once and runs
/// against a live connection or an open transaction, on either backend
/// (ADR-036 spike Q11).
async fn insert_operation<C: ConnectionTrait>(
    conn: &C,
    plan_id: PlanId,
    op: &PlannedOperation,
) -> Result<(), RepoError> {
    let payload = serde_json::to_string(op).map_err(|e| RepoError::Internal(e.to_string()))?;
    // Checked, not `as`: the column is INTEGER/INT4, and silently truncating an
    // out-of-range seq on INSERT would corrupt ordering — erroring here matches
    // what PostgreSQL itself would do (same discipline as the read paths'
    // `i32::try_from`, which no-op instead because nothing can match).
    let seq = i32::try_from(op.seq())
        .map_err(|_| RepoError::Internal(format!("op seq {} exceeds INT4 range", op.seq())))?;
    let (op_kind, account_id, email_id_opt, risk_s, status_s, action_s, source_kind) = match op {
        PlannedOperation::Materialized(r) => (
            "materialized",
            r.account_id.clone(),
            r.email_id.clone(),
            r.risk.as_str().to_string(),
            r.status.as_str().to_string(),
            serde_json::to_string(&r.action).unwrap_or_default(),
            source_kind_str(&r.source),
        ),
        PlannedOperation::Predicate(p) => (
            "predicate",
            p.account_id.clone(),
            None,
            p.risk.as_str().to_string(),
            p.status.as_str().to_string(),
            serde_json::to_string(&p.action).unwrap_or_default(),
            source_kind_str(&p.source),
        ),
    };
    ops::ActiveModel {
        plan_id: Set(plan_id.as_bytes().to_vec()),
        seq: Set(seq),
        op_kind: Set(op_kind.to_owned()),
        account_id: Set(account_id.as_bytes().to_vec()),
        email_id: Set(email_id_opt.map(|s| s.as_bytes().to_vec())),
        action: Set(action_s),
        source_kind: Set(source_kind.to_owned()),
        risk: Set(risk_s),
        status: Set(status_s),
        payload_json: Set(Some(payload)),
        ..Default::default()
    }
    .insert(conn)
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
// Integration tests: repo round-trip against an in-memory SQLite pool, plus an
// env-gated full-trait round trip against live PostgreSQL.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::operation::{
        AccountStateEtag, OperationStatus, PlanAction, PlanSource, PlanStatus, PlannedOperation,
        PlannedOperationPredicate, PlannedOperationRow, PredicateKind, PredicateStatus, Provider,
        RiskLevel,
    };
    use crate::cleanup::domain::plan::{CleanupPlan, PlanTotals, RiskRollup};
    use crate::db::Database;
    use chrono::{Duration, Utc};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn fresh_conn() -> DatabaseConnection {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let raw = include_str!("../../../migrations/sqlite/024_cleanup_planning.sql");
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
                sqlx::query(crate::db::audited_sql(s))
                    .execute(&pool)
                    .await
                    .expect("migrate");
            }
        }
        Database::Sqlite(pool).sea_orm()
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
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
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
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
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
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
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
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
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
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
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

    // -----------------------------------------------------------------------
    // Behavior pins for the SeaORM port (ADR-036): these were written against
    // the hand-rolled implementation and must stay green unchanged across the
    // re-port — they ARE the equivalence contract.
    // -----------------------------------------------------------------------

    fn sample_predicate(seq: u64, sample_ids: Vec<String>) -> PlannedOperation {
        PlannedOperation::Predicate(PlannedOperationPredicate {
            seq,
            account_id: "acct-a".into(),
            predicate_kind: PredicateKind::Rule,
            predicate_id: "rule-1".into(),
            action: PlanAction::Archive,
            target: None,
            source: PlanSource::Manual,
            projected_count: sample_ids.len() as u64,
            sample_email_ids: sample_ids,
            risk: RiskLevel::Low,
            status: PredicateStatus::Pending,
            partial_applied_count: 0,
            error: None,
        })
    }

    #[tokio::test]
    async fn update_operation_status_syncs_payload_status_only() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-upd-status";
        let plan = sample_plan(user);
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        repo.update_operation_status(plan_id, 1, OperationStatus::Applied, Utc::now())
            .await
            .expect("update");

        let (ops, _) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 10)
            .await
            .expect("list");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PlannedOperation::Materialized(r) => {
                assert_eq!(r.status, OperationStatus::Applied);
                // Only the payload's top-level `status` key is synced; `appliedAt`
                // inside the payload deliberately stays untouched (the applied_at
                // COLUMN carries the timestamp).
                assert!(r.applied_at.is_none());
            }
            other => panic!("expected materialized row, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_predicate_status_skips_materialized_rows() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-pred-skip";
        let mut plan = sample_plan(user);
        plan.operations.push(sample_predicate(2, vec!["e7".into()]));
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        // seq 1 is materialized — a predicate-status update must not touch it.
        repo.update_predicate_status(plan_id, 1, PredicateStatus::Expanded)
            .await
            .expect("noop update");
        // seq 2 is the predicate row — this one transitions.
        repo.update_predicate_status(plan_id, 2, PredicateStatus::Expanded)
            .await
            .expect("update");

        let (ops, _) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 10)
            .await
            .expect("list");
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            PlannedOperation::Materialized(r) => assert_eq!(r.status, OperationStatus::Pending),
            other => panic!("expected materialized row, got {other:?}"),
        }
        match &ops[1] {
            PlannedOperation::Predicate(p) => assert_eq!(p.status, PredicateStatus::Expanded),
            other => panic!("expected predicate row, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn max_seq_zero_on_empty_plan_and_max_otherwise() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-max-seq";
        let mut plan = sample_plan(user);
        plan.operations.push(sample_predicate(7, vec![]));
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");
        assert_eq!(repo.max_seq(plan_id).await.expect("max"), 7);

        let mut empty = sample_plan("user-max-seq-empty");
        empty.operations.clear();
        let empty_id = empty.id;
        repo.save(&empty).await.expect("save empty");
        assert_eq!(repo.max_seq(empty_id).await.expect("max"), 0);
    }

    #[tokio::test]
    async fn sample_operations_reads_column_save_never_populates() {
        // Pins CURRENT behavior: save() never writes sample_ids_json (sample ids
        // only live inside payload_json), so sample_operations() returns empty
        // for plans persisted through this repo. Latent gap recorded in the
        // phase's parking-lot — the port must preserve, not silently fix, it.
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-sample-ops";
        let mut plan = sample_plan(user);
        plan.operations.push(sample_predicate(
            2,
            vec!["e1".into(), "e2".into(), "e3".into()],
        ));
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        let ids = repo
            .sample_operations(plan_id, "manual", 2)
            .await
            .expect("sample");
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn expire_due_expires_only_overdue_ready_or_draft() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let now = Utc::now();

        let user_a = "user-expire-a";
        let mut overdue = sample_plan(user_a);
        overdue.valid_until = now - Duration::hours(1);
        let overdue_id = overdue.id;
        repo.save(&overdue).await.expect("save overdue");

        let user_b = "user-expire-b";
        let fresh = sample_plan(user_b);
        let fresh_id = fresh.id;
        repo.save(&fresh).await.expect("save fresh");

        let affected = repo.expire_due(now).await.expect("expire");
        assert_eq!(affected, 1);
        let overdue_loaded = repo.load(user_a, overdue_id).await.expect("load").unwrap();
        assert_eq!(overdue_loaded.status, PlanStatus::Expired);
        let fresh_loaded = repo.load(user_b, fresh_id).await.expect("load").unwrap();
        assert_eq!(fresh_loaded.status, PlanStatus::Ready);
    }

    #[tokio::test]
    async fn purge_older_than_deletes_only_older_plans() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let now = Utc::now();

        let user_a = "user-purge-a";
        let mut old = sample_plan(user_a);
        old.valid_until = now - Duration::hours(2);
        let old_id = old.id;
        repo.save(&old).await.expect("save old");

        let user_b = "user-purge-b";
        let recent = sample_plan(user_b);
        let recent_id = recent.id;
        repo.save(&recent).await.expect("save recent");

        let purged = repo
            .purge_older_than(now - Duration::hours(1))
            .await
            .expect("purge");
        assert_eq!(purged, 1);
        assert!(repo.load(user_a, old_id).await.expect("load").is_none());
        assert!(repo.load(user_b, recent_id).await.expect("load").is_some());
    }

    #[tokio::test]
    async fn replace_account_rows_swaps_only_target_account() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-replace";
        let mut plan = sample_plan(user);
        plan.operations
            .push(PlannedOperation::Materialized(PlannedOperationRow {
                seq: 2,
                account_id: "acct-b".into(),
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

        let replacement = PlannedOperation::Materialized(PlannedOperationRow {
            seq: 3,
            account_id: "acct-a".into(),
            email_id: Some("e9".into()),
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
        repo.replace_account_rows(plan_id, "acct-a", vec![replacement])
            .await
            .expect("replace");

        let (ops, _) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 10)
            .await
            .expect("list");
        let seqs: Vec<u64> = ops.iter().map(|o| o.seq()).collect();
        assert_eq!(seqs, vec![2, 3]);
        assert_eq!(ops[0].account_id(), "acct-b");
        assert_eq!(ops[1].account_id(), "acct-a");
    }

    #[tokio::test]
    async fn append_operations_appends_without_touching_existing_rows() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-append";
        let plan = sample_plan(user);
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        repo.append_operations(plan_id, Vec::new())
            .await
            .expect("append empty is a no-op");

        let appended = PlannedOperation::Materialized(PlannedOperationRow {
            seq: 5,
            account_id: "acct-a".into(),
            email_id: Some("e5".into()),
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
        repo.append_operations(plan_id, vec![appended])
            .await
            .expect("append");

        let (ops, _) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 10)
            .await
            .expect("list");
        let seqs: Vec<u64> = ops.iter().map(|o| o.seq()).collect();
        assert_eq!(seqs, vec![1, 5]);
    }

    #[tokio::test]
    async fn list_operations_filters_by_account_risk_and_action() {
        let repo = SeaOrmCleanupPlanRepo::new(fresh_conn().await);
        let user = "user-filters";
        let mut plan = sample_plan(user);
        plan.operations
            .push(PlannedOperation::Materialized(PlannedOperationRow {
                seq: 2,
                account_id: "acct-b".into(),
                email_id: Some("e2".into()),
                action: PlanAction::MarkRead,
                source: PlanSource::Manual,
                target: None,
                reverse_op: None,
                risk: RiskLevel::High,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            }));
        let plan_id = plan.id;
        repo.save(&plan).await.expect("save");

        let by_account = repo
            .list_operations(
                plan_id,
                OpsFilter {
                    account_id: Some("acct-b".into()),
                    ..OpsFilter::default()
                },
                None,
                10,
            )
            .await
            .expect("list");
        assert_eq!(
            by_account.0.iter().map(|o| o.seq()).collect::<Vec<_>>(),
            [2]
        );

        let by_risk = repo
            .list_operations(
                plan_id,
                OpsFilter {
                    risk: Some("high".into()),
                    ..OpsFilter::default()
                },
                None,
                10,
            )
            .await
            .expect("list");
        assert_eq!(by_risk.0.iter().map(|o| o.seq()).collect::<Vec<_>>(), [2]);

        let no_match = repo
            .list_operations(
                plan_id,
                OpsFilter {
                    account_id: Some("acct-zzz".into()),
                    ..OpsFilter::default()
                },
                None,
                10,
            )
            .await
            .expect("list");
        assert!(no_match.0.is_empty());
    }

    /// Full-trait verification against a live PostgreSQL instance — the
    /// ADR-036 exemplar proof (postgres-support pipeline, phase 2 DoD). Skips
    /// (trivially passing) when `EMAILIBRIUM_TEST_PG_URL` is unset so the
    /// default suite needs no infrastructure. Reproduction:
    ///
    /// ```sh
    /// docker run -d --rm --name emailibrium-pg-test -p 55433:5432 \
    ///   -e POSTGRES_PASSWORD=test -e POSTGRES_DB=emailibrium_test postgres:16-alpine
    /// EMAILIBRIUM_TEST_PG_URL='postgres://postgres:test@localhost:55433/emailibrium_test' \
    ///   cargo test cleanup::repository -- --nocapture
    /// docker rm -f emailibrium-pg-test
    /// ```
    #[tokio::test]
    async fn postgres_full_trait_round_trip() {
        let Ok(url) = std::env::var("EMAILIBRIUM_TEST_PG_URL") else {
            eprintln!("skipping postgres_full_trait_round_trip: EMAILIBRIUM_TEST_PG_URL unset");
            return;
        };
        let db = Database::connect(&url).await.expect("pg connect");
        db.run_migrations().await.expect("pg migrations");
        let repo = SeaOrmCleanupPlanRepo::new(db.sea_orm());

        // Unique users so reruns against a persistent database stay clean.
        let user = format!("pg-user-{}", Uuid::new_v4());
        let mut plan = sample_plan(&user);
        plan.operations.push(sample_predicate(2, vec!["e7".into()]));
        let plan_id = plan.id;

        // save: insert, then upsert over the same PK.
        repo.save(&plan).await.expect("save");
        repo.save(&plan).await.expect("upsert");

        let loaded = repo
            .load(&user, plan_id)
            .await
            .expect("load")
            .expect("present");
        assert_eq!(loaded.operations.len(), 2);
        assert_eq!(loaded.account_state_etags.len(), 1);

        let summaries = repo
            .list_by_user(&user, Some(PlanStatus::Ready), 10)
            .await
            .expect("list_by_user");
        assert_eq!(summaries.len(), 1);

        // Cursor pagination + filters.
        let (page, cursor) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 1)
            .await
            .expect("page 1");
        assert_eq!(page.len(), 1);
        let (rest, _) = repo
            .list_operations(plan_id, OpsFilter::default(), cursor, 10)
            .await
            .expect("page 2");
        assert_eq!(rest.len(), 1);
        let (by_account, _) = repo
            .list_operations(
                plan_id,
                OpsFilter {
                    account_id: Some("acct-a".into()),
                    ..OpsFilter::default()
                },
                None,
                10,
            )
            .await
            .expect("filtered");
        assert_eq!(by_account.len(), 2);

        // sample_operations: empty per pinned behavior (save never writes
        // sample_ids_json).
        assert!(repo
            .sample_operations(plan_id, "manual", 5)
            .await
            .expect("sample")
            .is_empty());

        // append + max_seq.
        repo.append_operations(
            plan_id,
            vec![PlannedOperation::Materialized(PlannedOperationRow {
                seq: 5,
                account_id: "acct-a".into(),
                email_id: Some("e5".into()),
                action: PlanAction::Archive,
                source: PlanSource::Manual,
                target: None,
                reverse_op: None,
                risk: RiskLevel::Low,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            })],
        )
        .await
        .expect("append");
        assert_eq!(repo.max_seq(plan_id).await.expect("max_seq"), 5);

        // Status updates: payload sync (formerly SQL-side JSON mutation).
        repo.update_operation_status(plan_id, 1, OperationStatus::Applied, Utc::now())
            .await
            .expect("update op status");
        repo.update_predicate_status(plan_id, 2, PredicateStatus::Expanded)
            .await
            .expect("update predicate status");
        let (ops, _) = repo
            .list_operations(plan_id, OpsFilter::default(), None, 10)
            .await
            .expect("list");
        match &ops[0] {
            PlannedOperation::Materialized(r) => assert_eq!(r.status, OperationStatus::Applied),
            other => panic!("expected materialized row, got {other:?}"),
        }
        match &ops[1] {
            PlannedOperation::Predicate(p) => assert_eq!(p.status, PredicateStatus::Expanded),
            other => panic!("expected predicate row, got {other:?}"),
        }

        // replace_account_rows (all rows are acct-a).
        repo.replace_account_rows(
            plan_id,
            "acct-a",
            vec![PlannedOperation::Materialized(PlannedOperationRow {
                seq: 9,
                account_id: "acct-a".into(),
                email_id: Some("e9".into()),
                action: PlanAction::Archive,
                source: PlanSource::Manual,
                target: None,
                reverse_op: None,
                risk: RiskLevel::Low,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            })],
        )
        .await
        .expect("replace");
        assert_eq!(repo.max_seq(plan_id).await.expect("max_seq"), 9);

        // cancel.
        repo.cancel(plan_id).await.expect("cancel");
        assert_eq!(
            repo.load(&user, plan_id)
                .await
                .expect("load")
                .unwrap()
                .status,
            PlanStatus::Cancelled
        );

        // expire_due + purge_older_than: assert on our own plans only — the
        // shared database may hold rows from other runs (counts are >=, never ==).
        let user2 = format!("pg-user-{}", Uuid::new_v4());
        let mut overdue = sample_plan(&user2);
        overdue.valid_until = Utc::now() - Duration::hours(1);
        let overdue_id = overdue.id;
        repo.save(&overdue).await.expect("save overdue");
        let expired = repo.expire_due(Utc::now()).await.expect("expire");
        assert!(expired >= 1);
        assert_eq!(
            repo.load(&user2, overdue_id)
                .await
                .expect("load")
                .unwrap()
                .status,
            PlanStatus::Expired
        );

        let purged = repo
            .purge_older_than(Utc::now() - Duration::minutes(30))
            .await
            .expect("purge");
        assert!(purged >= 1);
        assert!(repo.load(&user2, overdue_id).await.expect("load").is_none());
    }
}
