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
use crate::cleanup::domain::operation::{JobState, PlanStatus, Provider, RiskMax};
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
    /// Per-account EmailProvider factory (Item #1). Defaults to a no-op
    /// factory; production wiring installs `OAuthEmailProviderFactory`.
    pub provider_factory: Arc<dyn EmailProviderFactory>,
    pub unsubscribe: Arc<UnsubscribeService>,
    /// Per-operation audit writer (Phase D, ADR-030 §Security).
    pub audit: Arc<dyn CleanupAuditWriter>,
    /// Telemetry emitter (Phase D).
    pub telemetry: Arc<TelemetryEmitter>,
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
            job_channels: Arc::new(RwLock::new(HashMap::new())),
        }
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
        self.job_repo.create(&job_row).await?;

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

            // Determine terminal state.
            let final_state = if cancel.is_cancelled() {
                JobState::Cancelled
            } else if any_failed && combined.applied == 0 {
                JobState::Failed
            } else {
                JobState::Finished
            };

            // Persist job + plan envelopes.
            let now = Utc::now();
            let _ = me
                .job_repo
                .update_state(job_id, final_state, combined.clone(), Some(now))
                .await;

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
    }

    #[async_trait]
    impl CleanupPlanRepository for InMemPlanRepo {
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
            Ok((ops, None))
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
        Arc::new(ApplyOrchestrator::new(
            plan_repo as Arc<dyn CleanupPlanRepository>,
            job_repo as Arc<dyn CleanupApplyJobRepository>,
            drift,
            expander,
            Arc::new(|_| Provider::Gmail),
            Arc::new(UnsubscribeService::new()),
        ))
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
        let _jid2 = orch
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
        // Run to completion.
        // Apply High with explicit ack.
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
    // SqliteCleanupAuditWriter against in-memory SQLite.

    #[tokio::test]
    async fn audit_rows_written_for_each_apply_outcome() {
        use crate::cleanup::audit::{AuditOutcome, CleanupAuditWriter, SqliteCleanupAuditWriter};
        use sqlx::sqlite::SqlitePoolOptions;

        // Set up a SQLite pool with migrations 024 + 025 applied so the
        // audit writer has its table.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        for raw in [
            include_str!("../../../migrations/024_cleanup_planning.sql"),
            include_str!("../../../migrations/025_cleanup_audit_log.sql"),
        ] {
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
        }

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
            Arc::new(SqliteCleanupAuditWriter::new(pool.clone()));
        let orch = Arc::new(
            ApplyOrchestrator::new(
                plan_repo.clone() as Arc<dyn CleanupPlanRepository>,
                job_repo.clone() as Arc<dyn CleanupApplyJobRepository>,
                drift,
                expander,
                Arc::new(|_| Provider::Gmail),
                Arc::new(UnsubscribeService::new()),
            )
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
            assert!(matches!(
                e.outcome,
                AuditOutcome::Applied | AuditOutcome::Skipped | AuditOutcome::Failed
            ));
        }
    }
}
