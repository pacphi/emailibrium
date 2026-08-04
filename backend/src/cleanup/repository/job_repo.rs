//! `CleanupApplyJobRepository` (Phase C will drive; Phase A persists shape only).
//!
//! Repository bodies are single-code-path SeaORM (ADR-036) — see
//! `plan_repo.rs`'s module doc; this file follows the same exemplar pattern.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder,
};
use uuid::Uuid;

use crate::cleanup::domain::operation::{JobState, RiskMax};
use crate::cleanup::domain::plan::{CleanupApplyJob, JobCounts, JobId, PlanId};
use crate::cleanup::domain::ports::RepoError;
use crate::db::entities::cleanup_apply_jobs as jobs;

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

pub struct SeaOrmCleanupApplyJobRepo {
    conn: DatabaseConnection,
}

impl SeaOrmCleanupApplyJobRepo {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CleanupApplyJobRepository for SeaOrmCleanupApplyJobRepo {
    async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError> {
        let counts =
            serde_json::to_string(&job.counts).map_err(|e| RepoError::Internal(e.to_string()))?;
        let risk_str = match job.risk_max {
            RiskMax::Low => "low",
            RiskMax::Medium => "medium",
            RiskMax::High => "high",
        };
        jobs::ActiveModel {
            job_id: Set(job.job_id.as_bytes().to_vec()),
            plan_id: Set(job.plan_id.as_bytes().to_vec()),
            started_at: Set(job.started_at.timestamp_millis()),
            finished_at: Set(job.finished_at.map(|t| t.timestamp_millis())),
            state: Set(job.state.as_str().to_owned()),
            risk_max: Set(risk_str.to_owned()),
            counts_json: Set(counts),
        }
        .insert(&self.conn)
        .await?;
        Ok(())
    }

    async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError> {
        let row = jobs::Entity::find_by_id(job_id.as_bytes().to_vec())
            .one(&self.conn)
            .await?;
        row.map(map_job_model).transpose()
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
        // update_many (not ActiveModel::update) so an absent job stays a silent
        // zero-rows no-op, exactly as the old UPDATE behaved.
        jobs::Entity::update_many()
            .col_expr(jobs::Column::State, Expr::value(state.as_str()))
            .col_expr(jobs::Column::CountsJson, Expr::value(counts_s))
            .col_expr(
                jobs::Column::FinishedAt,
                Expr::value(finished_at.map(|t| t.timestamp_millis())),
            )
            .filter(jobs::Column::JobId.eq(job_id.as_bytes().to_vec()))
            .exec(&self.conn)
            .await?;
        Ok(())
    }

    async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError> {
        let rows = jobs::Entity::find()
            .filter(jobs::Column::PlanId.eq(plan_id.as_bytes().to_vec()))
            .order_by_desc(jobs::Column::StartedAt)
            .all(&self.conn)
            .await?;
        rows.into_iter().map(map_job_model).collect()
    }
}

/// Decode a `cleanup_apply_jobs` entity row into a `CleanupApplyJob`. The
/// entity's declared types (not a backend-concrete `Row`) are what make this
/// portable across SQLite and PostgreSQL — see ADR-036.
fn map_job_model(row: jobs::Model) -> Result<CleanupApplyJob, RepoError> {
    let job_id = Uuid::from_slice(&row.job_id)
        .map_err(|e| RepoError::Internal(format!("bad job_id: {e}")))?;
    let plan_id = Uuid::from_slice(&row.plan_id)
        .map_err(|e| RepoError::Internal(format!("bad plan_id: {e}")))?;

    let started_at = DateTime::from_timestamp_millis(row.started_at)
        .ok_or_else(|| RepoError::Internal("bad started_at".into()))?;

    let finished_at = row.finished_at.and_then(DateTime::from_timestamp_millis);

    let state = JobState::from_str_opt(&row.state)
        .ok_or_else(|| RepoError::Internal(format!("bad job state: {}", row.state)))?;

    let risk_max = match row.risk_max.as_str() {
        "medium" => RiskMax::Medium,
        "high" => RiskMax::High,
        _ => RiskMax::Low,
    };

    let counts: JobCounts =
        serde_json::from_str(&row.counts_json).map_err(|e| RepoError::Internal(e.to_string()))?;

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
    use crate::cleanup::repository::plan_repo::{CleanupPlanRepository, SeaOrmCleanupPlanRepo};
    use crate::db::Database;
    use chrono::{Duration, Utc};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    async fn fresh_db() -> Database {
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
        let repo = SeaOrmCleanupPlanRepo::new(db.sea_orm());
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
        let db = fresh_db().await;
        let plan_id = seed_plan(&db, "user-job-rt").await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());
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
        let db = fresh_db().await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());
        assert!(repo.load(Uuid::now_v7()).await.expect("load").is_none());
    }

    #[tokio::test]
    async fn update_state_transitions_state_counts_and_finished_at() {
        let db = fresh_db().await;
        let plan_id = seed_plan(&db, "user-job-upd").await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());
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
        let db = fresh_db().await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());
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
        let db = fresh_db().await;
        let plan_a = seed_plan(&db, "user-job-list-a").await;
        let plan_b = seed_plan(&db, "user-job-list-b").await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());

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

    /// Live-PostgreSQL job lifecycle verification (ADR-036 exemplar proof).
    /// Skips when `EMAILIBRIUM_TEST_PG_URL` is unset — reproduction command in
    /// `plan_repo.rs`'s `postgres_full_trait_round_trip` doc.
    #[tokio::test]
    async fn postgres_job_lifecycle_round_trip() {
        let Ok(url) = std::env::var("EMAILIBRIUM_TEST_PG_URL") else {
            eprintln!("skipping postgres_job_lifecycle_round_trip: EMAILIBRIUM_TEST_PG_URL unset");
            return;
        };
        let db = Database::connect(&url).await.expect("pg connect");
        db.run_migrations().await.expect("pg migrations");
        let plan_id = seed_plan(&db, &format!("pg-job-user-{}", Uuid::new_v4())).await;
        let repo = SeaOrmCleanupApplyJobRepo::new(db.sea_orm());

        let job = sample_job(plan_id, Utc::now());
        repo.create(&job).await.expect("create");
        let loaded = repo.load(job.job_id).await.expect("load").expect("present");
        assert_eq!(loaded.state, JobState::Queued);
        assert_eq!(loaded.counts.pending, 3);

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
        assert_eq!(
            loaded.finished_at.map(|t| t.timestamp_millis()),
            Some(finished.timestamp_millis())
        );

        // Absent-job update stays a silent no-op on PostgreSQL too.
        repo.update_state(
            Uuid::now_v7(),
            JobState::Failed,
            JobCounts {
                applied: 0,
                failed: 0,
                skipped: 0,
                pending: 0,
                skipped_by_reason: std::collections::BTreeMap::new(),
            },
            None,
        )
        .await
        .expect("absent-job update must not error");

        let listed = repo.list_by_plan(plan_id).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].job_id, job.job_id);
    }
}
