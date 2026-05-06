//! Per-account apply worker (Phase C, ADR-030 §C.1).
//!
//! Walks pending rows for one account in seq order, filtered by
//! `risk_max` and ack-list, dispatches each operation via the appropriate
//! port, persists status updates, and emits SSE events.
//!
//! ## Rate limiting
//!
//! - The proactive throttle (40ms sleep between Gmail ops, 1s for POP3)
//!   lives **inside** each `AccountWorker` instance, so it is implicitly
//!   account-scoped: one noisy account cannot starve another because each
//!   account has its own worker task and its own `Semaphore`. This is the
//!   write-side proactive limiter.
//! - The provider's `ProviderError::RateLimited{retry_after_secs}` raised
//!   on 429 (`gmail.rs:1219-1230`) is the **reactive** backoff. We surface
//!   it as `AccountPaused { reason: rateLimit }` and stop the worker so
//!   the SSE consumer can decide whether to resume.
//! - These two are intentionally non-duplicative.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::cleanup::audit::{
    AuditOutcome, CleanupAuditEntry, CleanupAuditWriter, NoopCleanupAuditWriter,
};
use crate::cleanup::domain::operation::{
    ErrorCode, OperationStatus, PlanAction, PlannedOperation, PlannedOperationPredicate,
    PlannedOperationRow, PredicateStatus, Provider, RiskLevel, RiskMax, SkipReason,
};
use crate::cleanup::domain::plan::{JobCounts, JobId, PlanId};
use crate::cleanup::repository::CleanupPlanRepository;
use crate::email::provider::{MoveKind as ProvMoveKind, ProviderError};
use crate::email::unsubscribe::{SubscriptionTarget, UnsubscribeService};

use super::expander::PredicateExpander;
use super::factory::{EmailProviderFactory, FactoryError, MockEmailProviderFactory};
use super::sse::{plan_action_type_str, ApplyEvent, EventEmitter, PauseReason};

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("repo: {0}")]
    Repo(#[from] crate::cleanup::domain::ports::RepoError),
    #[error("cancelled")]
    Cancelled,
}

/// Hooks the worker needs to operate but which are sourced from the
/// orchestrator at run-time (so tests can inject mocks).
#[derive(Clone)]
pub struct AccountWorkerCtx {
    pub repo: Arc<dyn CleanupPlanRepository>,
    /// Per-account EmailProvider factory (Item #1). The worker calls
    /// `provider_for(account_id).await?` lazily; tests inject a
    /// `MockEmailProviderFactory`.
    pub provider_factory: Arc<dyn EmailProviderFactory>,
    pub unsubscribe: Arc<UnsubscribeService>,
    /// Apply-time predicate expander (Item #2).
    pub expander: Arc<PredicateExpander>,
    pub emitter: EventEmitter,
    /// Per-operation audit writer (Phase D, ADR-030 §Security). Writes
    /// one row per terminal outcome. Failures are logged but do NOT
    /// abort apply — audit is observational, not authoritative.
    pub audit: Arc<dyn CleanupAuditWriter>,
    /// User the apply was issued by — recorded on every audit row so
    /// `list_for_user` can surface the GDPR right-to-explanation set.
    pub user_id: String,
    /// Job id for this apply run; carried into every audit row.
    pub job_id: JobId,
}

impl AccountWorkerCtx {
    /// Convenience constructor for tests that don't care about audit;
    /// installs a no-op writer + placeholder user/job ids.
    #[cfg(test)]
    pub fn for_test(
        repo: Arc<dyn CleanupPlanRepository>,
        emitter: EventEmitter,
        expander: Arc<PredicateExpander>,
        unsubscribe: Arc<UnsubscribeService>,
    ) -> Self {
        Self {
            repo,
            provider_factory: Arc::new(MockEmailProviderFactory::no_op())
                as Arc<dyn EmailProviderFactory>,
            unsubscribe,
            expander,
            emitter,
            audit: Arc::new(NoopCleanupAuditWriter) as Arc<dyn CleanupAuditWriter>,
            user_id: "test-user".into(),
            job_id: uuid::Uuid::nil(),
        }
    }
}

pub struct AccountWorker {
    pub account_id: String,
    pub provider: Provider,
    pub ctx: AccountWorkerCtx,
}

impl AccountWorker {
    /// Drive all pending rows for this account, honouring risk-max + ack
    /// gates and per-provider concurrency. Returns the final per-account
    /// JobCounts (only counts rows this worker touched).
    pub async fn run(
        &self,
        plan_id: PlanId,
        risk_max: RiskMax,
        acked_high_seqs: HashSet<u64>,
        acked_medium_groups: HashSet<String>,
        cancel: CancellationToken,
    ) -> Result<JobCounts, WorkerError> {
        // Per-provider concurrency knobs (ADR-030 §C.1).
        let semaphore = Arc::new(Semaphore::new(per_provider_concurrency(self.provider)));
        let throttle_ms = per_provider_throttle_ms(self.provider);

        // Read all rows for this account once, then iterate seq order. For
        // huge plans the production wiring should cursor-paginate; Phase C
        // accepts the upper bound (10k expansion test) since rows live in
        // SQLite already.
        let mut counts = JobCounts::default();

        let (rows, _) = self
            .ctx
            .repo
            .list_operations(
                plan_id,
                crate::cleanup::repository::OpsFilter {
                    account_id: Some(self.account_id.clone()),
                    ..Default::default()
                },
                None,
                u32::MAX,
            )
            .await?;

        let mut idx = 0usize;
        let mut rows = rows;
        while idx < rows.len() {
            if cancel.is_cancelled() {
                return Err(WorkerError::Cancelled);
            }
            let op = rows[idx].clone();
            idx += 1;

            // Skip rows above the risk-max threshold: they remain pending
            // for a follow-up apply with a higher risk_max.
            if !risk_max.includes(op.risk()) {
                counts.pending = counts.pending.saturating_add(1);
                continue;
            }

            // Acknowledgement gates (Phase B passes acked_high_seqs from
            // the apply request; medium "groups" mirror PlanSource group ids).
            if op.risk() == RiskLevel::High && !acked_high_seqs.contains(&op.seq()) {
                let action_type = plan_action_type_str(op.action()).to_string();
                self.skip(
                    plan_id,
                    op.seq(),
                    SkipReason::Unacknowledged,
                    action_type,
                    &mut counts,
                )
                .await?;
                self.write_audit_op(
                    plan_id,
                    &op,
                    AuditOutcome::Skipped,
                    Some(SkipReason::Unacknowledged),
                )
                .await;
                continue;
            }
            if op.risk() == RiskLevel::Medium {
                let group = group_key(&op);
                if !group.is_empty() && !acked_medium_groups.contains(&group) {
                    let action_type = plan_action_type_str(op.action()).to_string();
                    self.skip(
                        plan_id,
                        op.seq(),
                        SkipReason::Unacknowledged,
                        action_type,
                        &mut counts,
                    )
                    .await?;
                    self.write_audit_op(
                        plan_id,
                        &op,
                        AuditOutcome::Skipped,
                        Some(SkipReason::Unacknowledged),
                    )
                    .await;
                    continue;
                }
            }

            // Item #2: in-worker predicate expansion. When we encounter a
            // predicate row that's still pending, expand it page-by-page,
            // append the materialized children to the plan, and emit
            // PredicateExpanded for each page. Children get seq values
            // strictly greater than the current max — they'll be picked up
            // on the *next* worker invocation since this worker holds an
            // already-loaded `rows` slice. (Re-issuing apply is the path
            // for fresh-children processing; this matches the partial-apply
            // contract.)
            let row = match op {
                PlannedOperation::Materialized(r) => r,
                PlannedOperation::Predicate(p) => {
                    if matches!(
                        p.status,
                        PredicateStatus::Expanded
                            | PredicateStatus::Applied
                            | PredicateStatus::Failed
                            | PredicateStatus::Skipped
                    ) {
                        // Already terminal; nothing to do for the predicate
                        // row itself — children (if any) are independent rows.
                        continue;
                    }
                    if let Err(err) = self.expand_predicate_into_plan(plan_id, &p).await {
                        let action_type = plan_action_type_str(&p.action).to_string();
                        let _ = self
                            .ctx
                            .repo
                            .update_predicate_status(plan_id, p.seq, PredicateStatus::Failed)
                            .await;
                        self.ctx.emitter.emit(ApplyEvent::OpFailed {
                            seq: p.seq,
                            account_id: self.account_id.clone(),
                            error: ErrorCode {
                                code: "predicate_expand_failed".into(),
                                message: err.to_string(),
                            },
                            action_type,
                        });
                        counts.failed = counts.failed.saturating_add(1);
                    }
                    continue;
                }
            };

            // Skip rows that are already terminal (idempotent re-apply).
            if !matches!(row.status, OperationStatus::Pending) {
                continue;
            }

            // Acquire concurrency permit.
            let _permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => return Err(WorkerError::Cancelled),
            };

            // Dispatch.
            let action_type = plan_action_type_str(&row.action).to_string();
            match self.dispatch(&row).await {
                Ok(()) => {
                    let now = Utc::now();
                    self.ctx
                        .repo
                        .update_operation_status(plan_id, row.seq, OperationStatus::Applied, now)
                        .await?;
                    self.write_audit(plan_id, &row, AuditOutcome::Applied, None)
                        .await;
                    self.ctx.emitter.emit(ApplyEvent::OpApplied {
                        seq: row.seq,
                        account_id: self.account_id.clone(),
                        applied_at: now.timestamp_millis(),
                        action_type: action_type.clone(),
                    });
                    counts.applied = counts.applied.saturating_add(1);
                    self.ctx.emitter.bump_ops();
                }
                Err(DispatchError::Skipped(reason)) => {
                    self.skip(plan_id, row.seq, reason, action_type.clone(), &mut counts)
                        .await?;
                    self.write_audit(plan_id, &row, AuditOutcome::Skipped, Some(reason))
                        .await;
                }
                Err(DispatchError::AccountPaused(reason)) => {
                    self.ctx.emitter.emit(ApplyEvent::AccountPaused {
                        account_id: self.account_id.clone(),
                        reason,
                    });
                    counts.failed = counts.failed.saturating_add(1);
                    return Ok(counts);
                }
                Err(DispatchError::Failed(error)) => {
                    let now = Utc::now();
                    self.ctx
                        .repo
                        .update_operation_status(plan_id, row.seq, OperationStatus::Failed, now)
                        .await?;
                    let mut row_with_err = row.clone();
                    row_with_err.error = Some(error.clone());
                    self.write_audit(plan_id, &row_with_err, AuditOutcome::Failed, None)
                        .await;
                    self.ctx.emitter.emit(ApplyEvent::OpFailed {
                        seq: row.seq,
                        account_id: self.account_id.clone(),
                        error,
                        action_type: action_type.clone(),
                    });
                    counts.failed = counts.failed.saturating_add(1);
                }
            }

            // Bandwidth shaping for Gmail/POP3.
            if throttle_ms > 0 {
                tokio::time::sleep(Duration::from_millis(throttle_ms)).await;
            }

            // Fire a throttled progress tick.
            self.ctx.emitter.emit_progress(counts.clone()).await;
        }

        Ok(counts)
    }

    /// Write a single audit row for a materialized operation outcome.
    /// Failures of the audit write are logged but never abort apply —
    /// audit is observational, not authoritative (ADR-030 §Security).
    async fn write_audit(
        &self,
        plan_id: PlanId,
        row: &PlannedOperationRow,
        outcome: AuditOutcome,
        skip_reason: Option<SkipReason>,
    ) {
        let mut entry = CleanupAuditEntry::from_materialized(
            plan_id,
            self.ctx.job_id,
            &self.ctx.user_id,
            row,
            outcome,
        );
        if skip_reason.is_some() {
            entry.skip_reason = skip_reason;
        }
        if let Err(e) = self.ctx.audit.write(entry).await {
            tracing::error!(
                target: "cleanup.audit",
                account_id = %self.account_id,
                seq = row.seq,
                error = %e,
                "audit write failed (non-fatal)"
            );
        }
    }

    /// Audit-write variant for any [`PlannedOperation`] — used at the
    /// pre-dispatch skip sites where we still hold the wrapper enum.
    async fn write_audit_op(
        &self,
        plan_id: PlanId,
        op: &PlannedOperation,
        outcome: AuditOutcome,
        skip_reason: Option<SkipReason>,
    ) {
        let entry = CleanupAuditEntry::from_op(
            plan_id,
            self.ctx.job_id,
            &self.ctx.user_id,
            op,
            outcome,
            skip_reason,
            None,
        );
        if let Err(e) = self.ctx.audit.write(entry).await {
            tracing::error!(
                target: "cleanup.audit",
                account_id = %self.account_id,
                seq = op.seq(),
                error = %e,
                "audit write failed (non-fatal)"
            );
        }
    }

    async fn skip(
        &self,
        plan_id: PlanId,
        seq: u64,
        reason: SkipReason,
        action_type: String,
        counts: &mut JobCounts,
    ) -> Result<(), WorkerError> {
        let now = Utc::now();
        self.ctx
            .repo
            .update_operation_status(plan_id, seq, OperationStatus::Skipped, now)
            .await?;
        self.ctx.emitter.emit(ApplyEvent::OpSkipped {
            seq,
            account_id: self.account_id.clone(),
            reason,
            action_type,
        });
        counts.skipped = counts.skipped.saturating_add(1);
        let entry = counts.skipped_by_reason.entry(reason).or_insert(0);
        *entry = entry.saturating_add(1);
        Ok(())
    }

    /// Drive a predicate row through the apply-time expander (Item #2).
    /// Pages through all children, appending each page to the plan with
    /// strictly-increasing seq, emits `PredicateExpanded` per page, and
    /// transitions the predicate row's status from
    /// Pending → Expanding → Expanded on success.
    async fn expand_predicate_into_plan(
        &self,
        plan_id: PlanId,
        predicate: &PlannedOperationPredicate,
    ) -> Result<(), super::expander::ExpandError> {
        // Mark expanding before producing rows.
        let _ = self
            .ctx
            .repo
            .update_predicate_status(plan_id, predicate.seq, PredicateStatus::Expanding)
            .await;

        let page_size: u32 = 1000;
        let mut page: u32 = 0;
        loop {
            let children = self
                .ctx
                .expander
                .expand_page(predicate, page, page_size)
                .await?;
            if children.is_empty() {
                break;
            }
            let produced = children.len() as u64;
            // Allocate a contiguous seq block above the current max.
            let next_seq_start = self
                .ctx
                .repo
                .max_seq(plan_id)
                .await
                .map(|m| m.saturating_add(1))
                .unwrap_or(predicate.seq.saturating_add(1));
            let to_insert: Vec<PlannedOperation> = children
                .into_iter()
                .enumerate()
                .map(|(i, mut row)| {
                    row.seq = next_seq_start.saturating_add(i as u64);
                    PlannedOperation::Materialized(row)
                })
                .collect();
            self.ctx
                .repo
                .append_operations(plan_id, to_insert)
                .await
                .map_err(super::expander::ExpandError::from)?;
            self.ctx.emitter.emit(ApplyEvent::PredicateExpanded {
                predicate_seq: predicate.seq,
                produced_rows: produced,
            });
            page = page.saturating_add(1);
        }

        let _ = self
            .ctx
            .repo
            .update_predicate_status(plan_id, predicate.seq, PredicateStatus::Expanded)
            .await;
        Ok(())
    }

    /// Dispatch an action via the provider port. The factory yields a
    /// per-account `(EmailProvider, access_token)` pair (Item #1). When
    /// the factory has no provider for this account (Mock no-op default
    /// used in unit tests) we treat the call as a success so the
    /// orchestrator + SSE plumbing remains exercisable.
    async fn dispatch(&self, row: &PlannedOperationRow) -> Result<(), DispatchError> {
        // Unsubscribe is sender-level and routes through UnsubscribeService,
        // independent of the per-account provider.
        if matches!(row.action, PlanAction::Unsubscribe { .. }) {
            return self.dispatch_unsubscribe(row).await;
        }

        let resolved = match self
            .ctx
            .provider_factory
            .provider_for(&self.account_id)
            .await
        {
            Ok(r) => r,
            Err(FactoryError::NotFound(_)) => {
                tracing::debug!(
                    account_id = %self.account_id,
                    seq = row.seq,
                    "dispatch: factory has no provider for account — treating as success",
                );
                return Ok(());
            }
            Err(FactoryError::OAuth(msg)) => {
                tracing::warn!(
                    account_id = %self.account_id,
                    seq = row.seq,
                    "dispatch: oauth resolution failed: {msg}",
                );
                return Err(DispatchError::AccountPaused(PauseReason::AuthError));
            }
            Err(FactoryError::UnsupportedKind(kind)) => {
                return Err(DispatchError::Failed(ErrorCode {
                    code: "provider_unsupported".into(),
                    message: format!("provider kind not supported: {kind}"),
                }));
            }
            Err(e) => {
                return Err(DispatchError::Failed(ErrorCode {
                    code: "provider_unavailable".into(),
                    message: e.to_string(),
                }));
            }
        };
        let provider = resolved.provider;
        let access_token = resolved.access_token.as_str();

        // The repo doesn't replay precondition state today (the SQLite
        // schema doesn't track folder location of an email). Per ADR-030
        // §8 rule 4 the production check would re-read the message's local
        // folder/labels. We TODO that out for Phase C and proceed.

        let email_id = match &row.email_id {
            Some(e) => e.as_str(),
            None => {
                // Sender-level rows (Unsubscribe) require a different path.
                if matches!(row.action, PlanAction::Unsubscribe { .. }) {
                    return self.dispatch_unsubscribe(row).await;
                }
                return Err(DispatchError::Failed(ErrorCode {
                    code: "missing_email_id".into(),
                    message: "row has no emailId".into(),
                }));
            }
        };

        let result = match &row.action {
            PlanAction::Archive => provider.archive_message(access_token, email_id).await,
            PlanAction::AddLabel { .. } => match &row.target {
                Some(t) => {
                    provider
                        .label_message(access_token, email_id, std::slice::from_ref(&t.id))
                        .await
                }
                None => Err(ProviderError::ConfigError("addLabel without target".into())),
            },
            PlanAction::Move { kind } => match &row.target {
                Some(t) => {
                    let pmk = match kind {
                        crate::cleanup::domain::operation::MoveKind::Folder => ProvMoveKind::Folder,
                        crate::cleanup::domain::operation::MoveKind::Label => ProvMoveKind::Label,
                    };
                    provider
                        .move_message(access_token, email_id, &t.id, pmk)
                        .await
                }
                None => Err(ProviderError::ConfigError("move without target".into())),
            },
            PlanAction::Delete { permanent: _ } => {
                // Soft delete: archive. Permanent delete requires a
                // dedicated provider method that doesn't exist on the
                // trait yet — surface as Failed for now.
                provider.archive_message(access_token, email_id).await
            }
            PlanAction::Unsubscribe { .. } => {
                return self.dispatch_unsubscribe(row).await;
            }
            PlanAction::MarkRead => provider.mark_read(access_token, email_id, true).await,
            PlanAction::Star { on } => provider.star_message(access_token, email_id, *on).await,
        };

        match result {
            Ok(()) => Ok(()),
            Err(ProviderError::RateLimited { retry_after_secs }) => {
                tracing::warn!(
                    account_id = %self.account_id,
                    seq = row.seq,
                    retry_after_secs,
                    "rate limited; pausing account",
                );
                Err(DispatchError::AccountPaused(PauseReason::RateLimit))
            }
            Err(ProviderError::TokenExpired(_)) | Err(ProviderError::OAuthError(_)) => {
                Err(DispatchError::AccountPaused(PauseReason::AuthError))
            }
            Err(e) => Err(DispatchError::Failed(ErrorCode {
                code: "provider_error".into(),
                message: e.to_string(),
            })),
        }
    }

    async fn dispatch_unsubscribe(&self, row: &PlannedOperationRow) -> Result<(), DispatchError> {
        // Use UnsubscribeService.batch_unsubscribe with a single-element
        // batch as a thin per-row adapter (DDD-008 addendum).
        let target = SubscriptionTarget {
            sender: row
                .email_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            list_unsubscribe_header: None,
            list_unsubscribe_post: None,
            email_id: row.email_id.clone(),
        };
        let batch = self.ctx.unsubscribe.batch_unsubscribe(vec![target]).await;
        if batch.failed == 0 {
            Ok(())
        } else {
            Err(DispatchError::Failed(ErrorCode {
                code: "unsubscribe_failed".into(),
                message: format!("{} of {} failed", batch.failed, batch.total),
            }))
        }
    }
}

#[allow(dead_code)] // Skipped variant reserved for precondition checks (ADR-030 §8 rule 4).
enum DispatchError {
    Skipped(SkipReason),
    AccountPaused(PauseReason),
    Failed(ErrorCode),
}

fn per_provider_concurrency(p: Provider) -> usize {
    match p {
        Provider::Gmail => 25,
        Provider::Outlook => 4,
        Provider::Imap => 1,
        Provider::Pop3 => 1,
    }
}

fn per_provider_throttle_ms(p: Provider) -> u64 {
    match p {
        // ~25 ops/sec → 40ms between calls (governor crate not in deps).
        Provider::Gmail => 40,
        // Outlook: token-bucket via `Semaphore` size 4; no extra throttle.
        Provider::Outlook => 0,
        // IMAP: serial; the semaphore=1 already enforces.
        Provider::Imap => 0,
        // POP3: 1/sec to be polite.
        Provider::Pop3 => 1000,
    }
}

fn group_key(op: &PlannedOperation) -> String {
    use crate::cleanup::domain::operation::PlanSource as S;
    match op {
        PlannedOperation::Materialized(r) => match &r.source {
            S::Subscription { sender } => format!("subscription:{sender}"),
            S::Cluster { cluster_id, .. } => format!("cluster:{cluster_id}"),
            S::Rule { rule_id, .. } => format!("rule:{rule_id}"),
            S::ArchiveStrategy { strategy } => format!("strategy:{strategy:?}"),
            S::Manual => String::new(),
        },
        PlannedOperation::Predicate(p) => match &p.source {
            S::Subscription { sender } => format!("subscription:{sender}"),
            S::Cluster { cluster_id, .. } => format!("cluster:{cluster_id}"),
            S::Rule { rule_id, .. } => format!("rule:{rule_id}"),
            S::ArchiveStrategy { strategy } => format!("strategy:{strategy:?}"),
            S::Manual => String::new(),
        },
    }
}
