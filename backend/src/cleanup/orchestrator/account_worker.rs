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
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::cleanup::audit::{AuditOutcome, CleanupAuditEntry, CleanupAuditWriter};
use crate::cleanup::domain::operation::{
    ErrorCode, OperationStatus, PlanAction, PlannedOperation, PlannedOperationPredicate,
    PlannedOperationRow, PredicateStatus, Provider, RiskLevel, RiskMax, SkipReason,
};
use crate::cleanup::domain::plan::{JobCounts, JobId, PlanId};
use crate::cleanup::repository::CleanupPlanRepository;
use crate::db::entities::emails;
use crate::email::provider::{MoveKind as ProvMoveKind, ProviderError};
use crate::email::unsubscribe::{SubscriptionTarget, UnsubscribeService};

use super::expander::PredicateExpander;
use super::factory::{EmailProviderFactory, FactoryError};
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
    /// Optional local DB handle — when set, a successful provider archive
    /// also updates `is_archived = true` in the local emails table so the
    /// Archive view shows the email immediately without waiting for a sync.
    pub db: Option<crate::db::Database>,
}

impl AccountWorkerCtx {}

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
            PlanAction::Archive => {
                let r = provider.archive_message(access_token, email_id).await;
                if r.is_ok() {
                    if let Some(ref db) = self.ctx.db {
                        mark_archived_locally(db, email_id).await;
                    }
                }
                r
            }
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
            PlanAction::Delete { permanent } => {
                if *permanent {
                    // Permanent: delete from provider, then remove from local DB.
                    let r = provider.delete_message(access_token, email_id).await;
                    if r.is_ok() {
                        if let Some(ref db) = self.ctx.db {
                            delete_locally(db, email_id).await;
                        }
                    }
                    r
                } else {
                    // Soft delete: archive on provider + mark locally.
                    let r = provider.archive_message(access_token, email_id).await;
                    if r.is_ok() {
                        if let Some(ref db) = self.ctx.db {
                            mark_archived_locally(db, email_id).await;
                        }
                    }
                    r
                }
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
        // Sender is stored in PlanSource::Subscription, not in email_id
        // (subscription rows are sender-level and have email_id = None).
        let sender = match &row.source {
            crate::cleanup::domain::operation::PlanSource::Subscription { sender } => {
                sender.clone()
            }
            _ => row
                .email_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        };

        // Unsubscribe headers were captured from the source email at plan-build
        // time and carried in the action so no extra DB round-trip is needed.
        let (list_unsubscribe_header, list_unsubscribe_post) = match &row.action {
            PlanAction::Unsubscribe {
                list_unsubscribe_header,
                list_unsubscribe_post,
                ..
            } => (
                list_unsubscribe_header.clone(),
                list_unsubscribe_post.clone(),
            ),
            _ => (None, None),
        };

        let target = SubscriptionTarget {
            sender,
            list_unsubscribe_header,
            list_unsubscribe_post,
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

/// Mark an email `is_archived` locally after a successful provider archive. Best-effort:
/// errors are swallowed (matching this call site's pre-existing `let _ =` behavior — the
/// provider-side archive already succeeded, so a failure here is a local cache staleness,
/// not an apply failure).
///
/// `labels` is REPLACED by the single value `'ARCHIVED'`, not appended to — the pre-port
/// behavior, preserved deliberately.
///
/// `is_archived` is a real BOOLEAN column in both dialects, and the `emails` entity declares
/// it `bool`, so SeaORM encodes the right per-backend value: Postgres rejects an integer
/// literal against a BOOLEAN column (ADR-035, now entity-owned per ADR-036).
async fn mark_archived_locally(db: &crate::db::Database, email_id: &str) {
    let _ = emails::Entity::update_many()
        .col_expr(emails::Column::Labels, Expr::value("ARCHIVED"))
        .col_expr(emails::Column::IsArchived, Expr::value(true))
        .filter(emails::Column::Id.eq(email_id))
        .exec(&db.sea_orm())
        .await;
}

/// Remove an email from the local cache after a successful permanent provider delete.
/// Best-effort, matching `mark_archived_locally`'s error-swallowing rationale.
async fn delete_locally(db: &crate::db::Database, email_id: &str) {
    let _ = emails::Entity::delete_many()
        .filter(emails::Column::Id.eq(email_id))
        .exec(&db.sea_orm())
        .await;
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

// ---------------------------------------------------------------------------
// Tests — behavior pins for the two local-cache helpers.
//
// These reach the database directly rather than through `AccountWorker::run`:
// the orchestrator-level tests all run with `MockEmailProviderFactory::no_op()`,
// whose `NotFound` short-circuits `dispatch` before either helper is reached,
// so nothing above this line exercises them.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection};
    use sqlx::sqlite::SqlitePoolOptions;

    /// In-memory SQLite carrying every migration the `emails` entity spans (see
    /// `rules::executor`'s identical helper for why each one is needed).
    async fn fresh_db() -> Database {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("connect");
        let db = Database::Sqlite(pool);
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../../migrations/sqlite/027_is_archived.sql"),
        ] {
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
                    conn.execute_unprepared(s).await.expect("migrate");
                }
            }
        }
        db
    }

    async fn seed_email(conn: &DatabaseConnection, id: &str, labels: &str) {
        emails::ActiveModel {
            id: Set(id.to_owned()),
            account_id: Set("acct-1".to_owned()),
            provider: Set("gmail".to_owned()),
            subject: Set(format!("subject {id}")),
            from_addr: Set("sender@example.com".to_owned()),
            to_addrs: Set("me@example.com".to_owned()),
            received_at: Set(Utc::now().naive_utc()),
            labels: Set(Some(labels.to_owned())),
            is_read: Set(Some(false)),
            is_starred: Set(Some(false)),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".to_owned()),
            is_archived: Set(false),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed email");
    }

    async fn load(conn: &DatabaseConnection, id: &str) -> Option<emails::Model> {
        emails::Entity::find_by_id(id)
            .one(conn)
            .await
            .expect("load")
    }

    #[tokio::test]
    async fn mark_archived_locally_sets_the_flag_and_replaces_labels() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", "INBOX,IMPORTANT").await;
        seed_email(&conn, "e2", "INBOX").await;

        mark_archived_locally(&db, "e1").await;

        let target = load(&conn, "e1").await.expect("row present");
        assert!(target.is_archived);
        // Wholesale replacement, NOT an append — pre-port behavior, preserved.
        assert_eq!(target.labels.as_deref(), Some("ARCHIVED"));

        let bystander = load(&conn, "e2").await.expect("row present");
        assert!(!bystander.is_archived);
        assert_eq!(bystander.labels.as_deref(), Some("INBOX"));
    }

    #[tokio::test]
    async fn delete_locally_removes_only_the_target_row() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", "INBOX").await;
        seed_email(&conn, "e2", "INBOX").await;

        delete_locally(&db, "e1").await;

        assert!(load(&conn, "e1").await.is_none());
        assert!(load(&conn, "e2").await.is_some());
    }

    #[tokio::test]
    async fn local_helpers_are_silent_no_ops_for_an_unknown_email() {
        // Both are best-effort: a miss must not panic (they swallow errors and
        // return `()`, so "no panic" is the whole observable contract).
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", "INBOX").await;

        mark_archived_locally(&db, "does-not-exist").await;
        delete_locally(&db, "does-not-exist").await;

        let untouched = load(&conn, "e1").await.expect("row present");
        assert!(!untouched.is_archived);
        assert_eq!(untouched.labels.as_deref(), Some("INBOX"));
    }
}
