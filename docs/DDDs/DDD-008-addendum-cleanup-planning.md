# DDD-008 Addendum: Cleanup Planning Subdomain

| Field       | Value                                            |
| ----------- | ------------------------------------------------ |
| Status      | Accepted                                         |
| Date        | 2026-05-04                                       |
| Type        | Subdomain of Email Operations (Core)             |
| Context     | Cleanup Planning                                 |
| Realises    | ADR-030 (Cleanup Dry-Run / Plan-Apply)           |
| Informed by | `docs/research/cleanup-dry-run-due-diligence.md` |

## Overview

Cleanup Planning is a subdomain inside the Email Operations bounded context that materializes the user's wizard selections (subscription targets, topic-cluster actions, rule selections, archive strategy) into an immutable, reviewable, idempotently-applyable **CleanupPlan** before any mutation reaches a provider. It is the system of record for "what is about to happen" and "what already happened" during an Inbox Cleaner run. It composes — but does not replace — the existing per-feature unsubscribe preview and the rule engine.

This addendum adds a new aggregate, two repositories, and a small set of commands/events. Existing aggregates in DDD-008 (`EmailMessageAggregate`, `ThreadAggregate`) are unchanged. Only the orchestration of mutating commands inside the cleanup wizard is modified: outside the cleanup wizard, direct mutation paths (`MoveEmail`, `ArchiveEmail`, `DeleteEmail`, single-feature unsubscribe) remain available.

## Strategic Classification

| Aspect              | Value                                                                                           |
| ------------------- | ----------------------------------------------------------------------------------------------- |
| Domain type         | Core (subdomain of Email Operations)                                                            |
| Investment priority | High — closes the cleanup trust gap and delivers risk-scoped partial apply (unique-value lever) |
| Complexity driver   | Cross-source plan composition; per-row idempotent apply; hybrid materialized/predicate rows     |
| Change frequency    | Medium (new sources will plug in: digests, rules, filters)                                      |
| Risk                | Plan/state drift; large-plan storage; partial-apply UX                                          |

---

## Aggregates

### 3. CleanupPlanAggregate

An immutable, materialized description of a set of email operations the user has authorized but not yet applied. Mutation occurs only via `BuildPlan` (creates), `RefreshAccount` (rebuilds one account's rows on hard drift), and `MarkOperationApplied` / `MarkOperationFailed` / `MarkOperationSkipped` (during apply). Rows are never edited; replanning the entire plan produces a new aggregate.

**Root Entity: CleanupPlan**

| Field               | Type                               | Description                                                                                    |
| ------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------- |
| id                  | PlanId (UUID v7)                   | Idempotency / addressability key                                                               |
| user_id             | UserId                             | Owner; plans are user-scoped                                                                   |
| account_ids         | Vec\<AccountId\>                   | Accounts in scope                                                                              |
| created_at          | DateTime                           | Build timestamp                                                                                |
| valid_until         | DateTime                           | Expires (default `created_at + 30m`)                                                           |
| plan_hash           | Hash                               | Hash of `WizardSelections` + resolved local state                                              |
| account_state_etags | Map\<AccountId, AccountStateEtag\> | Per-account snapshot identifier captured at build                                              |
| status              | PlanStatus                         | `draft \| ready \| applying \| applied \| partially_applied \| failed \| expired \| cancelled` |
| totals              | Totals                             | Pre-computed rollups (per action, per account, per source, per risk)                           |
| warnings            | Vec\<PlanWarning\>                 | Non-blocking flags (large-group, conflict, low-confidence)                                     |

**Child Entity: PlannedOperation** (one row per atomic operation; sum-typed)

```rust
pub enum PlannedOperation {
    Materialized(PlannedOperationRow),
    Predicate(PlannedOperationPredicate),
}

pub struct PlannedOperationRow {
    pub seq: u64,                       // Deterministic apply order within the plan
    pub account_id: AccountId,
    pub email_id: Option<EmailId>,      // Specific message; None for sender-level ops (unsubscribe)
    pub action: PlanAction,
    pub source: PlanSource,             // Why this row exists
    pub target: Option<FolderOrLabel>,  // For label/move actions (ADR-018 vocabulary)
    pub reverse_op: Option<ReverseOp>,  // Inverse op for future Undo (None when irreversible)
    pub risk: RiskLevel,
    pub status: OperationStatus,        // pending | applied | failed | skipped
    pub applied_at: Option<DateTime>,
    pub error: Option<ErrorCode>,
}

pub struct PlannedOperationPredicate {
    pub seq: u64,
    pub account_id: AccountId,
    pub predicate_kind: PredicateKind,  // Rule | ArchiveStrategy | LabelFilter
    pub predicate_id: PredicateId,
    pub action: PlanAction,
    pub target: Option<FolderOrLabel>,
    pub projected_count: u64,           // Estimated message count at build time
    pub sample_email_ids: Vec<EmailId>, // 5–20 representatives for UI preview
    pub risk: RiskLevel,
    pub status: PredicateStatus,        // pending | expanding | expanded | applied | partially_applied | failed | skipped
    pub partial_applied_count: u64,     // Updated as rows are expanded and applied
    pub error: Option<ErrorCode>,
}
```

**Enums**

```rust
pub enum PlanAction {
    Archive,                          // Provider-uniform "remove from inbox without deleting"
    AddLabel { kind: MoveKind },      // Gmail label add or Outlook category
    Move { kind: MoveKind },          // Folder move (Outlook/IMAP) or label-set transition (Gmail)
    Delete { permanent: bool },       // Soft (Trash) or permanent
    Unsubscribe { method: UnsubscribeMethod },
    MarkRead,
    Star { on: bool },
}

pub enum PlanSource {
    Subscription { sender: String },
    Cluster      { cluster_id: ClusterId, cluster_action: ClusterAction },
    Rule         { rule_id: RuleId, match_basis: RuleMatchBasis },
    ArchiveStrategy { strategy: ArchiveStrategy },
    Manual,
}

pub enum RiskLevel { Low, Medium, High }

pub enum AccountStateEtag {
    GmailHistoryId(String),
    OutlookDeltaToken(String),
    ImapUidvalidityModseq { uidvalidity: u32, highest_modseq: u64 },
    None,                             // POP3 — any prior op invalidates
}
```

**Invariants**

- A plan is **append-only at construction**, then **status-only-mutable** during apply. No row's `action` / `target` / `source` / `risk` may change after `status = ready`. (Exception: `RefreshAccount` may delete and rebuild the rows for a single account in `hard_drift` state.)
- `plan_hash` is computed over `(WizardSelections, account_state_etags, materialized + predicate operations)`. If any of these are recomputed and differ in full, a new plan must be built.
- `valid_until` MUST be enforced server-side on every Apply call. Expired plans cannot be applied.
- `email_id` is set for all message-level materialized actions; `None` only for sender-level actions (`Unsubscribe`).
- `target` is required for `AddLabel` and `Move`; forbidden otherwise.
- `reverse_op` is `None` for `Delete { permanent: true }`, `Delete` on POP3, `Unsubscribe { WebLink | Mailto }`, and any operation on a provider whose Trash retention has lapsed. It is `Some` for label add/remove, archive, folder move on supporting providers, and reversible-by-protocol changes.
- The plan-level `risk` rollup is the histogram of operation `risk` values; recomputed on build, never edited.
- A plan owned by user A cannot be read or applied by user B (enforced via `user_id` predicate).
- **Drift is account-scoped, not plan-scoped.** Hard drift on account A invalidates only A's rows; the plan stays applyable for unaffected accounts. Soft drift on a row → `skipped` with reason `state_drift`.
- A predicate operation's `projected_count` is advisory; the authoritative count is the sum of materialized child rows produced at expansion time.

**Commands**

- `BuildPlan { user_id, selections, account_ids }` — pure (no provider mutation); produces `PlanBuilt`. Target P95 ≤ 800 ms for 50 k inbox.
- `RefreshAccount { plan_id, account_id }` — rebuilds only one account's operation rows after hard drift; preserves other accounts' state. Emits `PlanAccountRefreshed`.
- `RebuildPlan { plan_id }` — full rebuild (fresh `plan_hash`, fresh `PlanId`); used when selections change.
- `CancelPlan { plan_id }` — transitions to `cancelled`; emits `PlanCancelled`.
- `BeginApply { plan_id, risk_max, acknowledged_high_risk_seqs[], acknowledged_medium_groups[] }` — fails fast if any selected High-risk row is unacknowledged. Emits `ApplyStarted`.
- `ExpandPredicate { plan_id, predicate_seq }` — turns a predicate row into materialized children at apply time. Emits `PredicateExpanded`.
- `MarkOperationApplied { plan_id, seq, applied_at }` — emits `OperationApplied`.
- `MarkOperationFailed { plan_id, seq, error }` — emits `OperationFailed`.
- `MarkOperationSkipped { plan_id, seq, reason }` — emits `OperationSkipped` (drift, dedup, etc.).
- `FinishApply { plan_id, job_id }` — transitions plan status based on aggregate row outcomes; emits `ApplyFinished`.
- `CancelApply { job_id }` — pending rows become `skipped { reason: user_cancelled }`; applied rows stand. Emits `ApplyCancelled`.

**Events** (consumed by Email Intelligence for embedding-status updates, by Insights for activity log, by Audit for GDPR trail)

`PlanBuilt`, `PlanAccountRefreshed`, `PlanCancelled`, `PlanExpired`, `ApplyStarted`, `PredicateExpanded`, `OperationApplied`, `OperationFailed`, `OperationSkipped`, `ApplyFinished`, `ApplyCancelled`.

---

## Repositories

### CleanupPlanRepository

```rust
trait CleanupPlanRepository {
    async fn save(&self, plan: &CleanupPlan) -> Result<(), RepoError>;
    async fn load(&self, user: UserId, id: PlanId) -> Result<Option<CleanupPlan>, RepoError>;
    async fn list_by_user(&self, user: UserId, status: Option<PlanStatus>, limit: u32)
        -> Result<Vec<CleanupPlanSummary>, RepoError>;
    async fn list_operations(&self, id: PlanId, filter: OpsFilter, cursor: Cursor, limit: u32)
        -> Result<Page<PlannedOperation>, RepoError>;
    async fn sample_operations(&self, id: PlanId, source: PlanSource, n: u32)
        -> Result<Vec<EmailId>, RepoError>;
    async fn replace_account_rows(&self, id: PlanId, account: AccountId, new_rows: Vec<PlannedOperation>)
        -> Result<(), RepoError>;
    async fn update_operation_status(&self, id: PlanId, seq: u64, status: OperationStatus,
        ts: DateTime, err: Option<ErrorCode>) -> Result<(), RepoError>;
    async fn expand_predicate(&self, id: PlanId, predicate_seq: u64, page: u32, page_size: u32)
        -> Result<Vec<PlannedOperationRow>, RepoError>;
    async fn expire_due(&self, now: DateTime) -> Result<u32, RepoError>;
    async fn purge_older_than(&self, cutoff: DateTime) -> Result<u32, RepoError>;
}
```

Storage: SQLite tables `cleanup_plans`, `cleanup_plan_account_etags`, `cleanup_plan_operations`, `cleanup_apply_jobs` (see ADR-030 §10). Encrypted at rest per ADR-016/017.

### CleanupApplyJobRepository

```rust
trait CleanupApplyJobRepository {
    async fn create(&self, job: &CleanupApplyJob) -> Result<(), RepoError>;
    async fn load(&self, job_id: JobId) -> Result<Option<CleanupApplyJob>, RepoError>;
    async fn update_state(&self, job_id: JobId, state: JobState, counts: JobCounts) -> Result<(), RepoError>;
    async fn list_by_plan(&self, plan_id: PlanId) -> Result<Vec<CleanupApplyJob>, RepoError>;
}
```

A plan can have multiple apply jobs over time (one per `risk_max` tier). Each job operates on a disjoint subset of pending rows.

### PlanBuilder (domain service)

A pure orchestrator that, given `WizardSelections` and the current local repositories (`EmailRepository`, `SubscriptionRepository`, `ClusterRepository`, `RuleRepository`), produces a `CleanupPlan`. Composes:

- `UnsubscribeService::preview` (existing) for `PlanSource::Subscription` rows → Materialized rows.
- `ClusterRepository` → resolves cluster_id → email_ids → Materialized rows.
- `RuleEngine::evaluate(EvaluateOnly)` (DDD-007 addendum) → produces **Predicate rows** with sampled email_ids.
- `ArchiveStrategy` (per-account, ADR-010) → Predicate rows.

PlanBuilder is the only place where these four sources meet. It is responsible for:

- **Deduplication**: an email touched by both a cluster archive and a rule label gets one merged operation per `(account_id, email_id, target)`. Cluster wins over rule for conflicting actions; both add target labels are merged.
- **Conflict detection**: e.g., "rule moves to Folder-A and cluster moves to Folder-B" is a conflict reported as `PlanWarning::TargetConflict`.
- **Risk classification** (delegated to `RiskClassifier`).
- **Sample materialization**: for predicate rows, picks 5–20 representative messages (deterministic — head, tail, and stratified by date) for UI preview.

---

## Domain services

### ApplyOrchestrator

Drives `BeginApply → (per row) → FinishApply` for one apply job. Reads pending rows in `seq` order filtered by `risk_max`, dispatches to `EmailProvider` (ADR-018) — `move_message`, `star_message`, label/folder mutations — and to `UnsubscribeService::execute`. Records `MarkOperationApplied` / `MarkOperationFailed` / `MarkOperationSkipped` per row.

Per-account concurrency is provider-aware:

| Provider | Concurrency                                   | Backoff                                |
| -------- | --------------------------------------------- | -------------------------------------- |
| Gmail    | 25 ops/sec sustained (quota-bound)            | Truncated exponential on 429           |
| Outlook  | 4 concurrent (MailboxConcurrency limit)       | `Retry-After` honoured; backoff on 429 |
| IMAP     | Pipelined within 1 connection, 1 conn/account | Per-server policy                      |
| POP3     | 1 op/sec                                      | Drop on first 4xx; mark `failed`       |

For predicate rows, ApplyOrchestrator pages through expansion (1,000 rows/page), interleaving expansion and apply so memory stays bounded.

Drift handling on dispatch:

1. Re-read the row's email_id from the local repository.
2. If preconditions still hold → dispatch.
3. If preconditions don't hold → `MarkOperationSkipped { reason: state_drift }`.
4. If the account has hard-drifted (etag invalidation seen during background sync) → halt that account's worker, surface `ApplyPaused { account_id, reason: hard_drift }` to the SSE stream.

Streams progress via SSE on `/api/v1/cleanup/apply/{jobId}/stream`. Resumable by re-issuing apply with the same `plan_id` and same or higher `risk_max`; rows already `applied` are skipped.

### RiskClassifier

Pure function: `(PlannedOperation, AccountContext) -> RiskLevel`. Centralises ADR-030 §6 rules:

```rust
fn classify(op: &PlannedOperation, ctx: &AccountContext) -> RiskLevel {
    match (op.action(), ctx.provider) {
        (Delete { permanent: true }, _) => High,
        (Delete { .. }, Provider::Pop3) => High,            // POP3 has no Trash
        (_, Provider::Pop3) => High,                         // POP3 ops are always best-effort
        (Unsubscribe { method: WebLink }, _) => High,
        (Unsubscribe { method: Mailto }, _) => Medium,
        (Unsubscribe { method: ListUnsubscribePost }, _) => Low,
        (Move { .. }, Provider::Imap | Provider::Outlook) => Medium,
        (Archive, Provider::Imap) => Medium,                 // depends on server Archive support
        (Archive, _) => Low,
        (AddLabel { .. }, _) => Low,
        (MarkRead | Star { .. }, _) => Low,
    }
    // Then escalate to High on bulk-thresholds:
    .escalate_if(group_size > 1000 || senders_in_group > 5 || engagement_rate > 0.10)
}
```

Single source of truth for risk. Used at build time (assigns to operation) and at UI render time (no recomputation).

### DriftDetector

Reads `account_state_etags` from the plan and compares against current per-account etags from Account Management. Returns one of:

```rust
pub enum DriftStatus {
    Clean,                                      // etag unchanged
    Soft { account_id, advanced_to: AccountStateEtag },     // etag advanced; row-level recheck
    Hard { account_id, reason: HardDriftReason },           // re-plan that account
}
```

Called on Apply start (per-account, in parallel) and at intervals during long apply jobs.

---

## Boundaries (deltas from base DDD-008)

This subdomain DOES own:

- The `CleanupPlan` aggregate, its operations (materialized + predicate), and apply jobs.
- Composition across subscription / cluster / rule / archive-strategy sources.
- Deterministic apply ordering, resumability, and risk-scoped partial apply.
- Risk classification of cleanup operations.
- Drift detection and account-scoped recovery.

This subdomain does NOT own:

- The provider-side mutation primitives — those remain on `EmailProvider` (ADR-018).
- The unsubscribe method discovery — remains in `backend/src/email/unsubscribe.rs`.
- Rule evaluation — remains in DDD-007.
- Cluster construction or topic embeddings — remains in Email Intelligence.

Cross-context contracts:

- **Email Intelligence** consumes `OperationApplied` to mark embeddings stale where category-affecting state changes (e.g., archive removes INBOX label).
- **Search** consumes `OperationApplied` to update label/folder facets without a full reindex.
- **Account Management** is the source of `account_state_etag`; the etag advances on any sync that changes a message's labels/folder.
- **Insights** consumes `ApplyFinished` to record a "cleanup run" entry in the activity log.

---

## Ubiquitous Language additions

| Term                          | Definition                                                                                               |
| ----------------------------- | -------------------------------------------------------------------------------------------------------- |
| **Cleanup plan**              | Immutable, materialized list of email operations awaiting user confirmation                              |
| **Materialized operation**    | One row in a plan with a concrete email_id; bounded by user clicks                                       |
| **Predicate operation**       | One row in a plan that represents a rule or strategy and expands to many materialized rows at apply time |
| **Plan source**               | Why a planned operation exists: subscription, cluster, rule, archive strategy, or manual                 |
| **Reverse op**                | The inverse of a planned operation, captured at build time, for future Undo                              |
| **Risk level**                | Per-operation classification (Low/Medium/High) used to gate Apply                                        |
| **Risk-scoped partial apply** | An apply job that operates only on rows ≤ a chosen risk_max                                              |
| **Account state etag**        | Provider-specific opaque snapshot per account; advances when label/folder state changes                  |
| **Plan hash**                 | Hash of selections + state; used to detect drift between Build and Apply                                 |
| **Apply job**                 | The execution lifecycle of one apply attempt against one plan; resumable; multiple per plan              |
| **Soft drift**                | Etag advanced but rows can be revalidated locally; drift-affected rows become `skipped`                  |
| **Hard drift**                | Etag invalidated (404 / 410 / UIDVALIDITY change); requires `RefreshAccount`                             |
| **EvaluateOnly (rules)**      | Mode of `RuleEngine` that returns matched targets and intended actions without invoking commands         |

---

## Migration / rollout

- **Schema migration**: additive only. Four new tables; no changes to `emails` or related tables.
- **API**: new `/api/v1/cleanup/plan` and `/api/v1/cleanup/apply/*` endpoints; existing `/api/v1/unsubscribe/*` and `/api/v1/rules/*` are unchanged.
- **Frontend**: `InboxCleaner.tsx` Step 4 → Step 4.5 (`CleanupReview`) → `CleanupProgress`. The simulated progress (`setInterval` at `InboxCleaner.tsx:168–182`) is removed in Phase C.
- **Deprecation**: the wizard's current single-shot Execute Cleanup path is the only deprecation; per-feature unsubscribe (Insights) remains as a shortcut.

---

## DDD-007 (Rules) Addendum: EvaluateOnly Mode

DDD-007 must add a non-mutating evaluation mode used by `PlanBuilder`:

```rust
pub enum RuleExecutionMode {
    Apply,        // existing default — emits commands to Email Operations
    EvaluateOnly, // NEW — returns intended actions without emitting commands
}

pub struct RuleEvaluation {
    pub rule_id: RuleId,
    pub matched_email_ids: Vec<EmailId>,        // bounded sample (5–20) at plan-build time
    pub projected_count: u64,                   // total estimated matches
    pub intended_actions: Vec<IntendedAction>,  // Move, AddLabel, Archive, Delete, Star, MarkRead
    pub match_basis: RuleMatchBasis,            // semantic / literal / hybrid; for explanation in UI
}

trait RuleEngine {
    async fn evaluate(&self, mode: RuleExecutionMode, scope: EvaluationScope)
        -> Result<Vec<RuleEvaluation>, RuleError>;
}
```

Invariants for `EvaluateOnly`:

- MUST NOT emit any DDD-008 command, fire any provider call, or mutate any aggregate.
- MUST be deterministic for a given `(rule_set, scope, snapshot)` — required for `plan_hash`.
- MUST be available offline (uses local repositories only).
- MUST return both a sampled `matched_email_ids` (for UI preview) and a `projected_count` (for plan totals); the full match list is materialized lazily by `expand_predicate` at apply time.

This is the only change required of the Rules context to support cleanup planning.
