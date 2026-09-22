//! `ApplyOrchestrator` — the cleanup-apply entry point (Phase C).
//!
//! Lifecycle:
//! 1. `begin_apply` validates the plan + runs drift detection. Hard drift
//!    on any account → 409.
//! 2. Creates a `cleanup_apply_jobs` row (queued) and a broadcast channel
//!    keyed by `job_id`.
//! 3. Spawns one tokio task per account. Each task runs an
//!    [`AccountWorker`].
//! 4. Aggregates `JobCounts` across workers; on completion updates the
//!    job + plan row, emits `Finished`, drops the broadcast::Sender.
//!
//! ## Re-apply semantics
//!
//! Re-issuing apply on a plan that has rows skipped with
//! `SkipReason::UserCancelled` does NOT retry those rows — only `pending`
//! rows are walked. To retry user-cancelled rows the caller must rebuild
//! the plan. This matches the absence of a distinct `Cancelled` variant
//! on `OperationStatus`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::cleanup::audit::{CleanupAuditWriter, NoopCleanupAuditWriter};
use crate::cleanup::domain::operation::{
    JobState, OperationStatus, PlanStatus, PlannedOperation, PredicateStatus, Provider, RiskMax,
};
use crate::cleanup::domain::plan::{CleanupApplyJob, CleanupPlan, JobCounts, JobId, PlanId};
use crate::cleanup::repository::{CleanupApplyJobRepository, CleanupPlanRepository};
use crate::cleanup::telemetry::{hash_user_id, CleanupTelemetryEvent, TelemetryEmitter};
use crate::email::unsubscribe::UnsubscribeService;

use super::account_worker::{AccountWorker, AccountWorkerCtx};
use super::drift::{DriftDetector, DriftStatus};
use super::expander::PredicateExpander;
use super::factory::{EmailProviderFactory, MockEmailProviderFactory};
use super::sse::{AccountSnapshotState, ApplyEvent, EventEmitter};

#[derive(Debug, Error)]
pub enum BeginApplyError {
    #[error("plan not in an applyable state: {0:?}")]
    BadStatus(PlanStatus),
    #[error("plan expired")]
    Expired,
    #[error("plan was already claimed or changed since it was loaded")]
    ClaimConflict,
    #[error("hard drift on accounts: {0:?}")]
    HardDrift(Vec<String>),
    #[error("repo: {0}")]
    Repo(#[from] crate::cleanup::domain::ports::RepoError),
    #[error("drift: {0}")]
    Drift(#[from] super::drift::DriftError),
}

#[derive(Debug, Error)]
pub enum CancelError {
    #[error("not found")]
    NotFound,
}

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub risk_max: RiskMax,
    pub acknowledged_high_risk_seqs: Vec<u64>,
    pub acknowledged_medium_groups: Vec<String>,
}

struct JobChannels {
    sender: broadcast::Sender<ApplyEvent>,
    cancel: CancellationToken,
}

pub struct ApplyOrchestrator {
    pub plan_repo: Arc<dyn CleanupPlanRepository>,
    pub job_repo: Arc<dyn CleanupApplyJobRepository>,
    pub drift: Arc<DriftDetector>,
    pub expander: Arc<PredicateExpander>,
    pub workers_for: Arc<dyn Fn(&str) -> Provider + Send + Sync>,
    /// Per-account EmailProvider factory (Item #1). Defaults to an unavailable
    /// provider; production wiring installs `OAuthEmailProviderFactory`.
    pub provider_factory: Arc<dyn EmailProviderFactory>,
    pub unsubscribe: Arc<UnsubscribeService>,
    /// Per-operation audit writer (Phase D, ADR-030 §Security).
    pub audit: Arc<dyn CleanupAuditWriter>,
    /// Telemetry emitter (Phase D).
    pub telemetry: Arc<TelemetryEmitter>,
    /// Optional DB handle — when present, workers update `is_archived` locally
    /// after a successful provider archive so the Archive view stays in sync.
    pub db: Option<crate::db::Database>,
    /// Active jobs keyed by job_id, exposing the broadcast::Sender + cancel token.
    job_channels: Arc<RwLock<HashMap<JobId, JobChannels>>>,
}

impl ApplyOrchestrator {
    pub fn new(
        plan_repo: Arc<dyn CleanupPlanRepository>,
        job_repo: Arc<dyn CleanupApplyJobRepository>,
        drift: Arc<DriftDetector>,
        expander: Arc<PredicateExpander>,
        workers_for: Arc<dyn Fn(&str) -> Provider + Send + Sync>,
        unsubscribe: Arc<UnsubscribeService>,
    ) -> Self {
        Self {
            plan_repo,
            job_repo,
            drift,
            expander,
            workers_for,
            provider_factory: Arc::new(MockEmailProviderFactory::no_op())
                as Arc<dyn EmailProviderFactory>,
            unsubscribe,
            audit: Arc::new(NoopCleanupAuditWriter) as Arc<dyn CleanupAuditWriter>,
            telemetry: Arc::new(TelemetryEmitter::new()),
            db: None,
            job_channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Builder-style setter for the local DB handle (used to sync archive state).
    pub fn with_db(mut self, db: crate::db::Database) -> Self {
        self.db = Some(db);
        self
    }

    /// Builder-style setter for the EmailProvider factory (Item #1).
    pub fn with_provider_factory(mut self, factory: Arc<dyn EmailProviderFactory>) -> Self {
        self.provider_factory = factory;
        self
    }

    /// Builder-style setter for the audit writer (Phase D wiring from main.rs).
    pub fn with_audit(mut self, audit: Arc<dyn CleanupAuditWriter>) -> Self {
        self.audit = audit;
        self
    }

    /// Builder-style setter for the telemetry emitter (Phase D).
    pub fn with_telemetry(mut self, telemetry: Arc<TelemetryEmitter>) -> Self {
        self.telemetry = telemetry;
        self
    }

    pub async fn begin_apply(
        self: Arc<Self>,
        plan: &CleanupPlan,
        opts: ApplyOptions,
    ) -> Result<JobId, BeginApplyError> {
        // Status gate.
        match plan.status {
            PlanStatus::Ready | PlanStatus::PartiallyApplied => {}
            other => return Err(BeginApplyError::BadStatus(other)),
        }
        // TTL gate.
        if Utc::now() >= plan.valid_until {
            return Err(BeginApplyError::Expired);
        }

        // Drift gate (per ADR-030 §8 / DDD-008 addendum).
        let drift_map = self.drift.detect_all(plan).await?;
        let hard_accounts: Vec<String> = drift_map
            .iter()
            .filter(|(_, v)| matches!(v, DriftStatus::Hard { .. }))
            .map(|(k, _)| k.clone())
            .collect();
        if !hard_accounts.is_empty() {
            return Err(BeginApplyError::HardDrift(hard_accounts));
        }

        // This persisted compare-and-set is the concurrency boundary across
        // independent server instances, not merely this process's channel map.
        if !self.plan_repo.claim_apply(plan, Utc::now()).await? {
            return Err(BeginApplyError::ClaimConflict);
        }

        // Build per-account totals for Started event.
        let mut totals_by_account: BTreeMap<String, JobCounts> = BTreeMap::new();
        for op in &plan.operations {
            if !opts.risk_max.includes(op.risk()) {
                continue;
            }
            let entry = totals_by_account
                .entry(op.account_id().to_string())
                .or_default();
            entry.pending = entry.pending.saturating_add(1);
        }

        // Job + channel.
        let job_id: JobId = Uuid::now_v7();
        let (emitter, sender) = EventEmitter::new(1024);
        let cancel = CancellationToken::new();

        // Persist queued job.
        let job_row = CleanupApplyJob {
            job_id,
            plan_id: plan.id,
            started_at: Utc::now(),
            finished_at: None,
            state: JobState::Running,
            risk_max: opts.risk_max,
            counts: JobCounts::default(),
        };
        if let Err(error) = self.job_repo.create(&job_row).await {
            self.plan_repo.release_apply(plan.id, plan.status).await?;
            return Err(error.into());
        }
        {
            let mut guard = self.job_channels.write().await;
            guard.insert(
                job_id,
                JobChannels {
                    sender: sender.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        // Emit Started.
        emitter.emit(ApplyEvent::Started {
            job_id,
            plan_id: plan.id,
            totals_by_account: totals_by_account.clone(),
        });

        // Phase D telemetry: cleanup_apply_started — counts only.
        self.telemetry
            .emit(CleanupTelemetryEvent::CleanupApplyStarted {
                plan_id: plan.id,
                job_id,
                user_id_hash: hash_user_id(&plan.user_id),
                risk_max: opts.risk_max,
                ack_high_count: opts.acknowledged_high_risk_seqs.len() as u64,
                ack_medium_count: opts.acknowledged_medium_groups.len() as u64,
            });
        let apply_started_at = Utc::now();

        // Spawn the orchestrator driver task.
        let acked_high: HashSet<u64> = opts.acknowledged_high_risk_seqs.iter().copied().collect();
        let acked_med: HashSet<String> = opts.acknowledged_medium_groups.iter().cloned().collect();

        let plan_id = plan.id;
        let accounts: Vec<String> = totals_by_account.keys().cloned().collect();
        let user_id_for_audit = plan.user_id.clone();
        let user_id_for_telemetry = plan.user_id.clone();
        let me = self.clone();
        let emitter_outer = emitter.clone();
        tokio::spawn(async move {
            let mut handles = Vec::with_capacity(accounts.len());
            for account_id in accounts {
                let provider = (me.workers_for)(&account_id);
                let ctx = AccountWorkerCtx {
                    repo: me.plan_repo.clone(),
                    provider_factory: me.provider_factory.clone(),
                    unsubscribe: me.unsubscribe.clone(),
                    expander: me.expander.clone(),
                    emitter: emitter_outer.clone(),
                    audit: me.audit.clone(),
                    user_id: user_id_for_audit.clone(),
                    job_id,
                    db: me.db.clone(),
                };
                let worker = AccountWorker {
                    account_id: account_id.clone(),
                    provider,
                    ctx,
                };
                let acked_high = acked_high.clone();
                let acked_med = acked_med.clone();
                let cancel = cancel.clone();
                let risk_max = opts.risk_max;
                handles.push(tokio::spawn(async move {
                    worker
                        .run(plan_id, risk_max, acked_high, acked_med, cancel)
                        .await
                }));
            }

            let mut combined = JobCounts::default();
            let mut any_failed = false;
            for h in handles {
                match h.await {
                    Ok(Ok(counts)) => merge_counts(&mut combined, &counts),
                    Ok(Err(_e)) => {
                        any_failed = true;
                    }
                    Err(_) => {
                        any_failed = true;
                    }
                }
            }

            // Read the authoritative rows, including children appended during
            // predicate expansion. Never save the request's stale aggregate.
            let mut unresolved_failures = false;
            match me.plan_repo.load(&user_id_for_audit, plan_id).await {
                Ok(Some(current)) => {
                    combined.pending = current
                        .operations
                        .iter()
                        .filter(|op| match op {
                            PlannedOperation::Materialized(row) => {
                                row.status == OperationStatus::Pending
                            }
                            PlannedOperation::Predicate(row) => matches!(
                                row.status,
                                PredicateStatus::Pending
                                    | PredicateStatus::Expanding
                                    | PredicateStatus::PartiallyApplied
                            ),
                        })
                        .count() as u64;
                    unresolved_failures = current.operations.iter().any(|op| match op {
                        PlannedOperation::Materialized(row) => {
                            row.status == OperationStatus::Failed
                        }
                        PlannedOperation::Predicate(row) => row.status == PredicateStatus::Failed,
                    });
                }
                Ok(None) => any_failed = true,
                Err(error) => {
                    tracing::error!(%error, "could not inspect cleanup plan after apply");
                    any_failed = true;
                }
            }
            let mut final_state = if cancel.is_cancelled() {
                JobState::Cancelled
            } else if any_failed || combined.failed > 0 || unresolved_failures {
                JobState::Failed
            } else {
                JobState::Finished
            };
            let plan_status = if final_state == JobState::Finished && combined.pending == 0 {
                PlanStatus::Applied
            } else {
                PlanStatus::PartiallyApplied
            };
            if let Err(error) = me.plan_repo.release_apply(plan_id, plan_status).await {
                tracing::error!(%error, "could not release cleanup apply claim");
                final_state = JobState::Failed;
            }

            let now = Utc::now();
            if let Err(error) = me
                .job_repo
                .update_state(job_id, final_state, combined.clone(), Some(now))
                .await
            {
                tracing::error!(%error, "could not persist cleanup job result");
                final_state = JobState::Failed;
            }

            emitter_outer.emit_progress_now(combined.clone());
            emitter_outer.emit(ApplyEvent::Finished {
                job_id,
                status: final_state,
                counts: combined.clone(),
            });

            // Phase D telemetry: cleanup_apply_finished.
            let duration_ms = (Utc::now() - apply_started_at).num_milliseconds().max(0) as u64;
            me.telemetry
                .emit(CleanupTelemetryEvent::CleanupApplyFinished {
                    plan_id,
                    job_id,
                    user_id_hash: hash_user_id(&user_id_for_telemetry),
                    applied: combined.applied,
                    failed: combined.failed,
                    skipped: combined.skipped,
                    skipped_by_reason: combined.skipped_by_reason.clone(),
                    duration_ms,
                    status: final_state,
                });

            // Remove channel from active map.
            let mut guard = me.job_channels.write().await;
            guard.remove(&job_id);
        });

        Ok(job_id)
    }

    pub async fn cancel(&self, job_id: JobId) -> Result<(), CancelError> {
        let guard = self.job_channels.read().await;
        match guard.get(&job_id) {
            Some(ch) => {
                ch.cancel.cancel();
                Ok(())
            }
            None => Err(CancelError::NotFound),
        }
    }

    /// Subscribe to the SSE event stream for a job. Returns `None` if the
    /// job has already finished and its channel was dropped.
    pub async fn subscribe(&self, job_id: JobId) -> Option<broadcast::Receiver<ApplyEvent>> {
        let guard = self.job_channels.read().await;
        guard.get(&job_id).map(|c| c.sender.subscribe())
    }

    /// Build a [`ApplyEvent::Snapshot`] for a given job_id by reading the
    /// latest persisted job row. Returns `None` if no such job exists.
    /// Intended to be emitted as the FIRST event after a new subscriber
    /// connects.
    pub async fn build_snapshot(&self, job_id: JobId) -> Option<ApplyEvent> {
        let job = self.job_repo.load(job_id).await.ok().flatten()?;
        // Account states are not persisted today; default empty.
        let account_states: BTreeMap<String, AccountSnapshotState> = BTreeMap::new();
        Some(ApplyEvent::Snapshot {
            job_id: job.job_id,
            plan_id: job.plan_id,
            counts: job.counts,
            account_states,
        })
    }

    /// True iff a job is currently running for the given plan_id (used by
    /// the refresh handler to gate destructive replace-account-rows).
    pub async fn is_running_for_plan(&self, plan_id: PlanId) -> bool {
        let guard = self.job_channels.read().await;
        // Jobs in `job_channels` are by-construction running. We still
        // need to confirm the plan_id matches; since channel keys are job
        // ids, we'd have to query the job repo. To avoid an extra round
        // trip we use the simpler invariant: any active channel implies
        // some apply is running. Callers MUST also check the per-plan
        // job repo if they need exactness.
        !guard.is_empty() && {
            // Best-effort: ask the repo for jobs on this plan.
            drop(guard);
            match self.job_repo.list_by_plan(plan_id).await {
                Ok(jobs) => jobs.iter().any(|j| matches!(j.state, JobState::Running)),
                Err(_) => false,
            }
        }
    }
}

fn merge_counts(into: &mut JobCounts, src: &JobCounts) {
    into.applied = into.applied.saturating_add(src.applied);
    into.failed = into.failed.saturating_add(src.failed);
    into.skipped = into.skipped.saturating_add(src.skipped);
    into.pending = into.pending.saturating_add(src.pending);
    for (k, v) in &src.skipped_by_reason {
        let entry = into.skipped_by_reason.entry(*k).or_insert(0);
        *entry = entry.saturating_add(*v);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::cleanup::domain::operation::{
        AccountStateEtag, OperationStatus, PlanAction, PlanSource, PlannedOperation,
        PlannedOperationRow, RiskLevel, RiskMax,
    };
    use crate::cleanup::domain::plan::{CleanupPlan, PlanTotals, RiskRollup};
    use crate::cleanup::domain::ports::{AccountStateProvider, EmailRepository, RepoError};
    use crate::cleanup::repository::{CleanupPlanRepository, OpsFilter};

    // --- Mock email repo --------------------------------------------------

    struct StubEmailRepo;

    #[async_trait]
    impl EmailRepository for StubEmailRepo {
        async fn list_by_account(
            &self,
            _account_id: &str,
        ) -> Result<Vec<crate::cleanup::domain::operation::EmailRef>, RepoError> {
            Ok(Vec::new())
        }
        async fn list_by_cluster(
            &self,
            _cluster_id: &str,
        ) -> Result<Vec<crate::cleanup::domain::operation::EmailRef>, RepoError> {
            Ok(Vec::new())
        }
        async fn count_by_account(&self, _account_id: &str) -> Result<u64, RepoError> {
            Ok(0)
        }
    }

    struct StubRules;

    #[async_trait]
    impl crate::cleanup::domain::ports::RuleEvaluator for StubRules {
        async fn evaluate_scope(
            &self,
            _mode: crate::rules::types::RuleExecutionMode,
            _scope: crate::rules::types::EvaluationScope,
        ) -> Result<
            Vec<crate::rules::types::RuleEvaluation>,
            crate::cleanup::domain::ports::RuleEvalError,
        > {
            Ok(Vec::new())
        }
    }

    // --- Mock plan repo ---------------------------------------------------

    #[derive(Default)]
    struct InMemPlanRepo {
        plan: Mutex<Option<CleanupPlan>>,
        status_log: Mutex<Vec<(u64, OperationStatus)>>,
        cursor_backstep: Option<u64>,
        empty_cursor_regression: bool,
    }

    #[async_trait]
    impl CleanupPlanRepository for InMemPlanRepo {
        async fn claim_apply(
            &self,
            expected: &CleanupPlan,
            now: chrono::DateTime<Utc>,
        ) -> Result<bool, RepoError> {
            let mut stored = self.plan.lock().unwrap();
            let Some(plan) = stored.as_mut() else {
                return Ok(false);
            };
            if matches!(
                plan.status,
                PlanStatus::Ready | PlanStatus::PartiallyApplied
            ) && plan.id == expected.id
                && plan.user_id == expected.user_id
                && plan.plan_hash == expected.plan_hash
                && plan.status == expected.status
                && plan.valid_until > now
            {
                plan.status = PlanStatus::Applying;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        async fn release_apply(&self, id: PlanId, status: PlanStatus) -> Result<(), RepoError> {
            if let Some(plan) = self.plan.lock().unwrap().as_mut() {
                if plan.id == id && plan.status == PlanStatus::Applying {
                    plan.status = status;
                }
            }
            Ok(())
        }
        async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError> {
            *self.plan.lock().unwrap() = Some(plan.clone());
            Ok(())
        }
        async fn load(
            &self,
            _user_id: &str,
            _id: PlanId,
        ) -> Result<Option<CleanupPlan>, RepoError> {
            Ok(self.plan.lock().unwrap().clone())
        }
        async fn list_by_user(
            &self,
            _u: &str,
            _s: Option<PlanStatus>,
            _l: u32,
        ) -> Result<Vec<crate::cleanup::domain::plan::CleanupPlanSummary>, RepoError> {
            Ok(Vec::new())
        }
        async fn list_operations(
            &self,
            _id: PlanId,
            filter: OpsFilter,
            _cursor: Option<u64>,
            _limit: u32,
        ) -> Result<(Vec<PlannedOperation>, Option<u64>), RepoError> {
            let plan = self.plan.lock().unwrap();
            let ops: Vec<PlannedOperation> = plan
                .as_ref()
                .map(|p| {
                    p.operations
                        .iter()
                        .filter(|o| {
                            filter
                                .account_id
                                .as_deref()
                                .is_none_or(|a| o.account_id() == a)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            if self.empty_cursor_regression && _cursor.is_some() {
                return if _cursor == Some(1) {
                    Ok((Vec::new(), Some(0)))
                } else {
                    Ok((ops, None))
                };
            }
            let next_cursor = self.cursor_backstep.map(|backstep| {
                _cursor
                    .map(|cursor| cursor.saturating_sub(backstep))
                    .unwrap_or(1)
            });
            Ok((ops, next_cursor))
        }
        async fn sample_operations(
            &self,
            _id: PlanId,
            _s: &str,
            _n: u32,
        ) -> Result<Vec<String>, RepoError> {
            Ok(Vec::new())
        }
        async fn replace_account_rows(
            &self,
            _id: PlanId,
            _account: &str,
            _rows: Vec<PlannedOperation>,
        ) -> Result<(), RepoError> {
            Ok(())
        }
        async fn append_operations(
            &self,
            _id: PlanId,
            rows: Vec<PlannedOperation>,
        ) -> Result<(), RepoError> {
            if let Some(p) = self.plan.lock().unwrap().as_mut() {
                for op in rows {
                    p.operations.push(op);
                }
            }
            Ok(())
        }
        async fn max_seq(&self, _id: PlanId) -> Result<u64, RepoError> {
            Ok(self
                .plan
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.operations.iter().map(|o| o.seq()).max().unwrap_or(0))
                .unwrap_or(0))
        }
        async fn update_predicate_status(
            &self,
            _id: PlanId,
            seq: u64,
            status: crate::cleanup::domain::operation::PredicateStatus,
        ) -> Result<(), RepoError> {
            if let Some(p) = self.plan.lock().unwrap().as_mut() {
                for op in p.operations.iter_mut() {
                    if let PlannedOperation::Predicate(pr) = op {
                        if pr.seq == seq {
                            pr.status = status;
                        }
                    }
                }
            }
            Ok(())
        }
        async fn update_operation_status(
            &self,
            _id: PlanId,
            seq: u64,
            status: OperationStatus,
            _ts: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), RepoError> {
            self.status_log.lock().unwrap().push((seq, status));
            // Reflect status in the in-memory plan so re-issues see it.
            if let Some(p) = self.plan.lock().unwrap().as_mut() {
                for op in p.operations.iter_mut() {
                    if let PlannedOperation::Materialized(r) = op {
                        if r.seq == seq {
                            r.status = status;
                        }
                    }
                }
            }
            Ok(())
        }
        async fn cancel(&self, _id: PlanId) -> Result<(), RepoError> {
            Ok(())
        }
        async fn expire_due(&self, _now: chrono::DateTime<chrono::Utc>) -> Result<u32, RepoError> {
            Ok(0)
        }
        async fn purge_older_than(
            &self,
            _c: chrono::DateTime<chrono::Utc>,
        ) -> Result<u32, RepoError> {
            Ok(0)
        }
    }

    // --- Mock job repo ----------------------------------------------------

    #[derive(Default)]
    struct InMemJobRepo {
        jobs: Mutex<Vec<CleanupApplyJob>>,
    }

    #[async_trait]
    impl CleanupApplyJobRepository for InMemJobRepo {
        async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError> {
            self.jobs.lock().unwrap().push(job.clone());
            Ok(())
        }
        async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .find(|j| j.job_id == job_id)
                .cloned())
        }
        async fn update_state(
            &self,
            job_id: JobId,
            state: JobState,
            counts: JobCounts,
            finished_at: Option<chrono::DateTime<chrono::Utc>>,
        ) -> Result<(), RepoError> {
            for j in self.jobs.lock().unwrap().iter_mut() {
                if j.job_id == job_id {
                    j.state = state;
                    j.counts = counts.clone();
                    j.finished_at = finished_at;
                }
            }
            Ok(())
        }
        async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .filter(|j| j.plan_id == plan_id)
                .cloned()
                .collect())
        }
    }

    // --- Mock account-state provider --------------------------------------

    struct CleanProvider;

    #[async_trait]
    impl AccountStateProvider for CleanProvider {
        async fn etag(&self, _account_id: &str) -> Result<AccountStateEtag, RepoError> {
            Ok(AccountStateEtag::None)
        }
    }

    struct HardDriftProvider;

    #[async_trait]
    impl AccountStateProvider for HardDriftProvider {
        async fn etag(&self, _account_id: &str) -> Result<AccountStateEtag, RepoError> {
            // Returning a different kind than the baseline triggers Hard.
            Ok(AccountStateEtag::None)
        }
    }

    // --- Helpers ----------------------------------------------------------

    fn sample_plan_with_rows(rows: Vec<PlannedOperation>) -> CleanupPlan {
        let now = Utc::now();
        let mut etags = std::collections::BTreeMap::new();
        etags.insert("acct-a".into(), AccountStateEtag::None);
        CleanupPlan {
            id: Uuid::now_v7(),
            user_id: "u".into(),
            account_ids: vec!["acct-a".into()],
            created_at: now,
            valid_until: now + chrono::Duration::minutes(30),
            plan_hash: [0u8; 32],
            account_state_etags: etags,
            account_providers: std::collections::BTreeMap::new(),
            status: PlanStatus::Ready,
            totals: PlanTotals::default(),
            risk: RiskRollup::default(),
            warnings: vec![],
            operations: rows,
        }
    }

    fn row(seq: u64, risk: RiskLevel) -> PlannedOperation {
        PlannedOperation::Materialized(PlannedOperationRow {
            seq,
            account_id: "acct-a".into(),
            email_id: Some(format!("e{seq}")),
            action: PlanAction::Archive,
            source: PlanSource::Manual,
            target: None,
            reverse_op: None,
            risk,
            status: OperationStatus::Pending,
            skip_reason: None,
            applied_at: None,
            error: None,
        })
    }

    fn make_orchestrator(
        plan_repo: Arc<InMemPlanRepo>,
        job_repo: Arc<InMemJobRepo>,
        account_provider: Arc<dyn AccountStateProvider>,
    ) -> Arc<ApplyOrchestrator> {
        let drift = Arc::new(DriftDetector::new(account_provider));
        let expander = Arc::new(PredicateExpander::new(
            Arc::new(StubRules) as Arc<dyn crate::cleanup::domain::ports::RuleEvaluator>,
            Arc::new(StubEmailRepo) as Arc<dyn EmailRepository>,
        ));
        Arc::new(
            ApplyOrchestrator::new(
                plan_repo as Arc<dyn CleanupPlanRepository>,
                job_repo as Arc<dyn CleanupApplyJobRepository>,
                drift,
                expander,
                Arc::new(|_| Provider::Gmail),
                Arc::new(UnsubscribeService::new()),
            )
            .with_provider_factory(successful_archive_factory()),
        )
    }

    async fn wait_for_finish(rx: &mut broadcast::Receiver<ApplyEvent>) -> JobCounts {
        loop {
            match rx.recv().await {
                Ok(ApplyEvent::Finished { counts, .. }) => return counts,
                Ok(_) => continue,
                Err(_) => return JobCounts::default(),
            }
        }
    }

    // --- Tests ------------------------------------------------------------

    #[tokio::test]
    async fn cancel_mid_apply_preserves_applied_rows() {
        let plan_repo = Arc::new(InMemPlanRepo::default());
        let job_repo = Arc::new(InMemJobRepo::default());
        let plan = sample_plan_with_rows(vec![
            row(1, RiskLevel::Low),
            row(2, RiskLevel::Low),
            row(3, RiskLevel::Low),
        ]);
        plan_repo.save(&plan).await.unwrap();

        let orch = make_orchestrator(plan_repo.clone(), job_repo.clone(), Arc::new(CleanProvider));
        let job_id = orch
            .clone()
            .begin_apply(
                &plan,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();
        // Cancel almost immediately.
        let _ = orch.cancel(job_id).await;
        // Wait for finish — applied rows should be preserved.
        let log = plan_repo.status_log.lock().unwrap().clone();
        // We don't strictly require any to have been applied since cancel
        // races with the loop start; what we DO require is none is reverted.
        assert!(log
            .iter()
            .all(|(_, s)| !matches!(s, OperationStatus::Pending)));
    }

    #[tokio::test]
    async fn partial_apply_round_trip_low_then_medium_then_high() {
        let plan_repo = Arc::new(InMemPlanRepo::default());
        let job_repo = Arc::new(InMemJobRepo::default());
        let plan = sample_plan_with_rows(vec![
            row(1, RiskLevel::Low),
            row(2, RiskLevel::Medium),
            row(3, RiskLevel::High),
        ]);
        plan_repo.save(&plan).await.unwrap();

        let orch = make_orchestrator(plan_repo.clone(), job_repo.clone(), Arc::new(CleanProvider));

        // Apply Low.
        let jid1 = orch
            .clone()
            .begin_apply(
                &plan,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();
        let mut rx1 = orch.subscribe(jid1).await.unwrap();
        let _ = wait_for_finish(&mut rx1).await;

        // Apply Medium with manual ack (Manual source has empty group key
        // so no ack is needed for medium-risk rows here).
        let plan2 = plan_repo.plan.lock().unwrap().clone().unwrap();
        let jid2 = orch
            .clone()
            .begin_apply(
                &plan2,
                ApplyOptions {
                    risk_max: RiskMax::Medium,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();
        let mut rx2 = orch.subscribe(jid2).await.unwrap();
        let _ = wait_for_finish(&mut rx2).await;
        // Apply High with explicit ack after the second claim has finished.
        let plan3 = plan_repo.plan.lock().unwrap().clone().unwrap();
        let _jid3 = orch
            .clone()
            .begin_apply(
                &plan3,
                ApplyOptions {
                    risk_max: RiskMax::High,
                    acknowledged_high_risk_seqs: vec![3],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();

        // Allow background tasks to settle.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // The plan now has all rows in non-pending terminal state.
        let plan_final = plan_repo.plan.lock().unwrap().clone().unwrap();
        for op in plan_final.operations {
            if let PlannedOperation::Materialized(r) = op {
                assert!(
                    !matches!(r.status, OperationStatus::Pending),
                    "row {} still pending",
                    r.seq
                );
            }
        }
    }

    #[tokio::test]
    async fn hard_drift_blocks_apply() {
        // Baseline is Gmail history; live etag is None → Hard drift.
        let plan_repo = Arc::new(InMemPlanRepo::default());
        let job_repo = Arc::new(InMemJobRepo::default());
        let mut plan = sample_plan_with_rows(vec![row(1, RiskLevel::Low)]);
        plan.account_state_etags.insert(
            "acct-a".into(),
            AccountStateEtag::GmailHistory {
                history_id: "100".into(),
            },
        );
        plan_repo.save(&plan).await.unwrap();

        let orch = make_orchestrator(
            plan_repo.clone(),
            job_repo.clone(),
            Arc::new(HardDriftProvider),
        );
        let res = orch
            .begin_apply(
                &plan,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await;
        assert!(matches!(res, Err(BeginApplyError::HardDrift(_))));
    }

    #[tokio::test]
    async fn cancel_resume_continues_from_pending() {
        let plan_repo = Arc::new(InMemPlanRepo::default());
        let job_repo = Arc::new(InMemJobRepo::default());
        let plan = sample_plan_with_rows(vec![
            row(1, RiskLevel::Low),
            row(2, RiskLevel::Low),
            row(3, RiskLevel::Low),
        ]);
        plan_repo.save(&plan).await.unwrap();

        let orch = make_orchestrator(plan_repo.clone(), job_repo.clone(), Arc::new(CleanProvider));
        let jid1 = orch
            .clone()
            .begin_apply(
                &plan,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();
        // Cancel; then re-issue. Re-issued apply walks remaining pending rows.
        let _ = orch.cancel(jid1).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let plan2 = plan_repo.plan.lock().unwrap().clone().unwrap();
        let _jid2 = orch
            .clone()
            .begin_apply(
                &plan2,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let final_plan = plan_repo.plan.lock().unwrap().clone().unwrap();
        for op in final_plan.operations {
            if let PlannedOperation::Materialized(r) = op {
                assert!(
                    !matches!(r.status, OperationStatus::Pending),
                    "row {} still pending after resume",
                    r.seq
                );
            }
        }
    }

    // --- Phase D: audit-write integration ---------------------------------
    //
    // Verify that running a multi-account apply writes one audit row per
    // (seq, outcome) for every operation that left Pending. Uses a real
    // SeaOrmCleanupAuditWriter against in-memory SQLite.

    #[tokio::test]
    async fn audit_rows_written_for_each_apply_outcome() {
        use crate::cleanup::audit::{AuditOutcome, CleanupAuditWriter, SeaOrmCleanupAuditWriter};

        // Set up an in-memory database with migrations 024 + 025 applied so
        // the audit writer has its table.

        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        crate::db::apply_sqlite_migrations(
            &conn,
            &[
                include_str!("../../../migrations/sqlite/024_cleanup_planning.sql"),
                include_str!("../../../migrations/sqlite/025_cleanup_audit_log.sql"),
            ],
        )
        .await
        .expect("migrate");

        let plan_repo = Arc::new(InMemPlanRepo::default());
        let job_repo = Arc::new(InMemJobRepo::default());
        let plan = sample_plan_with_rows(vec![
            row(1, RiskLevel::Low),
            row(2, RiskLevel::Low),
            row(3, RiskLevel::Low),
        ]);
        plan_repo.save(&plan).await.unwrap();

        let drift = Arc::new(DriftDetector::new(Arc::new(CleanProvider)));
        let expander = Arc::new(PredicateExpander::new(
            Arc::new(StubRules) as Arc<dyn crate::cleanup::domain::ports::RuleEvaluator>,
            Arc::new(StubEmailRepo) as Arc<dyn EmailRepository>,
        ));
        let audit: Arc<dyn CleanupAuditWriter> =
            Arc::new(SeaOrmCleanupAuditWriter::new(db.clone()));
        let orch = Arc::new(
            ApplyOrchestrator::new(
                plan_repo.clone() as Arc<dyn CleanupPlanRepository>,
                job_repo.clone() as Arc<dyn CleanupApplyJobRepository>,
                drift,
                expander,
                Arc::new(|_| Provider::Gmail),
                Arc::new(UnsubscribeService::new()),
            )
            .with_provider_factory(successful_archive_factory())
            .with_audit(audit.clone()),
        );

        let _job_id = orch
            .clone()
            .begin_apply(
                &plan,
                ApplyOptions {
                    risk_max: RiskMax::Low,
                    acknowledged_high_risk_seqs: vec![],
                    acknowledged_medium_groups: vec![],
                },
            )
            .await
            .unwrap();

        // Allow background apply task to settle.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let entries = audit.list_for_plan(plan.id).await.expect("list");
        // Three rows applied → at least three audit entries (one per op).
        assert!(
            entries.len() >= 3,
            "expected ≥3 audit rows, got {}",
            entries.len()
        );
        // Every entry is for the right plan and user; no email_id leakage
        // is possible by construction (struct has no such field).
        for e in &entries {
            assert_eq!(e.plan_id, plan.id);
            assert_eq!(e.user_id, "u");
            assert_eq!(e.account_id, "acct-a");
            assert_eq!(e.outcome, AuditOutcome::Applied);
        }
    }
    // Real persistence is intentional: the repository caps every page at 1,000.
    async fn persisted_worker(
        rows: Vec<PlannedOperation>,
    ) -> (
        AccountWorker,
        Arc<crate::cleanup::repository::SeaOrmCleanupPlanRepo>,
        CleanupPlan,
    ) {
        let db = crate::db::test_sqlite_database().await;
        crate::db::apply_sqlite_migrations(
            &db.sea_orm(),
            &[include_str!(
                "../../../migrations/sqlite/024_cleanup_planning.sql"
            )],
        )
        .await
        .expect("migrate");
        let repo = Arc::new(crate::cleanup::repository::SeaOrmCleanupPlanRepo::new(
            db.sea_orm(),
        ));
        let plan = sample_plan_with_rows(rows);
        repo.save(&plan).await.expect("save plan");
        let (emitter, _) = EventEmitter::new(4096);
        let worker = AccountWorker {
            account_id: "acct-a".into(),
            provider: Provider::Outlook,
            ctx: AccountWorkerCtx {
                repo: repo.clone(),
                provider_factory: Arc::new(MockEmailProviderFactory::no_op()),
                unsubscribe: Arc::new(UnsubscribeService::new()),
                expander: Arc::new(PredicateExpander::new(
                    Arc::new(StubRules),
                    Arc::new(StubEmailRepo),
                )),
                emitter,
                audit: Arc::new(NoopCleanupAuditWriter),
                user_id: plan.user_id.clone(),
                job_id: Uuid::now_v7(),
                db: None,
            },
        };
        (worker, repo, plan)
    }

    #[tokio::test]
    async fn worker_visits_operations_beyond_repository_page_limit() {
        let (worker, repo, plan) =
            persisted_worker((1..=1005).map(|seq| row(seq, RiskLevel::High)).collect()).await;
        let counts = worker
            .run(
                plan.id,
                RiskMax::High,
                HashSet::new(),
                HashSet::new(),
                CancellationToken::new(),
            )
            .await
            .expect("run worker");
        assert_eq!(
            counts.skipped, 1005,
            "all unacknowledged rows must be visited"
        );
        let (tail, _) = repo
            .list_operations(plan.id, OpsFilter::default(), Some(1000), 10)
            .await
            .expect("tail");
        assert_eq!(tail.len(), 5);
        assert!(tail.iter().all(|op| matches!(op,
            PlannedOperation::Materialized(r) if r.status == OperationStatus::Skipped
        )));
    }

    #[tokio::test]
    async fn worker_counts_pending_operations_beyond_repository_page_limit() {
        let (worker, _, plan) =
            persisted_worker((1..=1005).map(|seq| row(seq, RiskLevel::High)).collect()).await;
        let counts = worker
            .run(
                plan.id,
                RiskMax::Low,
                HashSet::new(),
                HashSet::new(),
                CancellationToken::new(),
            )
            .await
            .expect("run worker");
        assert_eq!(
            counts.pending, 1005,
            "risk-excluded rows must remain visible in counts"
        );
        assert_eq!(counts.applied + counts.failed + counts.skipped, 0);
    }
    #[tokio::test]
    async fn missing_provider_fails_without_recording_success() {
        let (worker, repo, plan) = persisted_worker(vec![row(1, RiskLevel::Low)]).await;
        let mut events = worker.ctx.emitter.subscribe();
        let counts = worker
            .run(
                plan.id,
                RiskMax::Low,
                HashSet::new(),
                HashSet::new(),
                CancellationToken::new(),
            )
            .await
            .expect("run worker");
        assert_eq!(counts.applied, 0, "no provider operation was performed");
        assert_eq!(counts.failed, 1);
        let (rows, _) = repo
            .list_operations(plan.id, OpsFilter::default(), None, 10)
            .await
            .expect("rows");
        assert!(matches!(&rows[0], PlannedOperation::Materialized(r)
            if r.status == OperationStatus::Failed));
        assert!(matches!(events.try_recv().expect("failure event"),
            ApplyEvent::OpFailed { seq: 1, error, .. } if error.code == "account_not_found"));
        while let Ok(event) = events.try_recv() {
            assert!(!matches!(event, ApplyEvent::OpApplied { .. }));
        }
    }

    // The external mailbox is the only fake; worker persistence and events stay real.
    struct SuccessfulArchiveProvider;

    #[async_trait]
    impl crate::email::provider::EmailProvider for SuccessfulArchiveProvider {
        async fn authenticate(
            &self,
            _: &str,
        ) -> Result<crate::email::types::OAuthTokens, crate::email::provider::ProviderError>
        {
            unreachable!("archive fixture does not authenticate")
        }
        async fn refresh_token(
            &self,
            _: &str,
        ) -> Result<crate::email::types::OAuthTokens, crate::email::provider::ProviderError>
        {
            unreachable!("archive fixture does not refresh tokens")
        }
        async fn list_messages(
            &self,
            _: &str,
            _: &crate::email::types::ListParams,
        ) -> Result<crate::email::types::EmailPage, crate::email::provider::ProviderError> {
            unreachable!("archive fixture does not list messages")
        }
        async fn get_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::email::types::EmailMessage, crate::email::provider::ProviderError>
        {
            unreachable!("archive fixture does not fetch messages")
        }
        async fn archive_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::email::provider::ProviderError> {
            Ok(())
        }
        async fn label_message(
            &self,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<(), crate::email::provider::ProviderError> {
            unreachable!("archive fixture does not label messages")
        }
        async fn remove_labels(
            &self,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<(), crate::email::provider::ProviderError> {
            unreachable!("archive fixture does not remove labels")
        }
        async fn create_label(
            &self,
            _: &str,
            _: &str,
        ) -> Result<String, crate::email::provider::ProviderError> {
            unreachable!("archive fixture does not create labels")
        }
    }

    fn successful_archive_factory() -> Arc<dyn EmailProviderFactory> {
        Arc::new(MockEmailProviderFactory::new(|_| {
            Ok(super::super::factory::ResolvedProvider {
                provider: Arc::new(SuccessfulArchiveProvider),
                access_token: "test-token".into(),
                kind: crate::email::types::ProviderKind::Gmail,
            })
        }))
    }
    fn terminal_rows() -> Vec<PlannedOperation> {
        let mut rows: Vec<_> = [
            OperationStatus::Applied,
            OperationStatus::Failed,
            OperationStatus::Skipped,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, status)| {
            let PlannedOperation::Materialized(mut r) = row(i as u64 + 1, RiskLevel::High) else {
                unreachable!()
            };
            r.status = status;
            PlannedOperation::Materialized(r)
        })
        .collect();
        use crate::cleanup::domain::operation::PredicateStatus;
        for (i, status) in [
            PredicateStatus::Expanded,
            PredicateStatus::Applied,
            PredicateStatus::Failed,
            PredicateStatus::Skipped,
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(PlannedOperation::Predicate(
                crate::cleanup::domain::operation::PlannedOperationPredicate {
                    seq: i as u64 + 4,
                    account_id: "acct-a".into(),
                    predicate_kind:
                        crate::cleanup::domain::operation::PredicateKind::ArchiveStrategy,
                    predicate_id: "older30d".into(),
                    constraint_fingerprint: None,
                    action: PlanAction::Archive,
                    target: None,
                    source: PlanSource::Manual,
                    projected_count: 0,
                    sample_email_ids: vec![],
                    risk: RiskLevel::High,
                    status,
                    partial_applied_count: 0,
                    error: None,
                },
            ));
        }
        rows
    }

    #[tokio::test]
    async fn terminal_rows_preserve_outcomes_without_reacknowledgement() {
        let expected = serde_json::to_value(terminal_rows()).expect("expected rows");
        let (worker, repo, plan) = persisted_worker(terminal_rows()).await;
        let mut events = worker.ctx.emitter.subscribe();
        let counts = worker
            .run(
                plan.id,
                RiskMax::High,
                HashSet::new(),
                HashSet::new(),
                CancellationToken::new(),
            )
            .await
            .expect("run worker");
        let (rows, _) = repo
            .list_operations(plan.id, OpsFilter::default(), None, 10)
            .await
            .expect("rows");
        assert_eq!(
            serde_json::to_value(rows).expect("stored rows"),
            expected,
            "replaying terminal operations must not rewrite their persisted outcomes"
        );
        assert_eq!(
            counts.applied + counts.failed + counts.skipped + counts.pending,
            0
        );
        assert!(
            events.try_recv().is_err(),
            "terminal rows must not emit new outcomes"
        );
    }

    #[tokio::test]
    async fn terminal_rows_are_not_counted_pending_above_risk_max() {
        let (worker, _, plan) = persisted_worker(terminal_rows()).await;
        let counts = worker
            .run(
                plan.id,
                RiskMax::Low,
                HashSet::new(),
                HashSet::new(),
                CancellationToken::new(),
            )
            .await
            .expect("run worker");
        assert_eq!(
            counts.pending, 0,
            "completed rows are not awaiting a higher-risk apply"
        );
        assert_eq!(counts.applied + counts.failed + counts.skipped, 0);
    }
    #[tokio::test]
    async fn worker_rejects_non_advancing_cursor_before_dispatch() {
        for (backstep, empty_cursor_regression) in [(0, false), (1, false), (1, true)] {
            let (mut worker, _, plan) = persisted_worker(vec![row(1, RiskLevel::Low)]).await;
            let repo = Arc::new(InMemPlanRepo {
                cursor_backstep: Some(backstep),
                empty_cursor_regression,
                ..Default::default()
            });
            repo.save(&plan).await.expect("save");
            worker.ctx.repo = repo.clone();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                worker.run(
                    plan.id,
                    RiskMax::Low,
                    HashSet::new(),
                    HashSet::new(),
                    CancellationToken::new(),
                ),
            )
            .await
            .expect("faulty pagination must terminate");
            assert!(
                matches!(
                    result,
                    Err(super::super::account_worker::WorkerError::Repo(_))
                ),
                "stalled nonempty pages and regressing cursors must not look complete"
            );
            assert!(
                repo.status_log.lock().unwrap().is_empty(),
                "no operation may be dispatched from an incomplete snapshot"
            );
        }
    }
    async fn persistent_apply_fixture() -> (crate::db::Database, Arc<ApplyOrchestrator>, CleanupPlan)
    {
        let db = crate::db::test_sqlite_database().await;
        crate::db::apply_sqlite_migrations(
            &db.sea_orm(),
            &[include_str!(
                "../../../migrations/sqlite/024_cleanup_planning.sql"
            )],
        )
        .await
        .expect("migrate");
        let plan_repo = Arc::new(crate::cleanup::repository::SeaOrmCleanupPlanRepo::new(
            db.sea_orm(),
        ));
        let job_repo = Arc::new(crate::cleanup::repository::SeaOrmCleanupApplyJobRepo::new(
            db.sea_orm(),
        ));
        let plan = sample_plan_with_rows(vec![row(1, RiskLevel::Low), row(2, RiskLevel::Low)]);
        plan_repo.save(&plan).await.expect("save");
        let orch = Arc::new(
            ApplyOrchestrator::new(
                plan_repo,
                job_repo,
                Arc::new(DriftDetector::new(Arc::new(CleanProvider))),
                Arc::new(PredicateExpander::new(
                    Arc::new(StubRules),
                    Arc::new(StubEmailRepo),
                )),
                Arc::new(|_| Provider::Gmail),
                Arc::new(UnsubscribeService::new()),
            )
            .with_provider_factory(successful_archive_factory()),
        );
        (db, orch, plan)
    }

    async fn persistent_wait(orch: &ApplyOrchestrator, job_id: JobId) -> CleanupApplyJob {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let job = orch
                    .job_repo
                    .load(job_id)
                    .await
                    .expect("job")
                    .expect("present");
                if !matches!(job.state, JobState::Running | JobState::Queued) {
                    break job;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("job finishes")
    }

    fn low_options() -> ApplyOptions {
        ApplyOptions {
            risk_max: RiskMax::Low,
            acknowledged_high_risk_seqs: vec![],
            acknowledged_medium_groups: vec![],
        }
    }

    #[tokio::test]
    async fn persistent_apply_claim_allows_only_one_concurrent_request() {
        let (_, orch, plan) = persistent_apply_fixture().await;
        // Independent channel maps model two server instances sharing only persistence.
        let peer = Arc::new(
            ApplyOrchestrator::new(
                orch.plan_repo.clone(),
                orch.job_repo.clone(),
                orch.drift.clone(),
                orch.expander.clone(),
                orch.workers_for.clone(),
                orch.unsubscribe.clone(),
            )
            .with_provider_factory(successful_archive_factory()),
        );
        let (a, b) = tokio::join!(
            orch.clone().begin_apply(&plan, low_options()),
            peer.begin_apply(&plan, low_options())
        );
        let winners: Vec<_> = [a, b].into_iter().filter_map(Result::ok).collect();
        assert_eq!(
            winners.len(),
            1,
            "exactly one apply may own the stored plan"
        );
        let job = persistent_wait(&orch, winners[0]).await;
        assert_eq!(job.state, JobState::Finished);
        assert_eq!(job.counts.applied, 2);
        let stored = orch.plan_repo.load("u", plan.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PlanStatus::Applied);
        assert_eq!(orch.job_repo.list_by_plan(plan.id).await.unwrap().len(), 1);
        assert!(
            orch.clone()
                .begin_apply(&plan, low_options())
                .await
                .is_err(),
            "stale ready snapshot cannot replay a completed plan"
        );
    }

    #[tokio::test]
    async fn persistent_apply_cancellation_releases_claim_for_resume() {
        let (_, orch, plan) = persistent_apply_fixture().await;
        let first = orch
            .clone()
            .begin_apply(&plan, low_options())
            .await
            .unwrap();
        orch.cancel(first).await.unwrap();
        let cancelled = persistent_wait(&orch, first).await;
        assert_eq!(cancelled.state, JobState::Cancelled);
        let remaining = orch.plan_repo.load("u", plan.id).await.unwrap().unwrap();
        assert_eq!(remaining.status, PlanStatus::PartiallyApplied);
        let resumed = orch
            .clone()
            .begin_apply(&remaining, low_options())
            .await
            .unwrap();
        let completed = persistent_wait(&orch, resumed).await;
        assert_eq!(completed.state, JobState::Finished);
        let stored = orch.plan_repo.load("u", plan.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PlanStatus::Applied);
        assert!(stored.operations.iter().all(|op| matches!(op, PlannedOperation::Materialized(r) if r.status == OperationStatus::Applied)));
    }

    #[tokio::test]
    async fn persistent_apply_job_creation_error_releases_claim_without_rewriting_rows() {
        use sea_orm::ConnectionTrait;
        let (db, orch, plan) = persistent_apply_fixture().await;
        db.sea_orm().execute_unprepared("CREATE TRIGGER reject_job BEFORE INSERT ON cleanup_apply_jobs BEGIN SELECT RAISE(FAIL, 'injected job error'); END").await.unwrap();
        assert!(orch
            .clone()
            .begin_apply(&plan, low_options())
            .await
            .is_err());
        let stored = orch.plan_repo.load("u", plan.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PlanStatus::Ready);
        assert_eq!(
            serde_json::to_value(&stored.operations).unwrap(),
            serde_json::to_value(&plan.operations).unwrap()
        );
        db.sea_orm()
            .execute_unprepared("DROP TRIGGER reject_job")
            .await
            .unwrap();
        let job = orch
            .clone()
            .begin_apply(&stored, low_options())
            .await
            .unwrap();
        assert_eq!(persistent_wait(&orch, job).await.state, JobState::Finished);
    }

    #[tokio::test]
    async fn persistent_apply_reports_failed_job_when_all_operations_fail() {
        let (_, mut orch, plan) = persistent_apply_fixture().await;
        Arc::get_mut(&mut orch).unwrap().provider_factory =
            Arc::new(MockEmailProviderFactory::no_op());
        let job = orch
            .clone()
            .begin_apply(&plan, low_options())
            .await
            .unwrap();
        let completed = persistent_wait(&orch, job).await;
        assert_eq!(completed.counts.failed, 2);
        assert_eq!(completed.state, JobState::Failed);
        assert_eq!(
            orch.plan_repo
                .load("u", plan.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            PlanStatus::PartiallyApplied
        );
    }
    #[tokio::test]
    async fn persistent_apply_replay_does_not_hide_unresolved_failures() {
        let (_, mut orch, plan) = persistent_apply_fixture().await;
        Arc::get_mut(&mut orch).unwrap().provider_factory =
            Arc::new(MockEmailProviderFactory::no_op());
        let first = orch
            .clone()
            .begin_apply(&plan, low_options())
            .await
            .unwrap();
        assert_eq!(persistent_wait(&orch, first).await.state, JobState::Failed);
        let remaining = orch.plan_repo.load("u", plan.id).await.unwrap().unwrap();
        let second = orch
            .clone()
            .begin_apply(&remaining, low_options())
            .await
            .unwrap();
        let replay = persistent_wait(&orch, second).await;
        assert_eq!(
            replay.counts.failed, 0,
            "terminal rows are not dispatched a second time"
        );
        assert_eq!(
            replay.state,
            JobState::Failed,
            "existing failures still prevent successful completion"
        );
        assert_eq!(
            orch.plan_repo
                .load("u", plan.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            PlanStatus::PartiallyApplied
        );
    }
}
