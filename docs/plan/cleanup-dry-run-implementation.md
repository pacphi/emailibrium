# Cleanup Dry-Run — Technical Implementation Plan

| Field            | Value                                                                |
| ---------------- | -------------------------------------------------------------------- |
| Status           | **Phases A–D shipped (2026-05-05). 5 of 8 open follow-ups closed.**  |
| Date             | 2026-05-04 (drafted) · 2026-05-05 (delivered)                        |
| Realises         | ADR-030, DDD-008 Addendum, DDD-007 Addendum                          |
| Informed by      | `docs/research/cleanup-dry-run-due-diligence.md`                     |
| Estimated effort | 6–8 weeks (Phases A–D); Phase E (Undo) and F (cross-device) deferred |

This plan operationalises ADR-030 into concrete tasks with file-level targets, acceptance criteria, and ordering. It does not restate the design — read ADR-030 first.

## Implementation status (2026-05-05)

All four active phases shipped end-to-end across five commits on `main`:

| Commit    | Phase                                  | Test gate             |
| --------- | -------------------------------------- | --------------------- |
| `fc56b3a` | Phase A — backend domain + Plan API    | 31 cleanup + 56 rules |
| `8477c34` | Phase B — frontend review screen       | 5 Storybook scenarios |
| `98a42f3` | Phase C — apply + SSE + drift handling | 53 cleanup tests      |
| `7a3c474` | Phase D — risk + telemetry + history   | 67 cleanup tests      |
| `a1b44a1` | Close 5 of 8 open follow-ups           | 72 cleanup tests      |

`cargo test cleanup::` → **72 passed, 0 failed, 1 ignored** (the 50k-row PlanBuilder perf test, P95 = 71 ms vs 800 ms budget). `cargo clippy -p emailibrium --all-targets` (with the project's `-A dead_code …` allowance set) → clean. Web app `pnpm tsc --noEmit` and `pnpm lint` → clean. Every wizard-driven cleanup now flows Steps 1–4 → `POST /cleanup/plan` → Step 4.5 review → `POST /cleanup/apply` → SSE-driven `CleanupProgress`, with the original `setInterval` simulator deleted.

### Spec deviations adopted (conservative interpretations, surfaced here for the next reader)

1. **`backend/src/storage/` does not exist in the repo.** All storage actually lives under `backend/src/db/`. Treat every reference to `backend/src/storage/` in this plan and ADR-030 as a typo for `backend/src/db/`.
2. **The wire is camelCase end-to-end via `#[serde(rename_all = "camelCase")]`** on every cleanup DTO. The plan's mention of a "snake_case → camelCase transformer" in §B.2 is incorrect — no such transformer exists in `frontend/packages/api/src/`. Frontend types in `frontend/packages/types/src/cleanup.ts` consume the camelCase wire directly. `PlanStatus`, `SkipReason`, `PredicateStatus` JSON values are camelCase (`"partiallyApplied"`, `"stateDrift"`, `"userCancelled"`); the snake-cased `as_str()` form on the Rust enums is for DB-storage only.
3. **`engine.rs::execute` does not exist; the live rule engine is `backend/src/rules/rule_processor.rs`.** Phase A added `RuleExecutionMode::EvaluateOnly` types to `rules/types.rs` and a top-level `evaluate_rules()` to `rule_processor.rs`. The dead `engine.rs::evaluate(&mut self, &EmailContext)` was left untouched.
4. **`*_json` columns in migration 024 / 025 are plaintext `TEXT`.** ADR-030 §10 calls for an "encryption interceptor" pattern from ADR-016/017, but no such interceptor exists in `backend/src/db/` today (existing tables like `topic_clusters` also store JSON as plaintext). Switching to encrypted blobs later is additive (column-type swap + serializer wrapper). Tracked as deferred follow-up §3 below.
5. **`UnsubscribeService::execute` does not exist.** The closest method is `batch_unsubscribe(&[target])`. The `account_worker` dispatch path uses a single-element batch for unsubscribe rows.
6. **`InboxCleaner.tsx` simulator anchor.** Plan §C.3 says "lines 168–182"; today the simulator was at lines 146–198 (as of Phase B) and is now deleted entirely (Phase C). Only a one-line historical comment at `InboxCleaner.tsx:141` remains.
7. **`accountProviders` envelope field, `Pop3Sentinel` etag, `actionType` SSE field, `Snapshot` SSE variant, `JobCounts.skipped_by_reason`, and the 409 refresh-while-applying race-guard** are _additions_ on top of the original spec, surfaced by the cross-phase coherence reviews. They are now part of the lived contract.

### Closed follow-ups (commit `a1b44a1`)

- [x] OAuth-token-aware `EmailProvider` dispatch via the new `EmailProviderFactory` trait + `OAuthEmailProviderFactory` (Gmail/Outlook fully wired, IMAP/POP3 surface clear `provider_unsupported` errors).
- [x] Worker calls `PredicateExpander::expand_page` and emits `PredicateExpanded`; predicate status transitions Pending → Expanding → Expanded; expanded children get `seq > max(plan.seq)`.
- [x] `AccountStateEtag::Pop3Sentinel { last_uidl }` variant + `HardDriftReason::Pop3Invalidated` + drift detector mapping.
- [x] `CleanupPlan.account_providers: BTreeMap<String, Provider>` populated by `PlanBuilder` and included in `canonical_plan_hash`. Frontend reads it; the `accountStateEtags[id].kind === 'none'` POP3 inference hack is gone.
- [x] SSE op events (`OpApplied`, `OpFailed`, `OpSkipped`) carry `actionType: String`; `useCleanupApply` reducer aggregates `perAction: Record<string, {applied; failed; skipped}>` lazily.

### Deferred follow-ups (warrant separate ADRs)

1. **Per-row precondition re-read** (ADR-030 §8 rule 4) — needs schema work to track `emails.folder` + per-action invariant rules; touches the sync subsystem more than the cleanup domain. Phase E candidate.
2. **Encryption-at-rest interceptor for `*_json` columns** — workspace-wide infra (every encrypted table needs the wrapper). Belongs in a new `backend/src/db/encryption.rs` middleware module spanning `topic_clusters`, `mcp_tool_audit`, the new `cleanup_plan_*` tables, etc.
3. **Shared current-user store on the frontend** — every feature today reads `userId` from `localStorage`. `getCurrentUserId()` in cleanup-history is the single migration point when an auth context lands. Frontend infrastructure task.

### Smaller open items (in-code TODOs)

- IMAP/POP3 `EmailProviderFactory` support — providers' host/credentials aren't threaded into the orchestrator; currently surfaces as `OpFailed { error_code: "provider_unsupported" }`.
- `cleanup_plan_reviewed` time-on-review is a best-effort heuristic (component unmount after 2s). Acceptable; documented as such.

## Phase summary

| Phase | Theme                      | Effort   | Ships when                                                                    |
| ----- | -------------------------- | -------- | ----------------------------------------------------------------------------- |
| A     | Backend domain + Plan API  | ~2 weeks | `POST /cleanup/plan` returns a queryable plan; no apply yet                   |
| B     | Frontend Review screen     | ~1 week  | Step 4.5 reads a plan and shows the diff; existing Execute is gated behind it |
| C     | Apply + SSE + drift        | ~2 weeks | `POST /cleanup/apply?risk_max=...` works end-to-end; simulator removed        |
| D     | Risk + telemetry + history | ~1 week  | Per-row High acks, account tiles, plan history view, telemetry events         |
| E     | Undo via reverse_op        | deferred | Out of scope for this plan; tracked separately                                |
| F     | Cross-device sync (V2)     | deferred | Gated on ADR-015 maturing                                                     |

Phases A and B can ship independently. C depends on A. D depends on B and C.

---

## Phase A — Backend domain + Plan API (~2 weeks)

> **Status: shipped (commit `fc56b3a`).** All A.1–A.9 deliverables landed. Migration `024_cleanup_planning.sql` applies cleanly. PlanBuilder P95 = 71 ms on 50k fixture (target ≤ 800 ms). Mock-provider mutation-free invariant holds via type signature.

### A.1 Crate / module layout

New module: `backend/src/cleanup/` with submodules:

```text
backend/src/cleanup/
├── mod.rs                    // pub use exports
├── domain/
│   ├── mod.rs
│   ├── plan.rs               // CleanupPlan, PlannedOperation enum
│   ├── operation.rs          // PlanAction, PlanSource, RiskLevel, AccountStateEtag
│   ├── builder.rs            // PlanBuilder (domain service)
│   └── classifier.rs         // RiskClassifier
├── repository/
│   ├── mod.rs
│   ├── plan_repo.rs          // CleanupPlanRepository trait + SqliteCleanupPlanRepo
│   ├── job_repo.rs           // CleanupApplyJobRepository
│   └── migrations/
│       └── 20260504_cleanup_planning.sql
└── api/
    ├── mod.rs
    └── plan.rs               // POST /plan, GET /plan/:id/operations etc.
```

Apply orchestration lives in `backend/src/cleanup/orchestrator/` and is added in Phase C.

### A.2 Migration

Create `backend/migrations/20260504_cleanup_planning.sql`:

```sql
CREATE TABLE cleanup_plans (
    id              BLOB PRIMARY KEY,         -- UUID v7
    user_id         BLOB NOT NULL,
    created_at      INTEGER NOT NULL,         -- unix millis
    valid_until     INTEGER NOT NULL,
    plan_hash       BLOB NOT NULL,            -- 32 bytes blake3
    status          TEXT NOT NULL,            -- enum
    totals_json     BLOB,                     -- encrypted (ADR-016/017)
    risk_json       BLOB,
    warnings_json   BLOB
);
CREATE INDEX idx_cleanup_plans_user_status ON cleanup_plans(user_id, status);
CREATE INDEX idx_cleanup_plans_expiry ON cleanup_plans(valid_until);

CREATE TABLE cleanup_plan_account_etags (
    plan_id     BLOB NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    account_id  BLOB NOT NULL,
    etag_kind   TEXT NOT NULL,                -- 'gmail_history' | 'outlook_delta' | 'imap_uvms' | 'none'
    etag_value  BLOB,                         -- opaque per kind
    PRIMARY KEY (plan_id, account_id)
);

CREATE TABLE cleanup_plan_operations (
    plan_id         BLOB NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    op_kind         TEXT NOT NULL,            -- 'materialized' | 'predicate'
    account_id      BLOB NOT NULL,
    email_id        BLOB,                     -- materialized only
    predicate_kind  TEXT,                     -- predicate only
    predicate_id    BLOB,                     -- predicate only
    action          TEXT NOT NULL,
    target_kind     TEXT,                     -- 'folder' | 'label' | NULL
    target_id       TEXT,
    source_kind     TEXT NOT NULL,            -- 'subscription' | 'cluster' | 'rule' | 'strategy' | 'manual'
    source_id       TEXT,
    projected_count INTEGER,                  -- predicate only
    sample_ids_json BLOB,                     -- predicate only
    reverse_op_json BLOB,
    risk            TEXT NOT NULL,            -- 'low' | 'medium' | 'high'
    status          TEXT NOT NULL DEFAULT 'pending',
    applied_at      INTEGER,
    error           TEXT,
    partial_applied INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (plan_id, seq)
);
CREATE INDEX idx_cleanup_ops_status ON cleanup_plan_operations(plan_id, status);
CREATE INDEX idx_cleanup_ops_risk ON cleanup_plan_operations(plan_id, risk, status);
CREATE INDEX idx_cleanup_ops_account ON cleanup_plan_operations(plan_id, account_id);

CREATE TABLE cleanup_apply_jobs (
    job_id      BLOB PRIMARY KEY,
    plan_id     BLOB NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    state       TEXT NOT NULL,
    risk_max    TEXT NOT NULL,
    counts_json BLOB
);
CREATE INDEX idx_cleanup_jobs_plan ON cleanup_apply_jobs(plan_id);
```

All `*_json` columns use the existing encryption interceptor (ADR-016/017 pattern from `backend/src/storage/`).

### A.3 Domain types — `domain/plan.rs`, `domain/operation.rs`

Mirror the structs in DDD-008 addendum §Aggregates §3 verbatim. Use `serde::{Serialize, Deserialize}` for storage/wire. Use `uuid::Uuid` v7 (`uuid` crate) for `PlanId` / `JobId`.

Acceptance: `cargo test -p emailibrium cleanup::domain` covers:

- enum round-trip serde for all `PlanAction`, `PlanSource`, `AccountStateEtag` variants
- invariant checks (e.g., `target` required for `Move`, forbidden for `MarkRead`)
- `plan_hash` deterministic for identical inputs

### A.4 PlanBuilder — `domain/builder.rs`

`PlanBuilder` injection signature:

```rust
pub struct PlanBuilder {
    emails: Arc<dyn EmailRepository>,
    subs: Arc<dyn SubscriptionRepository>,
    clusters: Arc<dyn ClusterRepository>,
    rules: Arc<dyn RuleEngine>,                    // EvaluateOnly mode
    accounts: Arc<dyn AccountStateProvider>,       // returns AccountStateEtag
    unsubscribe: Arc<UnsubscribeService>,          // existing
    classifier: Arc<RiskClassifier>,
}

impl PlanBuilder {
    pub async fn build(&self, user: UserId, sel: WizardSelections) -> Result<CleanupPlan, BuildError>;
}
```

Internal flow:

1. Capture `account_state_etags` per account via `AccountStateProvider`.
2. For each `subscription` in selections → materialised rows via `unsubscribe.preview()`.
3. For each `clusterAction` → resolve email_ids from `clusters.emails(cluster_id)` → materialised rows.
4. For each `ruleSelection` → call `rules.evaluate(EvaluateOnly, scope)` → predicate row + sampled `matched_email_ids`.
5. For `archiveStrategy` → predicate rows per account.
6. Dedupe `(account_id, email_id, target)` keys; merge sources; record `PlanWarning::TargetConflict` on action conflicts.
7. Classify each row's risk via `RiskClassifier`.
8. Compute `plan_hash = blake3(canonical_json(selections, etags, ops))`.
9. Persist via `CleanupPlanRepository::save`.

Acceptance: integration test `cleanup_plan_build_smoke` runs PlanBuilder against an in-memory test fixture (10k emails, 50 subs, 10 clusters, 5 rules) and asserts:

- non-zero rows in each PlanSource category
- `plan_hash` stable across 3 rebuilds with identical inputs
- P95 build time < 800 ms on the 50k-row fixture
- dedup correctness (overlapping rule + cluster on same email → 1 op)

### A.5 RiskClassifier — `domain/classifier.rs`

Pure function per ADR-030 §6 / DDD-008 addendum. No I/O. Keep it in one file ≤ 200 lines.

Acceptance: table-driven unit tests for every (action × provider × bulk-context) combination listed in the addendum.

### A.6 Repository — `repository/plan_repo.rs`

`SqliteCleanupPlanRepo` implementing `CleanupPlanRepository`. Use `sqlx` consistent with the rest of `backend/src/storage/`. Cursor pagination on `list_operations` (cursor = `seq`). `expand_predicate` runs the underlying rule/strategy expansion via the injected `RuleEngine` / `EmailRepository` and persists the new materialized children with sequential `seq` values appended after existing rows.

Acceptance: repo unit tests cover save → load round-trip, cursor pagination correctness, status transitions.

### A.7 API — `api/plan.rs`

Routes (mounted under `/api/v1/cleanup`):

```rust
.route("/plan", post(create_plan))
.route("/plan/:plan_id", get(get_plan).delete(cancel_plan))
.route("/plan/:plan_id/operations", get(list_operations))
.route("/plan/:plan_id/sample", get(sample_operations))
.route("/plan/:plan_id/refresh", post(refresh_account))
.route("/plans", get(list_plans))
```

Request/response shapes mirror ADR-030 §9. Auth: existing user middleware. Validation: `axum::extract::Json` + `validator` crate.

Acceptance: API integration test posts wizard selections, gets back a planId, lists operations with pagination, samples one source, expires the plan via clock advance.

### A.8 DDD-007 Rules addendum — EvaluateOnly mode

Add `RuleExecutionMode` to `backend/src/rules/types.rs`. Modify `backend/src/rules/engine.rs` (`engine::execute`) to short-circuit on `EvaluateOnly`:

- Run the same matcher logic
- Skip the command emission step
- Return `Vec<RuleEvaluation>` with `matched_email_ids` capped at 20 (deterministic: head 5, tail 5, 10 stratified by date) and `projected_count = total_matches`.

Acceptance: existing rule tests pass; new test asserts `EvaluateOnly` returns the same matched set as `Apply` would have produced (without firing commands).

### A.9 Phase A definition of done

- [ ] Migration applied; schema verified in dev DB.
- [ ] `cargo test -p emailibrium cleanup::` passes.
- [ ] Manual smoke: `curl POST /api/v1/cleanup/plan` with a hand-crafted body returns `201` with a planId, then `GET /plan/:id/operations` returns paginated rows.
- [ ] PlanBuilder benchmark: P95 ≤ 800 ms on the 50k-row fixture.
- [ ] No mutation paths reach a provider during plan build (verified by mock provider that fails on any call).

---

## Phase B — Frontend Review screen (~1 week)

> **Status: shipped (commit `8477c34`).** All B.1–B.5 deliverables landed except the explicit Lighthouse a11y measurement and the Playwright e2e (semantic markup, `aria-disabled`, `aria-label`, role attributes are in place; a11y measurement deferred until a CI dev-server harness exists). 5 Storybook scenarios ship: empty / low-only / mixed-risk / large-with-warnings / conflict-warnings.

### B.1 Component layout

```text
frontend/apps/web/src/features/inbox-cleaner/
├── InboxCleaner.tsx                 // existing — modified: route to CleanupReview after Step 4
├── steps/
│   └── ...                          // existing Step1–4
├── review/                          // NEW
│   ├── CleanupReview.tsx            // root for Step 4.5
│   ├── AccountSummaryTile.tsx
│   ├── PlanDiffGroup.tsx
│   ├── PlanDiffRow.tsx
│   ├── RiskLegend.tsx
│   ├── RiskAcknowledger.tsx
│   ├── SampleEmailPeek.tsx
│   ├── RefreshAccountAffordance.tsx
│   ├── ApplyButtons.tsx             // three buttons: Low / Low+Medium / All
│   └── PlanWarningsBanner.tsx
└── hooks/
    ├── useInboxCleaner.ts           // existing — extend with buildPlan/applyPlan/refreshAccount
    ├── usePlanOperations.ts         // NEW — paginated, filtered ops query
    └── usePlanSamples.ts            // NEW
```

### B.2 API client

Add to `frontend/packages/api/src/cleanup.ts`:

```ts
export const buildPlan: (sel: WizardSelections) => Promise<CleanupPlanSummary>;
export const getPlan: (id: PlanId) => Promise<CleanupPlanEnvelope>;
export const listPlanOperations: (id: PlanId, filter, cursor) => Promise<Page<PlannedOperation>>;
export const samplePlanOperations: (id, source, n) => Promise<Email[]>;
export const refreshPlanAccount: (id, accountId) => Promise<void>;
export const cancelPlan: (id) => Promise<void>;
// (apply functions arrive in Phase C)
```

Add `CleanupPlanEnvelope` / `PlannedOperation` / `RiskLevel` to `frontend/packages/types/src/cleanup.ts`. **Keep type names symmetric with the backend's serde output** (snake_case → camelCase via existing transformer).

### B.3 CleanupReview wireframe (text)

```text
┌─ Step 4.5: Review & Confirm ─────────────────────────────────┐
│ 2,847 emails to clean across 2 accounts • 11 hrs/mo saved    │
│ Risk: 2,401 Low ▮▮▮▮▮  430 Medium ▮▮  16 High ▮              │
│                                                              │
│ ┌─ Gmail (work@x.com) ───────────────────┐                   │
│ │ Archive 2,142 emails              Low  │ [▾]               │
│ │ Add label 'Receipts/2026' (212)   Low  │ [▾]               │
│ │ Unsubscribe 8 senders             Low  │ [▾]               │
│ │ Move 4 to Folder 'Old Projects'   Med  │ [▾] [✓ ack group] │
│ └────────────────────────────────────────┘                   │
│ ┌─ Outlook (personal@y.com) ─────────────┐                   │
│ │ Archive 480 emails                Low  │ [▾]               │
│ │ Delete permanent 16 messages      High │ [▾] [✓ ack each]  │
│ └────────────────────────────────────────┘                   │
│                                                              │
│ ⚠ Group "Archive 2,142" exceeds 1k — sample carefully.       │
│                                                              │
│ [Apply Low only]  [Apply Low + Medium]  [Apply all (16 High)]│
└──────────────────────────────────────────────────────────────┘
```

### B.4 Behaviour

- On wizard "Next" from Step 4: call `buildPlan(selections)`, navigate to `/cleanup/review/:planId`, render `<CleanupReview planId=... />`.
- Each `PlanDiffGroup` is virtualized (react-virtual) to handle large groups; default 20 rows visible, expand on click.
- `SampleEmailPeek` calls `samplePlanOperations` lazily on hover/expand.
- `RiskAcknowledger` tracks per-row High acks and per-group Medium acks in component state; serializes them into the apply call.
- Apply buttons are disabled until required acks exist.
- "Refresh just this account" is offered when the SSE in Phase C signals hard drift; in Phase B it is also reachable from a "Plan was built X minutes ago — refresh" affordance after 25 minutes.

### B.5 Phase B definition of done

- [ ] Visiting `/cleanup/review/:planId` renders without errors against a Phase A backend.
- [ ] All three Apply buttons are disabled with no acks; correct ones enable as acks accumulate.
- [ ] Lighthouse a11y ≥ 95 on Review screen.
- [ ] Storybook stories for empty plan, low-only plan, mixed-risk plan, large-plan-with-warnings, conflict-warnings.
- [ ] e2e test (Playwright) clicks through Step 1–4 with seeded data, lands on Review, asserts visible rollups match the API response.

---

## Phase C — Apply + SSE + drift handling (~2 weeks)

> **Status: shipped (commit `98a42f3`).** All C.1–C.6 deliverables landed. `setInterval` simulator deleted from `InboxCleaner.tsx`; the ApplyEvent union grew an extra `Snapshot` variant emitted as the first event on every SSE subscription (architecture-review addition); `JobCounts` gained `skipped_by_reason: BTreeMap<SkipReason, u64>` (additive); `POST /plan/:id/refresh` returns `409 { error: "apply_in_progress" }` when a Running job exists for the plan (race-guard).

### C.1 ApplyOrchestrator — `backend/src/cleanup/orchestrator/`

```text
orchestrator/
├── mod.rs
├── apply.rs              // ApplyOrchestrator (struct + run loop)
├── drift.rs              // DriftDetector
├── account_worker.rs     // per-account worker with provider-aware concurrency
├── expander.rs           // predicate → materialized rows lazy expansion
└── sse.rs                // SSE event types + emitter
```

Run loop:

1. Validate plan: `valid_until > now`, `status == ready`.
2. Validate request: `risk_max ≥ all unacked Highs filtered out`, all Medium groups within `risk_max` are acknowledged.
3. Run `DriftDetector::detect_all(plan)` — if any account is `Hard`, refuse the job with 409 + per-account drift list.
4. Create `cleanup_apply_jobs` row, return `202 + jobId`.
5. Spawn one tokio task per account, each with provider-specific concurrency cap.
6. Each account worker:
   - Stream pending rows for its account in `seq` order, filtered by `risk_max` and ack list.
   - For predicate rows: page-expand 1k at a time, append to `cleanup_plan_operations` (fresh seq), interleave with apply.
   - Per row: revalidate against local repo → dispatch via `EmailProvider` → mark applied/failed/skipped.
   - Honour 429 backoff (`Retry-After` for Outlook; truncated exponential for Gmail).
   - Emit SSE events on every status transition.
7. When all account workers exit: `FinishApply`, transition plan status, emit `ApplyFinished`.

### C.2 SSE event schema

```ts
type ApplyEvent =
  | { type: 'started'; jobId; planId; totalsByAccount }
  | { type: 'op_applied'; seq; accountId; appliedAt }
  | { type: 'op_failed'; seq; accountId; error }
  | { type: 'op_skipped'; seq; accountId; reason: 'state_drift' | 'dedup' }
  | { type: 'predicate_expanded'; predicateSeq; producedRows }
  | { type: 'account_paused'; accountId; reason: 'hard_drift' | 'rate_limit' | 'auth_error' }
  | { type: 'account_resumed'; accountId }
  | { type: 'progress'; counts: { applied; failed; skipped; pending } }
  | { type: 'finished'; jobId; status: 'applied' | 'partially_applied' | 'failed'; counts };
```

Emit `progress` every 250 ms or every 50 ops (whichever is sooner) to keep the wire chatty enough but bounded.

### C.3 Frontend wiring

- `useCleanupApply(planId)` hook opens an SSE connection on Apply click, manages reconnection, exposes `state` / `counts` / `accountStatus`.
- Replace `CleanupProgress.tsx` simulator (`InboxCleaner.tsx:168–182`) with this hook.
- Surface `account_paused { reason: 'hard_drift' }` as the "Refresh this account" affordance from Phase B.
- Show `op_skipped` aggregate count in the progress UI ("431 emails skipped — they changed since you reviewed").

### C.4 Provider-specific concurrency

`backend/src/email/{gmail,outlook,imap}.rs` already implement `EmailProvider`. Add per-provider worker config in `backend/src/cleanup/orchestrator/account_worker.rs`:

| Provider | Concurrency             | Notes                                                                 |
| -------- | ----------------------- | --------------------------------------------------------------------- |
| Gmail    | 25 ops/sec rate-limited | Use `governor` crate token bucket; group label adds via batchModify   |
| Outlook  | 4 concurrent semaphore  | Use Microsoft Graph JSON batch (cap 20) with `dependsOn` for ordering |
| IMAP     | 1 connection, pipelined | Reuse existing `imap` crate connection pool                           |
| POP3     | 1 op/sec                | Bail on first 4xx                                                     |

### C.5 DriftDetector — `orchestrator/drift.rs`

Read each account's current etag from `AccountStateProvider`, compare with `cleanup_plan_account_etags` snapshot. Classify per ADR-030 §8 table. On `Hard` drift, never auto-rebuild — surface to the user.

`POST /api/v1/cleanup/plan/:id/refresh?accountId=` deletes that account's rows in `cleanup_plan_operations`, re-runs the per-account portion of `PlanBuilder`, and updates the etag tuple. Plan_hash is recomputed; status reverts to `ready`.

### C.6 Phase C definition of done

- [ ] `POST /api/v1/cleanup/apply/:planId?risk_max=low` returns 202 and applies all Low rows for a multi-account fixture.
- [ ] SSE stream delivers `started`, intermittent `progress`, terminal `finished` against a real test mailbox (Gmail sandbox).
- [ ] Cancel mid-apply: applied rows stay applied, pending rows transition to `skipped: user_cancelled`.
- [ ] Partial-apply round trip: apply Low → Apply Medium → Apply High, all in sequence on the same plan.
- [ ] Predicate row expansion: 10k-row predicate expanded and applied in pages without OOM (verified by RSS observation).
- [ ] Hard drift simulation: invalidate Gmail historyId mid-apply → SSE emits `account_paused: hard_drift` → frontend Refresh → re-apply succeeds.
- [ ] `setInterval` simulator in `InboxCleaner.tsx` is removed.

---

## Phase D — Risk + telemetry + history (~1 week)

> **Status: shipped (commit `7a3c474`, plus follow-ups in `a1b44a1`).** All D.1–D.4 deliverables landed. New `cleanup_audit_log` table (migration `025`) with append-only writes from `account_worker`; `cleanup_plan_*` telemetry events emit via `tracing::info!` to target `cleanup.telemetry`; `/cleanup/history` + `/cleanup/history/:planId` routes ship with main-nav entry; `CleanupReview.readOnly` mode for past plans; POP3 typed-confirmation gate ("type DELETE"); action-specific High-risk acknowledgement copy. The audit table schema is enforced free of plan content (no `email_id`, body, folder paths, sender, rule body, or sample ids) by the `audit_excludes_email_content` test using `pragma_table_info` introspection.

### D.1 Frontend polish

- `RiskLegend` always visible on Review.
- `AccountSummaryTile` — account-bordered cards at top of Review (per due-diligence §3.3).
- `PlanWarningsBanner` for `TargetConflict` and large-group warnings.
- Per-row plain-English explainer ("This will move 142 emails from `marketing@saas.com` to Trash — recoverable for 30 days") generated client-side from `PlanAction` + `target` + provider.
- Plan history view: `/cleanup/history` — list user's last 20 plans with status, totals, applied counts; click into read-only review.

### D.2 Telemetry events (via existing analytics pipeline)

- `cleanup_plan_built` — totals, build_duration_ms, accounts, warnings.
- `cleanup_plan_reviewed` — time_on_review_ms, expanded_groups, samples_viewed.
- `cleanup_apply_started` — risk_max, ack_counts.
- `cleanup_apply_finished` — applied, failed, skipped, duration_ms.
- `cleanup_plan_refreshed` — account_id, reason.

These are key inputs for the V2 cross-device-sync gating decision (do users actually want sync?) and for the partial-apply uptake measurement (is risk-scoped apply being used?).

### D.3 Audit trail

Every `OperationApplied` / `OperationFailed` / `OperationSkipped` event writes one append-only row to the existing audit log (DDD-005 / ADR-017). Required for GDPR right-to-explanation.

### D.4 Phase D definition of done

- [ ] Review screen visually matches the wireframe in B.3 (Storybook + a11y review).
- [ ] Plan history list view ships and is reachable from the user menu.
- [ ] All five telemetry events fire with the documented payloads.
- [ ] Audit-log entries written for a multi-account apply test.
- [ ] User-facing strings reviewed for clarity (especially High-risk acknowledgement copy).

---

## Cross-cutting concerns

### Performance budgets

| Operation                        | Target P95                   |
| -------------------------------- | ---------------------------- |
| `POST /cleanup/plan` (50k inbox) | ≤ 800 ms                     |
| `GET /plan/:id/operations` (1k)  | ≤ 150 ms                     |
| Predicate expansion (1k rows)    | ≤ 300 ms                     |
| SSE event latency                | ≤ 50 ms server emit          |
| Review screen TTI (10k rows)     | ≤ 600 ms with virtualization |

### Security

- All plan storage encrypted at rest (existing ADR-016/017 interceptor).
- Plan endpoints require authenticated user; explicit `user_id` predicate on every repo query.
- No plan content is logged; only counts.
- POP3 plans always require typed-confirmation ("type DELETE") in addition to per-row ack — added in Phase D.

### Observability

- Structured logs at INFO for plan-built, apply-start/finish; WARN for hard-drift, rate-limit pause; ERROR for unrecoverable provider errors.
- Existing OpenTelemetry traces wrap plan build and each account worker.

### Testing strategy

| Layer                         | Tool                            | Coverage target                             |
| ----------------------------- | ------------------------------- | ------------------------------------------- |
| Domain types & RiskClassifier | cargo test                      | ≥ 95% line                                  |
| PlanBuilder                   | cargo test (in-memory fixtures) | dedup, conflict, hash determinism, perf P95 |
| Repository                    | cargo test (sqlx test pool)     | round-trip + concurrent status updates      |
| API                           | cargo test (axum-test)          | happy path + auth + error cases             |
| ApplyOrchestrator             | cargo test (mock providers)     | drift, cancel, resume, predicate expansion  |
| Frontend components           | Vitest + Storybook              | renders, ack-state machine                  |
| End-to-end                    | Playwright + Gmail sandbox      | full wizard → review → apply happy path     |

### Backwards compatibility

None required — the wizard's Execute button currently runs a frontend simulator; switching it to plan/apply has no compat surface. The existing `/api/v1/unsubscribe/*` endpoints are untouched.

---

## Risk register (implementation-specific)

| Risk                                                           | Likelihood | Impact | Mitigation                                                     |
| -------------------------------------------------------------- | ---------- | ------ | -------------------------------------------------------------- |
| `RuleEngine::EvaluateOnly` re-implementation drifts from Apply | Med        | Med    | Shared matcher; integration test asserts equivalence           |
| Outlook MailboxConcurrency throttles dominate apply latency    | High       | Low    | JSON-batch with `dependsOn`; show estimated time on Review     |
| Predicate expansion blows up (rule matches ~all emails)        | Med        | High   | Hard cap of 1M materialized rows per predicate; show warning   |
| Plan accumulation bloats SQLite                                | Low        | Low    | `expire_due` + `purge_older_than` cron at `valid_until + 7d`   |
| User clicks Apply, mailbox drifts during 30s preflight         | Low        | Low    | Re-run DriftDetector at apply start; soft-skip on row dispatch |
| Hard drift mid-apply leaves user confused                      | Med        | Med    | Clear UI: "Account X paused. Refresh to continue."             |

---

## Out-of-scope (tracked separately)

- **Phase E — Undo via reverse_op.** Requires (a) reverse_op vocabulary stable, (b) UI for "Undo last cleanup" with audit context, (c) idempotency of undo apply. Will be its own ADR + plan.
- **Phase F — Cross-device plan sync.** Deferred per ADR-030 §12 / due-diligence §4.3. Re-evaluate after Phase D telemetry shows real user demand and ADR-015 (offline-sync) ships.
- Scheduled apply ("apply tonight at 2am").
- Saved plan templates.
- Multi-user shared plans.
