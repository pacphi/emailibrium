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
