//! `CleanupApplyJobRepository` (Phase C will drive; Phase A persists shape only).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::cleanup::domain::operation::{JobState, RiskMax};
use crate::cleanup::domain::plan::{CleanupApplyJob, JobCounts, JobId, PlanId};
use crate::cleanup::domain::ports::RepoError;
use crate::db::{audited_sql, Database};

/// Row tuple for `cleanup_apply_jobs` — a portable decode target across both backends (see
/// `map_job_row`'s doc for why this replaced raw `Row::get()` on a backend-concrete row type).
type JobRow = (Vec<u8>, Vec<u8>, i64, Option<i64>, String, String, String);

#[async_trait]
pub trait CleanupApplyJobRepository: Send + Sync {
    async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError>;
    async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError>;
    async fn update_state(
        &self,
        job_id: JobId,
        state: JobState,
        counts: JobCounts,
        finished_at: Option<DateTime<Utc>>,
    ) -> Result<(), RepoError>;
    async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError>;
}

pub struct SqliteCleanupApplyJobRepo {
    db: Database,
}

impl SqliteCleanupApplyJobRepo {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CleanupApplyJobRepository for SqliteCleanupApplyJobRepo {
    async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError> {
        let counts =
            serde_json::to_string(&job.counts).map_err(|e| RepoError::Internal(e.to_string()))?;
        let sql = self.db.adapt(
            r#"INSERT INTO cleanup_apply_jobs
               (job_id, plan_id, started_at, finished_at, state, risk_max, counts_json)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        );
        let risk_str = match job.risk_max {
            crate::cleanup::domain::operation::RiskMax::Low => "low",
            crate::cleanup::domain::operation::RiskMax::Medium => "medium",
            crate::cleanup::domain::operation::RiskMax::High => "high",
        };
        match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(job.job_id.as_bytes().to_vec())
                    .bind(job.plan_id.as_bytes().to_vec())
                    .bind(job.started_at.timestamp_millis())
                    .bind(job.finished_at.map(|t| t.timestamp_millis()))
                    .bind(job.state.as_str())
                    .bind(risk_str)
                    .bind(&counts)
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(job.job_id.as_bytes().to_vec())
                    .bind(job.plan_id.as_bytes().to_vec())
                    .bind(job.started_at.timestamp_millis())
                    .bind(job.finished_at.map(|t| t.timestamp_millis()))
                    .bind(job.state.as_str())
                    .bind(risk_str)
                    .bind(&counts)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError> {
        let sql = self.db.adapt(
            r#"SELECT job_id, plan_id, started_at, finished_at, state, risk_max, counts_json
               FROM cleanup_apply_jobs WHERE job_id = ?"#,
        );
        let row: Option<JobRow> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(job_id.as_bytes().to_vec())
                    .fetch_optional(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(job_id.as_bytes().to_vec())
                    .fetch_optional(pool)
                    .await?
            }
        };

        let Some(r) = row else {
            return Ok(None);
        };

        let job = map_job_row(r)?;
        Ok(Some(job))
    }

    async fn update_state(
        &self,
        job_id: JobId,
        state: JobState,
        counts: JobCounts,
        finished_at: Option<DateTime<Utc>>,
    ) -> Result<(), RepoError> {
        let counts_s =
            serde_json::to_string(&counts).map_err(|e| RepoError::Internal(e.to_string()))?;
        let sql = self.db.adapt(
            r#"UPDATE cleanup_apply_jobs
               SET state = ?, counts_json = ?, finished_at = ?
               WHERE job_id = ?"#,
        );
        let finished_ms = finished_at.map(|t| t.timestamp_millis());
        match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(state.as_str())
                    .bind(&counts_s)
                    .bind(finished_ms)
                    .bind(job_id.as_bytes().to_vec())
                    .execute(pool)
                    .await?;
            }
            Database::Postgres(pool) => {
                sqlx::query(audited_sql(&sql))
                    .bind(state.as_str())
                    .bind(&counts_s)
                    .bind(finished_ms)
                    .bind(job_id.as_bytes().to_vec())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError> {
        let sql = self.db.adapt(
            r#"SELECT job_id, plan_id, started_at, finished_at, state, risk_max, counts_json
               FROM cleanup_apply_jobs WHERE plan_id = ?
               ORDER BY started_at DESC"#,
        );
        let rows: Vec<JobRow> = match &self.db {
            Database::Sqlite(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(plan_id.as_bytes().to_vec())
                    .fetch_all(pool)
                    .await?
            }
            Database::Postgres(pool) => {
                sqlx::query_as(audited_sql(&sql))
                    .bind(plan_id.as_bytes().to_vec())
                    .fetch_all(pool)
                    .await?
            }
        };

        rows.into_iter().map(map_job_row).collect()
    }
}

/// Decode a portable [`JobRow`] tuple into a `CleanupApplyJob`. Takes an owned tuple (not a raw
/// backend-concrete `Row`) so the same function works whether the row came from SQLite or
/// PostgreSQL — see ADR-035.
fn map_job_row(r: JobRow) -> Result<CleanupApplyJob, RepoError> {
    let (job_id_bytes, plan_id_bytes, started_ms, finished_ms, state_s, risk_s, counts_s) = r;

    let job_id = Uuid::from_slice(&job_id_bytes)
        .map_err(|e| RepoError::Internal(format!("bad job_id: {e}")))?;
    let plan_id = Uuid::from_slice(&plan_id_bytes)
        .map_err(|e| RepoError::Internal(format!("bad plan_id: {e}")))?;

    let started_at = DateTime::from_timestamp_millis(started_ms)
        .ok_or_else(|| RepoError::Internal("bad started_at".into()))?;

    let finished_at = finished_ms.and_then(DateTime::from_timestamp_millis);

    let state = JobState::from_str_opt(&state_s)
        .ok_or_else(|| RepoError::Internal(format!("bad job state: {state_s}")))?;

    let risk_max = match risk_s.as_str() {
        "medium" => RiskMax::Medium,
        "high" => RiskMax::High,
        _ => RiskMax::Low,
    };

    let counts: JobCounts =
        serde_json::from_str(&counts_s).map_err(|e| RepoError::Internal(e.to_string()))?;

    Ok(CleanupApplyJob {
        job_id,
        plan_id,
        started_at,
        finished_at,
        state,
        risk_max,
        counts,
    })
}

// ---------------------------------------------------------------------------
// Behavior pins for the SeaORM port (ADR-036): written against the hand-rolled
// implementation; they must stay green unchanged across the re-port.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::operation::{JobState, RiskMax};
    use crate::cleanup::domain::plan::{CleanupApplyJob, JobCounts};
    use crate::cleanup::repository::plan_repo::{CleanupPlanRepository, SqliteCleanupPlanRepo};
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

    /// Persist a minimal parent plan so `cleanup_apply_jobs.plan_id`'s FK is
    /// satisfiable, then return its id.
    async fn seed_plan(db: &Database, user: &str) -> PlanId {
        use crate::cleanup::domain::plan::{CleanupPlan, PlanTotals, RiskRollup};
        let now = Utc::now();
        let plan = CleanupPlan {
            id: Uuid::now_v7(),
            user_id: user.into(),
            account_ids: vec![],
            created_at: now,
            valid_until: now + Duration::minutes(30),
            plan_hash: [0u8; 32],
            account_state_etags: std::collections::BTreeMap::new(),
            account_providers: std::collections::BTreeMap::new(),
            status: crate::cleanup::domain::operation::PlanStatus::Ready,
            totals: PlanTotals::default(),
            risk: RiskRollup {
                low: 0,
                medium: 0,
                high: 0,
            },
            warnings: vec![],
            operations: vec![],
        };
        let repo = SqliteCleanupPlanRepo::new(db.clone());
        repo.save(&plan).await.expect("seed plan");
        plan.id
    }

    fn sample_job(plan_id: PlanId, started_at: chrono::DateTime<Utc>) -> CleanupApplyJob {
        CleanupApplyJob {
            job_id: Uuid::now_v7(),
            plan_id,
            started_at,
            finished_at: None,
            state: JobState::Queued,
            risk_max: RiskMax::Low,
            counts: JobCounts {
                applied: 0,
                failed: 0,
                skipped: 0,
                pending: 3,
                skipped_by_reason: std::collections::BTreeMap::new(),
            },
        }
    }

    #[tokio::test]
    async fn create_load_round_trip() {
        let db = fresh_pool().await;
        let plan_id = seed_plan(&db, "user-job-rt").await;
        let repo = SqliteCleanupApplyJobRepo::new(db);
        let job = sample_job(plan_id, Utc::now());

        repo.create(&job).await.expect("create");
        let loaded = repo.load(job.job_id).await.expect("load").expect("present");

        assert_eq!(loaded.job_id, job.job_id);
        assert_eq!(loaded.plan_id, plan_id);
        assert_eq!(
            loaded.started_at.timestamp_millis(),
            job.started_at.timestamp_millis()
        );
        assert!(loaded.finished_at.is_none());
        assert_eq!(loaded.state, JobState::Queued);
        assert!(matches!(loaded.risk_max, RiskMax::Low));
        assert_eq!(loaded.counts.pending, 3);
    }

    #[tokio::test]
    async fn load_absent_returns_none() {
        let db = fresh_pool().await;
        let repo = SqliteCleanupApplyJobRepo::new(db);
        assert!(repo.load(Uuid::now_v7()).await.expect("load").is_none());
    }

    #[tokio::test]
    async fn update_state_transitions_state_counts_and_finished_at() {
        let db = fresh_pool().await;
        let plan_id = seed_plan(&db, "user-job-upd").await;
        let repo = SqliteCleanupApplyJobRepo::new(db);
        let job = sample_job(plan_id, Utc::now());
        repo.create(&job).await.expect("create");

        let finished = Utc::now();
        let counts = JobCounts {
            applied: 3,
            failed: 0,
            skipped: 0,
            pending: 0,
            skipped_by_reason: std::collections::BTreeMap::new(),
        };
        repo.update_state(job.job_id, JobState::Finished, counts, Some(finished))
            .await
            .expect("update");

        let loaded = repo.load(job.job_id).await.expect("load").expect("present");
        assert_eq!(loaded.state, JobState::Finished);
        assert_eq!(loaded.counts.applied, 3);
        assert_eq!(loaded.counts.pending, 0);
        assert_eq!(
            loaded.finished_at.map(|t| t.timestamp_millis()),
            Some(finished.timestamp_millis())
        );
    }

    #[tokio::test]
    async fn update_state_on_absent_job_is_silent_noop() {
        let db = fresh_pool().await;
        let repo = SqliteCleanupApplyJobRepo::new(db);
        let counts = JobCounts {
            applied: 0,
            failed: 0,
            skipped: 0,
            pending: 0,
            skipped_by_reason: std::collections::BTreeMap::new(),
        };
        repo.update_state(Uuid::now_v7(), JobState::Failed, counts, None)
            .await
            .expect("absent-job update must not error");
    }

    #[tokio::test]
    async fn list_by_plan_orders_by_started_at_desc_and_scopes_to_plan() {
        let db = fresh_pool().await;
        let plan_a = seed_plan(&db, "user-job-list-a").await;
        let plan_b = seed_plan(&db, "user-job-list-b").await;
        let repo = SqliteCleanupApplyJobRepo::new(db);

        let t0 = Utc::now() - Duration::minutes(10);
        let t1 = Utc::now();
        let early = sample_job(plan_a, t0);
        let late = sample_job(plan_a, t1);
        let other = sample_job(plan_b, t1);
        repo.create(&early).await.expect("create early");
        repo.create(&late).await.expect("create late");
        repo.create(&other).await.expect("create other");

        let jobs = repo.list_by_plan(plan_a).await.expect("list");
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_id, late.job_id);
        assert_eq!(jobs[1].job_id, early.job_id);
    }
}
