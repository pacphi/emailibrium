//! `CleanupApplyJobRepository` (Phase C will drive; Phase A persists shape only).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::cleanup::domain::operation::{JobState, RiskMax};
use crate::cleanup::domain::plan::{CleanupApplyJob, JobCounts, JobId, PlanId};
use crate::cleanup::domain::ports::RepoError;

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
    pool: SqlitePool,
}

impl SqliteCleanupApplyJobRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CleanupApplyJobRepository for SqliteCleanupApplyJobRepo {
    async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError> {
        let counts =
            serde_json::to_string(&job.counts).map_err(|e| RepoError::Internal(e.to_string()))?;
        sqlx::query(
            r#"INSERT INTO cleanup_apply_jobs
               (job_id, plan_id, started_at, finished_at, state, risk_max, counts_json)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(job.job_id.as_bytes().to_vec())
        .bind(job.plan_id.as_bytes().to_vec())
        .bind(job.started_at.timestamp_millis())
        .bind(job.finished_at.map(|t| t.timestamp_millis()))
        .bind(job.state.as_str())
        .bind(match job.risk_max {
            crate::cleanup::domain::operation::RiskMax::Low => "low",
            crate::cleanup::domain::operation::RiskMax::Medium => "medium",
            crate::cleanup::domain::operation::RiskMax::High => "high",
        })
        .bind(counts)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError> {
        let row = sqlx::query(
            r#"SELECT job_id, plan_id, started_at, finished_at, state, risk_max, counts_json
               FROM cleanup_apply_jobs WHERE job_id = ?"#,
        )
        .bind(job_id.as_bytes().to_vec())
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = row else {
            return Ok(None);
        };

        let job = map_job_row(&r)?;
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
        sqlx::query(
            r#"UPDATE cleanup_apply_jobs
               SET state = ?, counts_json = ?, finished_at = ?
               WHERE job_id = ?"#,
        )
        .bind(state.as_str())
        .bind(counts_s)
        .bind(finished_at.map(|t| t.timestamp_millis()))
        .bind(job_id.as_bytes().to_vec())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError> {
        let rows = sqlx::query(
            r#"SELECT job_id, plan_id, started_at, finished_at, state, risk_max, counts_json
               FROM cleanup_apply_jobs WHERE plan_id = ?
               ORDER BY started_at DESC"#,
        )
        .bind(plan_id.as_bytes().to_vec())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(map_job_row).collect()
    }
}

fn map_job_row(r: &sqlx::sqlite::SqliteRow) -> Result<CleanupApplyJob, RepoError> {
    let job_id_bytes: Vec<u8> = r.get("job_id");
    let plan_id_bytes: Vec<u8> = r.get("plan_id");

    let job_id = Uuid::from_slice(&job_id_bytes)
        .map_err(|e| RepoError::Internal(format!("bad job_id: {e}")))?;
    let plan_id = Uuid::from_slice(&plan_id_bytes)
        .map_err(|e| RepoError::Internal(format!("bad plan_id: {e}")))?;

    let started_ms: i64 = r.get("started_at");
    let started_at = DateTime::from_timestamp_millis(started_ms)
        .ok_or_else(|| RepoError::Internal("bad started_at".into()))?;

    let finished_ms: Option<i64> = r.get("finished_at");
    let finished_at = finished_ms.and_then(DateTime::from_timestamp_millis);

    let state_s: String = r.get("state");
    let state = JobState::from_str_opt(&state_s)
        .ok_or_else(|| RepoError::Internal(format!("bad job state: {state_s}")))?;

    let risk_s: String = r.get("risk_max");
    let risk_max = match risk_s.as_str() {
        "medium" => RiskMax::Medium,
        "high" => RiskMax::High,
        _ => RiskMax::Low,
    };

    let counts_s: String = r.get("counts_json");
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
