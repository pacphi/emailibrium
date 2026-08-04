//! `CleanupPlanRepository` trait + `SqliteCleanupPlanRepo` impl.
//!
//! See migration `024_cleanup_planning.sql` for schema. JSON-typed columns are
//! stored as plaintext TEXT (see migration header for the encryption-debt note).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cleanup::domain::operation::{
    AccountStateEtag, OperationStatus, PlanStatus, PlannedOperation, Provider,
};
use crate::cleanup::domain::plan::{CleanupPlan, CleanupPlanSummary, PlanId};
use crate::cleanup::domain::ports::RepoError;
use crate::db::{audited_sql, Database};

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
    db: Database,
}

impl SqliteCleanupPlanRepo {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CleanupPlanRepository for SqliteCleanupPlanRepo {
    async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError> {
        // SQLite's `INSERT OR REPLACE` has no direct PostgreSQL keyword-for-keyword
        // equivalent — PostgreSQL's upsert is `INSERT ... ON CONFLICT (id) DO UPDATE SET ...`.
        // The two are written out separately per backend (not through `Database::adapt`,
        // which only handles placeholder/`datetime('now')` translation — a genuinely
        // different SQL clause needs genuinely different SQL text, per ADR-035 §2.3).
        let totals_json = serde_json::to_string(&PersistedTotals {
            totals: &plan.totals,
            account_providers: &plan.account_providers,
        })
        .map_err(|e| RepoError::Internal(e.to_string()))?;
        let risk_json =
            serde_json::to_string(&plan.risk).map_err(|e| RepoError::Internal(e.to_string()))?;
        let warnings_json = serde_json::to_string(&plan.warnings)
            .map_err(|e| RepoError::Internal(e.to_string()))?;

        let delete_etags_sql = self
            .db
            .adapt("DELETE FROM cleanup_plan_account_etags WHERE plan_id = ?");
        let insert_etag_sql = self.db.adapt(
            r#"INSERT INTO cleanup_plan_account_etags
               (plan_id, account_id, etag_kind, etag_value)
               VALUES (?, ?, ?, ?)"#,
        );
        let delete_ops_sql = self
            .db
            .adapt("DELETE FROM cleanup_plan_operations WHERE plan_id = ?");
        let insert_op_sql = self.db.adapt(
            r#"INSERT INTO cleanup_plan_operations
               (plan_id, seq, op_kind, account_id, email_id, action, source_kind,
                risk, status, payload_json)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        );

        match &self.db {
            Database::Sqlite(pool) => {
                let mut tx = pool.begin().await?;
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
                .bind(&totals_json)
                .bind(&risk_json)
                .bind(&warnings_json)
                .execute(&mut *tx)
                .await?;

                sqlx::query(audited_sql(&delete_etags_sql))
                    .bind(plan.id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for (account_id, etag) in &plan.account_state_etags {
                    let kind = etag.kind_str();
                    let value = serde_json::to_string(etag)
                        .map_err(|e| RepoError::Internal(e.to_string()))?;
                    sqlx::query(audited_sql(&insert_etag_sql))
                        .bind(plan.id.as_bytes().to_vec())
                        .bind(account_id.as_bytes().to_vec())
                        .bind(kind)
                        .bind(value)
                        .execute(&mut *tx)
                        .await?;
                }

                sqlx::query(audited_sql(&delete_ops_sql))
                    .bind(plan.id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for op in &plan.operations {
                    insert_operation(&mut *tx, &insert_op_sql, plan.id, op).await?;
                }

                tx.commit().await?;
            }
            Database::Postgres(pool) => {
                let mut tx = pool.begin().await?;
                let sql = self.db.adapt(
                    r#"INSERT INTO cleanup_plans
                       (id, user_id, created_at, valid_until, plan_hash, status,
                        totals_json, risk_json, warnings_json)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                       ON CONFLICT (id) DO UPDATE SET
                         user_id = EXCLUDED.user_id, created_at = EXCLUDED.created_at,
                         valid_until = EXCLUDED.valid_until, plan_hash = EXCLUDED.plan_hash,
                         status = EXCLUDED.status, totals_json = EXCLUDED.totals_json,
                         risk_json = EXCLUDED.risk_json, warnings_json = EXCLUDED.warnings_json"#,
                );
                sqlx::query(audited_sql(&sql))
                    .bind(plan.id.as_bytes().to_vec())
                    .bind(plan.user_id.as_bytes().to_vec())
                    .bind(plan.created_at.timestamp_millis())
                    .bind(plan.valid_until.timestamp_millis())
                    .bind(plan.plan_hash.to_vec())
                    .bind(plan.status.as_str())
                    .bind(&totals_json)
                    .bind(&risk_json)
                    .bind(&warnings_json)
                    .execute(&mut *tx)
                    .await?;

                sqlx::query(audited_sql(&delete_etags_sql))
                    .bind(plan.id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for (account_id, etag) in &plan.account_state_etags {
                    let kind = etag.kind_str();
                    let value = serde_json::to_string(etag)
                        .map_err(|e| RepoError::Internal(e.to_string()))?;
                    sqlx::query(audited_sql(&insert_etag_sql))
                        .bind(plan.id.as_bytes().to_vec())
                        .bind(account_id.as_bytes().to_vec())
                        .bind(kind)
                        .bind(value)
                        .execute(&mut *tx)
                        .await?;
                }

                sqlx::query(audited_sql(&delete_ops_sql))
                    .bind(plan.id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for op in &plan.operations {
                    insert_operation(&mut *tx, &insert_op_sql, plan.id, op).await?;
                }

                tx.commit().await?;
            }
        }
        Ok(())
    }

    async fn load(&self, user_id: &str, id: PlanId) -> Result<Option<CleanupPlan>, RepoError> {
        // Phase A: simplified loader — only the envelope + ops in seq order.
        // Phase C will optimise.
        let sql = self.db.adapt(
            r#"SELECT user_id, created_at, valid_until, plan_hash, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans WHERE id = ? AND user_id = ?"#,
        );
        let row: Option<(Vec<u8>, i64, i64, Vec<u8>, String, String, String, String)> =
            match &self.db {
                Database::Sqlite(pool) => {
                    sqlx::query_as(audited_sql(&sql))
                        .bind(id.as_bytes().to_vec())
                        .bind(user_id.as_bytes().to_vec())
                        .fetch_optional(pool)
                        .await?
                }
                Database::Postgres(pool) => {
                    sqlx::query_as(audited_sql(&sql))
                        .bind(id.as_bytes().to_vec())
                        .bind(user_id.as_bytes().to_vec())
                        .fetch_optional(pool)
                        .await?
                }
            };
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
        let etag_sql = self.db.adapt(
            r#"SELECT account_id, etag_kind, etag_value
               FROM cleanup_plan_account_etags WHERE plan_id = ?"#,
        );
        let etag_rows: Vec<(Vec<u8>, String, Option<String>)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&etag_sql))
                    .bind(id.as_bytes().to_vec())
                    .fetch_all(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&etag_sql))
                    .bind(id.as_bytes().to_vec())
                    .fetch_all(pool)
                    .await?
            }
        };
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
                let sql = self.db.adapt(
                    r#"SELECT id, created_at, valid_until, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans
                   WHERE user_id = ? AND status = ?
                   ORDER BY created_at DESC LIMIT ?"#,
                );
                match &self.db {
                    Database::Sqlite(pool) => {
                        sqlx::query_as(audited_sql(&sql))
                            .bind(user_id.as_bytes().to_vec())
                            .bind(s.as_str())
                            .bind(limit)
                            .fetch_all(pool)
                            .await?
                    }
                    Database::Postgres(pool) => {
                        sqlx::query_as(audited_sql(&sql))
                            .bind(user_id.as_bytes().to_vec())
                            .bind(s.as_str())
                            .bind(limit)
                            .fetch_all(pool)
                            .await?
                    }
                }
            }
            None => {
                let sql = self.db.adapt(
                    r#"SELECT id, created_at, valid_until, status,
                          totals_json, risk_json, warnings_json
                   FROM cleanup_plans
                   WHERE user_id = ?
                   ORDER BY created_at DESC LIMIT ?"#,
                );
                match &self.db {
                    Database::Sqlite(pool) => {
                        sqlx::query_as(audited_sql(&sql))
                            .bind(user_id.as_bytes().to_vec())
                            .bind(limit)
                            .fetch_all(pool)
                            .await?
                    }
                    Database::Postgres(pool) => {
                        sqlx::query_as(audited_sql(&sql))
                            .bind(user_id.as_bytes().to_vec())
                            .bind(limit)
                            .fetch_all(pool)
                            .await?
                    }
                }
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
        // Adapt placeholders (and datetime('now'), unused here) for the connected backend
        // before the audited-SQL guard, same as every other dynamic query in this file.
        let sql = self.db.adapt(&sql);

        // (seq, payload) — a portable decode target across both backends (see ADR-035).
        // seq is i32 because the `seq` column is INTEGER/INT4 in both dialects (a real
        // Postgres 4-byte int, unlike SQLite's dynamically-8-byte INTEGER) — decoding it
        // as i64 fails at runtime against Postgres with a ColumnDecode type mismatch.
        // Replaces the raw Row::get() this dynamic-filter query used to need.
        let rows: Vec<(i32, String)> = {
            macro_rules! run_query {
                ($pool:expr) => {{
                    let mut q = sqlx::query_as(audited_sql(&sql))
                        .bind(id.as_bytes().to_vec())
                        .bind(cursor_i);
                    if let Some(ref a) = filter.account_id {
                        // account_id is stored as BLOB/BYTEA (bytes); bind as bytes so the
                        // comparison succeeds against the byte column, not a TEXT one.
                        q = q.bind(a.as_bytes().to_vec());
                    }
                    if let Some(ref r) = filter.risk {
                        q = q.bind(r);
                    }
                    if let Some(ref act) = filter.action {
                        q = q.bind(act);
                    }
                    q = q.bind(limit);
                    q.fetch_all($pool).await
                }};
            }
            let result = match &self.db {
                Database::Sqlite(pool) => run_query!(pool),
                Database::Postgres(pool) => run_query!(pool),
            };
            result?
        };

        let mut ops = Vec::with_capacity(rows.len());
        let mut last_seq: Option<u64> = cursor;
        for (seq, payload) in rows {
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
        let sql = self.db.adapt(
            r#"SELECT sample_ids_json
               FROM cleanup_plan_operations
               WHERE plan_id = ? AND op_kind = 'predicate' AND source_kind = ?
               LIMIT 1"#,
        );
        let row: Option<(Option<String>,)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .bind(source_kind)
                    .fetch_optional(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .bind(source_kind)
                    .fetch_optional(pool)
                    .await?
            }
        };

        let Some((json_s,)) = row else {
            return Ok(Vec::new());
        };
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
        let delete_sql = self.db.adapt(
            r#"DELETE FROM cleanup_plan_operations
               WHERE plan_id = ? AND account_id = ?"#,
        );
        let insert_op_sql = self.db.adapt(
            r#"INSERT INTO cleanup_plan_operations
               (plan_id, seq, op_kind, account_id, email_id, action, source_kind,
                risk, status, payload_json)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        );
        match &self.db {
            Database::Sqlite(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(audited_sql(&delete_sql))
                    .bind(id.as_bytes().to_vec())
                    .bind(account_id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for op in &new_rows {
                    insert_operation(&mut *tx, &insert_op_sql, id, op).await?;
                }
                tx.commit().await?;
            }
            Database::Postgres(pool) => {
                let mut tx = pool.begin().await?;
                sqlx::query(audited_sql(&delete_sql))
                    .bind(id.as_bytes().to_vec())
                    .bind(account_id.as_bytes().to_vec())
                    .execute(&mut *tx)
                    .await?;
                for op in &new_rows {
                    insert_operation(&mut *tx, &insert_op_sql, id, op).await?;
                }
                tx.commit().await?;
            }
        }
        Ok(())
    }

    async fn update_operation_status(
        &self,
        id: PlanId,
        seq: u64,
        status: OperationStatus,
        ts: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        // Keep payload_json in sync so list_operations returns current status without a
        // separate column merge. SQLite's json_set() and Postgres's jsonb_set() have genuinely
        // different signatures (path syntax, and payload_json is TEXT so Postgres needs an
        // explicit ::jsonb/::text cast pair) — not a case Database::adapt's mechanical
        // placeholder/datetime translation covers, so each backend gets its own SQL text here
        // (ADR-035 §2.3: hand-duplication remains the right call where translation isn't
        // mechanically sufficient).
        match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query(
                    r#"UPDATE cleanup_plan_operations
                       SET status = ?, applied_at = ?,
                           payload_json = json_set(payload_json, '$.status', ?)
                       WHERE plan_id = ? AND seq = ?"#,
                )
                .bind(status.as_str())
                .bind(ts.timestamp_millis())
                .bind(status.as_str())
                .bind(id.as_bytes().to_vec())
                .bind(seq as i64)
                .execute(pool)
                .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(
                    r#"UPDATE cleanup_plan_operations
                       SET status = $1, applied_at = $2,
                           payload_json = jsonb_set(payload_json::jsonb, '{status}', to_jsonb($3::text))::text
                       WHERE plan_id = $4 AND seq = $5"#,
                )
                .bind(status.as_str())
                .bind(ts.timestamp_millis())
                .bind(status.as_str())
                .bind(id.as_bytes().to_vec())
                .bind(seq as i64)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    async fn update_predicate_status(
        &self,
        id: PlanId,
        seq: u64,
        status: crate::cleanup::domain::operation::PredicateStatus,
    ) -> Result<(), RepoError> {
        // See update_operation_status's doc for why json_set/jsonb_set get separate SQL text.
        match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query(
                    r#"UPDATE cleanup_plan_operations
                       SET status = ?,
                           payload_json = json_set(payload_json, '$.status', ?)
                       WHERE plan_id = ? AND seq = ? AND op_kind = 'predicate'"#,
                )
                .bind(status.as_str())
                .bind(status.as_str())
                .bind(id.as_bytes().to_vec())
                .bind(seq as i64)
                .execute(pool)
                .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(
                    r#"UPDATE cleanup_plan_operations
                       SET status = $1,
                           payload_json = jsonb_set(payload_json::jsonb, '{status}', to_jsonb($2::text))::text
                       WHERE plan_id = $3 AND seq = $4 AND op_kind = 'predicate'"#,
                )
                .bind(status.as_str())
                .bind(status.as_str())
                .bind(id.as_bytes().to_vec())
                .bind(seq as i64)
                .execute(pool)
                .await?;
            }
        }
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
        let sql = self.db.adapt(
            r#"INSERT INTO cleanup_plan_operations
               (plan_id, seq, op_kind, account_id, email_id, action, source_kind,
                risk, status, payload_json)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        );
        match &self.db {
            Database::Sqlite(pool) => {
                let mut tx = pool.begin().await?;
                for op in &rows {
                    insert_operation(&mut *tx, &sql, id, op).await?;
                }
                tx.commit().await?;
            }
            Database::Postgres(pool) => {
                let mut tx = pool.begin().await?;
                for op in &rows {
                    insert_operation(&mut *tx, &sql, id, op).await?;
                }
                tx.commit().await?;
            }
        }
        Ok(())
    }

    async fn max_seq(&self, id: PlanId) -> Result<u64, RepoError> {
        let sql = self
            .db
            .adapt(r#"SELECT MAX(seq) FROM cleanup_plan_operations WHERE plan_id = ?"#);
        // MAX() over an INTEGER/INT4 `seq` column returns that same width — i32, not i64
        // (same real-4-byte-int gotcha as list_operations' seq decode above).
        let row: Option<(Option<i32>,)> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .fetch_optional(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .fetch_optional(pool)
                    .await?
            }
        };
        Ok(row.and_then(|(m,)| m).map(|v| v.max(0) as u64).unwrap_or(0))
    }

    async fn cancel(&self, id: PlanId) -> Result<(), RepoError> {
        let sql = self
            .db
            .adapt("UPDATE cleanup_plans SET status = 'cancelled' WHERE id = ?");
        match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(id.as_bytes().to_vec())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<u32, RepoError> {
        let sql = self.db.adapt(
            r#"UPDATE cleanup_plans SET status = 'expired'
               WHERE valid_until < ? AND status IN ('ready', 'draft')"#,
        );
        let affected = match &self.db {
            Database::Sqlite(pool) => sqlx::query(audited_sql(&sql))
                .bind(now.timestamp_millis())
                .execute(pool)
                .await?
                .rows_affected(),
            Database::Postgres(pool) => sqlx::query(audited_sql(&sql))
                .bind(now.timestamp_millis())
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected as u32)
    }

    async fn purge_older_than(&self, cutoff: DateTime<Utc>) -> Result<u32, RepoError> {
        let sql = self
            .db
            .adapt("DELETE FROM cleanup_plans WHERE valid_until < ?");
        let affected = match &self.db {
            Database::Sqlite(pool) => sqlx::query(audited_sql(&sql))
                .bind(cutoff.timestamp_millis())
                .execute(pool)
                .await?
                .rows_affected(),
            Database::Postgres(pool) => sqlx::query(audited_sql(&sql))
                .bind(cutoff.timestamp_millis())
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(affected as u32)
    }
}

/// Insert one operation row. Generic over the sqlx backend (`DB`) so this logic — parsing a
/// [`PlannedOperation`] into columns — is written once and works against either a SQLite or a
/// PostgreSQL transaction; only the transaction lifecycle (`begin`/`commit`) and the SQL text
/// itself (pre-adapted by the caller via `Database::adapt`, since a generic function has no
/// enum variant to dispatch on) differ per backend — see ADR-035.
async fn insert_operation<'e, DB, E>(
    exec: E,
    sql: &str,
    plan_id: PlanId,
    op: &PlannedOperation,
) -> Result<(), RepoError>
where
    DB: sqlx::Database,
    E: sqlx::Executor<'e, Database = DB>,
    DB::Arguments: sqlx::IntoArguments<DB>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Vec<u8>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Option<Vec<u8>>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> &'q str: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
{
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
    sqlx::query(audited_sql(sql))
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
        .execute(exec)
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
        PlannedOperationPredicate, PlannedOperationRow, PredicateKind, PredicateStatus, Provider,
        RiskLevel,
    };
    use crate::cleanup::domain::plan::{CleanupPlan, PlanTotals, RiskRollup};
    use chrono::{Duration, Utc};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn fresh_pool() -> Database {
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
        Database::Sqlite(pool)
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
        let user = "user-sample-ops";
        let mut plan = sample_plan(user);
        plan.operations
            .push(sample_predicate(2, vec!["e1".into(), "e2".into(), "e3".into()]));
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        let pool = fresh_pool().await;
        let repo = SqliteCleanupPlanRepo::new(pool);
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
        assert_eq!(by_account.0.iter().map(|o| o.seq()).collect::<Vec<_>>(), [2]);

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
}
