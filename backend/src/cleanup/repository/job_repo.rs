//! `CleanupApplyJobRepository` (Phase C will drive; Phase A persists shape only).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::cleanup::domain::operation::JobState;
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

    async fn load(&self, _job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError> {
        // Phase C will fully implement — Phase A only needs the table to exist.
        Ok(None)
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

    async fn list_by_plan(&self, _plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError> {
        Ok(Vec::new())
    }
}
