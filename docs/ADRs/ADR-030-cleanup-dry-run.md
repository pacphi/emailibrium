# ADR-030: Cleanup Dry-Run and Plan/Apply Separation

- **Status**: Accepted
- **Date**: 2026-05-04
- **Extends**: DDD-008 (Email Operations), DDD-007 (Rules), ADR-018 (Provider Folder/Label Operations)
- **Supersedes (partially)**: Inbox Cleaner Wizard execute path described in `docs/plan/inception.md` §10.3 (single-shot Execute Cleanup button)
- **Informed by**: `docs/research/cleanup-dry-run-due-diligence.md` (competitive analysis, provider technical limits, resolution of all three originally-deferred questions)
- **Implementation plan**: `docs/plan/cleanup-dry-run-implementation.md`

## Context

The Inbox Cleaner Wizard (FEAT-052, `frontend/apps/web/src/features/inbox-cleaner/`) currently progresses from selection (Steps 1–4) directly to a `CleanupProgress` overlay. The "Execute Cleanup" button at Step 4 (`InboxCleaner.tsx:384`) immediately commits the user's selections and the progress is currently driven by a `setInterval` simulator (no real apply endpoint is wired in). The summary bar exposes only three aggregate numbers (subscriptions selected, emails-to-clean, hours/month saved); there is no per-message, per-account, per-folder enumeration of the changes the user has authorized.

Promised UX in `docs/user-interface-overview.md:64`: "each step offers batch archive, delete, and unsubscribe actions with a **preview before anything is committed**." Today, only the unsubscribe slice has a true preview — `backend/src/email/unsubscribe.rs` (`UnsubscribePreview`, `service.preview()`), exposed via `GET/POST /api/v1/unsubscribe/preview` (`backend/src/api/unsubscribe.rs:25`). It is reachable from `frontend/apps/web/src/features/insights/SubscriptionsPanel.tsx` but **not** from the wizard. Topic-cluster archive/delete (Step 3) and rule-based moves (Step 4) have no preview path. ADR-018, DDD-007, and DDD-008 do not define a dry-run, simulation, or evaluate-without-mutate verb on the provider/rules abstractions.

This is a high-blast-radius gap. The wizard's value proposition is "10,000 emails to zero in under ten minutes" across multiple accounts and providers (Gmail labels are additive, Outlook/IMAP folder moves are exclusive — see ADR-018). A user committing the wizard cannot see, before commit, which messages will be archived, which labels will be added, which folders things will move into, or which subscriptions will be unsubscribed via List-Unsubscribe POST vs. mailto vs. unsubscribe-link. Mistakes are recoverable on Gmail (Trash retention 30 days) and Outlook (Deletions 14d default, 30d max) but expensive to recover from (rate-limited APIs, manual triage), and POP3 deletes are irrecoverable.

The competitive read (full detail in the due-diligence doc): Mailstrom and Clean Email do per-bundle previews; SaneBox is the opposite philosophy ("AI decides for you"); Unroll.me is subscription-only; mailtrim is the closest analog (Gmail-only CLI `--dry-run`). **None offer a unified, cross-source, materialized, partially-appliable plan that spans subscriptions + clusters + rules + archive strategy in one reviewable artifact.** That is the differentiator this ADR delivers.

## Decision

Split cleanup into two explicit phases: **Plan** (pure, side-effect-free, materialized, reviewable) and **Apply** (idempotent execution of a previously-built plan, with optional risk-scoped partial apply). The wizard gains a new Step 4.5 — "Review & Confirm" — that renders the plan and is the only path to apply.

### 1. Plan / Apply lifecycle

```text
┌─ Steps 1–4: Selections ─┐    ┌─ POST /cleanup/plan ─┐    ┌─ Step 4.5: Review ─┐    ┌─ POST /cleanup/apply/{planId} ─┐
│ subs, clusters, rules,  │ -> │ evaluate, enumerate, │ -> │ user diffs, risk-  │ -> │ idempotent execute,            │
│ archive strategy        │    │ persist plan         │    │ scope, confirms    │    │ stream progress (SSE)          │
└─────────────────────────┘    └──────────────────────┘    └────────────────────┘    └────────────────────────────────┘
```

A plan is an immutable, persisted artifact identified by a `PlanId` (UUID v7). A plan has a `valid_until` (default 30 minutes) and a `plan_hash` over the inputs and the resolved local state. Apply rejects plans whose `valid_until` has expired or whose underlying provider state has drifted beyond a soft-recoverable threshold (see §8 Drift policy).

### 2. Provider-side dry-run is forbidden

Plan does **not** call provider mutation APIs. It uses already-synced local state (`EmailRepository`, `SubscriptionRepository`, `ClusterRepository`, `RuleRepository`) plus pure evaluation of rules in **EvaluateOnly** mode (DDD-007 addendum). Unsubscribe method discovery uses cached headers from prior sync only; no probing requests. This keeps Plan fast (target: P95 ≤ 800 ms for 50 k inbox), free of rate-limit risk, and deterministic.

### 3. Plan materialization — hybrid (Materialized | Predicate)

A `CleanupPlan` is hybrid:

- **Materialized rows** for explicit user-selected actions (subscription unsubscribe, cluster archive/delete, manual selections). One row per `(account_id, email_id, action)`. Bounded by user clicks; typically O(thousands).
- **Predicate rows** for rule-driven and archive-strategy-driven actions. Stored as `(predicate_kind, predicate_id, account_id, projected_count, sample_email_ids[5..20])`. Expanded to message-level rows lazily by ApplyOrchestrator, in pages of 1,000.

Per-message rows are stored compactly (account_id, email_id, action, source). The plan also contains pre-computed group rollups (per-account, per-source, per-action, per-risk) so the UI can render summaries without re-aggregating millions of rows client-side.

**No hard cap on plan size.** Soft thresholds:

- Group > 10,000 projected rows → Review screen shows a "Large operation — verify the sample carefully" banner.
- Total plan > 100,000 projected rows → Review requires an explicit "I have reviewed and accept the scale of this operation" acknowledgement before Apply.

### 4. Idempotency, partial apply, and resumability

Apply walks the plan in a deterministic order (account → action → source → seq → email_id). Each operation is idempotent at the provider boundary (already required by DDD-008 invariants). Apply is resumable: a plan tracks per-row status (`pending | applied | failed | skipped`), and re-issuing apply continues from the first non-applied row.

There is no in-flight transactional rollback across providers — that is impossible across Gmail/Outlook/IMAP — but every applied row records a `reverse_op` envelope where reversible (e.g., remove-label-INBOX is reversed by add-label-INBOX) so a follow-up "Undo last cleanup" can be implemented (Phase E).

### 5. Preview is provider-uniform

The plan describes operations in DDD-008 / ADR-018 vocabulary (`MoveKind::Folder | Label`, target `FolderOrLabel`). The UI shows the user the **post-state** in plain English, not the provider-specific call. For example, a Gmail "archive" plan row reads "Remove from Inbox", an Outlook archive row reads "Move to Archive folder", and a Gmail rule-applied label reads "Add label 'Receipts/2026'". This is symmetric with the existing provider abstraction.

### 6. Risk classification

Each plan operation is annotated with a risk level:

**Low** (no extra acknowledgement required)

- Archive on Gmail/Outlook (reversible from Trash 14–30d).
- Label add (Gmail) / category add (Outlook).
- Mark read / star.
- Unsubscribe via List-Unsubscribe POST (RFC 8058 — server-side, silent).

**Medium** (per-group acknowledgement)

- Move to a non-system folder on IMAP / Outlook (reversible but cross-folder).
- Unsubscribe via mailto (sends user mail; visible side effect).
- Archive on IMAP (folder move — depends on server's Archive support).

**High** (per-row acknowledgement; Apply blocked until acknowledged)

- Delete-permanent on any provider.
- Any delete on POP3 (always irrecoverable by protocol).
- Move-to-Trash on POP3 (no Trash; equivalent to permanent delete).
- Unsubscribe via web link (opens browser tab; opaque outcome).
- Bulk threshold escalation: any single source group with > 1,000 ops, or > 5 senders touched in one source, or any account where engagement_rate > 0.10 — the existing `UnsubscribePreview` warning logic generalized to all sources.

The Review screen blocks Apply on any High-risk row until the user explicitly acknowledges. Medium-risk rows require a single per-group click. Low-risk rows have no per-row gating.

### 7. Risk-scoped partial apply (first-class)

The Review screen exposes three Apply buttons:

- **Apply Low risk only** — applies all Low rows; Medium/High remain `pending` in the plan.
- **Apply Low + Medium** — applies Low and acknowledged Medium rows.
- **Apply all (incl. High)** — applies everything, gated by per-row High acknowledgements.

This is wired through the same endpoint via `?risk_max=`. Plans support multiple sequential apply jobs (each on a disjoint subset of pending rows) until all rows are applied, skipped, or the plan expires. This is the strongest unique-value lever vs. competitors and is intentionally first-class, not a deferred enhancement.

### 8. Account-scoped drift policy

`account_state_etag` is per-account, stored as a tuple in the plan envelope: `{account_id, etag_kind, etag_value}`.

| Provider | etag_kind           | Concrete value                 | Soft drift signal                   | Hard drift signal          |
| -------- | ------------------- | ------------------------------ | ----------------------------------- | -------------------------- |
| Gmail    | `historyId`         | `"123456789"`                  | history advanced, `history.list` OK | `history.list` returns 404 |
| Outlook  | `deltaToken`        | opaque `$deltatoken`           | delta returns changes               | delta-token invalid (410)  |
| IMAP     | `uidvalidityModseq` | `(UIDVALIDITY, HIGHESTMODSEQ)` | HIGHESTMODSEQ advanced              | UIDVALIDITY changed        |
| POP3     | `none`              | n/a                            | n/a (any prior op invalidates)      | any prior op               |

Rules:

1. Drift on account A invalidates **only that account's rows**, never the entire plan.
2. **Soft drift recovery is automatic.** ApplyOrchestrator re-checks each pending row against the local repository on dispatch; if the row's preconditions no longer hold (label/folder changed, message deleted, etc.), the row is marked `skipped` with reason `state_drift`. Aggregate skip counts are surfaced in the progress UI.
3. **Hard drift requires user acknowledgement.** Apply pauses for that account, surfaces "Account X changed substantially since you reviewed this plan", and offers a "Refresh just this account" affordance that rebuilds only that account's plan rows.
4. Revalidation cost is local-only — we re-read the synced repository, never the provider — so it is cheap to do per-row.

### 9. API surface

```text
POST /api/v1/cleanup/plan
  body: WizardSelections { subscriptions[], clusterActions[], ruleSelections[], archiveStrategy, accountIds[] }
  resp: 201 Created { planId, validUntil, totals, perAccount, perAction, perRisk, warnings[] }

GET  /api/v1/cleanup/plan/{planId}                             // envelope only
GET  /api/v1/cleanup/plan/{planId}/operations?cursor=&risk=&action=&accountId=
GET  /api/v1/cleanup/plan/{planId}/sample?source=&n=5          // representative emails per group
POST /api/v1/cleanup/plan/{planId}/refresh?accountId=          // rebuild account-scoped rows after hard drift
DELETE /api/v1/cleanup/plan/{planId}                           // user-initiated cancel; transitions to `cancelled`

POST /api/v1/cleanup/apply/{planId}?risk_max=low|medium|high   // default: low
  body: ApplyOptions { acknowledgedHighRiskSeqs[], acknowledgedMediumGroups[] }
  resp: 202 Accepted { jobId }
GET  /api/v1/cleanup/apply/{jobId}/stream                      // SSE progress (reuses ingestion progress pattern)
POST /api/v1/cleanup/apply/{jobId}/cancel                      // cancel pending rows; applied rows stand
GET  /api/v1/cleanup/apply/{jobId}                             // job status snapshot

GET  /api/v1/cleanup/plans?status=&limit=                      // user's plan history (for V1 simple list view)
```

`risk_max` filters the rows the apply job operates on. A plan can be applied multiple times with increasing `risk_max` until exhausted. Existing `/api/v1/unsubscribe/preview` and `/api/v1/unsubscribe` remain — they are the per-feature shortcut from Insights — and the new endpoint composes the same `UnsubscribeService::preview` internally.

### 10. Storage

A new SQLite table cluster (encrypted at rest per ADR-016/017):

- `cleanup_plans` — plan envelope (id, user_id, created_at, valid_until, plan_hash, status, totals_json, risk_json).
- `cleanup_plan_account_etags` — per-account etag tuple at build time (plan_id, account_id, etag_kind, etag_value).
- `cleanup_plan_operations` — one row per planned operation. Discriminator `op_kind = materialized | predicate`. For materialized: `(plan_id, seq, account_id, email_id, action, target_kind, target_id, source_kind, source_id, reverse_op_json, risk, status, applied_at, error)`. For predicate: `(plan_id, seq, account_id, predicate_kind, predicate_id, action, target_kind, target_id, projected_count, sample_email_ids_json, risk, status, partial_applied_count, error)`.
- `cleanup_apply_jobs` — apply lifecycle (job_id, plan_id, started_at, finished_at, state, risk_max, counts_json).

Plans are user-scoped, GDPR-deletable (DDD-005 / ADR-017), and auto-expired after `valid_until + 7 days`.

### 11. Frontend changes

- `InboxCleaner.tsx` adds a new `Step4_5_Review` between Step 4 and `CleanupProgress`. The current `handleExecuteCleanup` simulator (lines 168–182) is removed.
- New components in `frontend/apps/web/src/features/inbox-cleaner/`:
  - `CleanupReview.tsx` — root with summary, per-account breakdown, per-action drill-down, risk legend, sample emails, risk acknowledgement, three Apply buttons (Low / Low+Medium / All).
  - `PlanDiffGroup.tsx`, `PlanDiffRow.tsx`, `RiskAcknowledger.tsx`, `SampleEmailPeek.tsx`, `AccountSummaryTile.tsx`, `RefreshAccountAffordance.tsx`.
- `useInboxCleaner` exposes `buildPlan()`, `currentPlan`, `applyPlan({ riskMax, acks })`, `refreshAccount(accountId)`. `CleanupProgress` switches from `setInterval` to SSE.
- The wizard's existing aggregate summary bar remains as the in-flight tally; Step 4.5 is the authoritative review.

### 12. Out of scope (deferred)

- **Cross-account undo / replay** — `reverse_op` envelopes are captured today; full Undo flow is Phase E.
- **Cross-device plan sync** — V1 plans are device-local. JSON export/import is the V1 escape hatch. CRDT-based sync (Yjs or Loro) is V2 / Phase F, gated on ADR-015 (offline-sync) maturing. Rationale in due-diligence doc §4.3.
- Scheduling apply for later ("apply tonight").
- Plan templates / saved plans.
- Multi-user shared plans.

## Consequences

**Positive**

- Closes the user-facing trust gap: every destructive action is reviewable in DDD-008 vocabulary before commit.
- Decouples the wizard from execution latency: Plan is fast and runs on local state; Apply streams against rate-limited provider APIs without the user sitting on a modal.
- Risk-scoped partial apply is a genuinely novel feature in this market, not present in Mailstrom / Clean Email / SaneBox / Unroll.me / mailtrim.
- Enables retry / resume — an apply that fails halfway is restartable from the persisted plan.
- Centralises preview logic; the existing per-feature unsubscribe preview becomes a special case.
- Risk annotations create a uniform place to introduce safeguards for new providers (e.g., POP3's irrecoverable delete).

**Negative / costs**

- Net new SQLite tables and migration (low risk, additive).
- New dependency between Email Operations, Rules, and Subscriptions contexts via the new Cleanup Planning subdomain (DDD-008 addendum). Boundary discipline must be enforced.
- Hybrid Materialized/Predicate model adds a small amount of code complexity at the read API and ApplyOrchestrator. Justified by avoiding hard caps on plan size.
- Slightly slower path to "Apply" — one extra screen. Acceptable; this is the entire point.

**Risks / mitigations**

- Plan/apply drift if mailbox changes between phases → guarded by per-account etag check, soft-drift skip, hard-drift pause with refresh affordance.
- Provider rate-limit ceilings constrain throughput (Outlook MailboxConcurrency=4, Gmail batchDelete=50 quota units/op) → ApplyOrchestrator has per-account concurrency caps and provider-aware backoff.
- User abandons plan → auto-expire and GC via `valid_until`.
- Provider partial failure mid-apply → resumable via per-row status; user-visible error column on Review's history view.
- Large plan (≥ 100k rows) UX degradation → predicate-row lazy expansion and the > 100k acknowledgement gate.

## Implementation phasing

See `docs/plan/cleanup-dry-run-implementation.md` for the full implementation plan with task breakdown, ownership, and acceptance criteria.

1. **Phase A (backend domain)** — Domain types, storage, `POST /cleanup/plan` + `GET .../operations` (no apply). DDD-007 EvaluateOnly mode. Reuse `UnsubscribeService::preview`.
2. **Phase B (frontend review)** — Step 4.5 (`CleanupReview`) reading plans; the existing Execute button is gated behind the review.
3. **Phase C (apply)** — `POST /cleanup/apply` with `?risk_max=`, SSE progress, predicate-row expansion, per-account drift handling. Replaces `setInterval` simulator.
4. **Phase D (risk + telemetry)** — Risk acknowledgement UI, High-risk gating, account-grouped review tiles, plan history view, telemetry.
5. **Phase E (Undo, deferred)** — Apply `reverse_op` envelopes for the "Undo last cleanup" flow.
6. **Phase F (cross-device sync, V2)** — Plan replication via CRDT atop ADR-015.

## Alternatives considered

- **Server-side ephemeral dry-run flag on existing endpoints** (e.g., `?dryRun=true` on `/cleanup/execute`). Rejected: doesn't materialize the plan for review, doesn't survive the round-trip to the user, doesn't support large enumerations, conflates two operations.
- **Client-side preview by re-running rules in the browser**. Rejected: duplicates rule engine logic, can't read provider-side cached data, can't be persisted, can't be audited, makes risk classification inconsistent.
- **Per-feature previews only** (current state — extend the unsubscribe preview pattern to clusters and rules). Rejected: fragments the model, leaves the wizard's Execute Cleanup button as a single-shot trust cliff, no cross-feature aggregate view, no resumable apply.
- **Hard plan size cap** (e.g., 50k rows max, force user to scope down). Rejected: penalises the "10k emails to zero" target user; predicate-row lazy expansion solves the underlying scale problem at lower cost.
- **CRDT-replicated plans from V1**. Rejected: plans are short-lived (30 min) and single-author; CRDT's main advantage (concurrent-write merge) is unused; existing CRDT libraries aren't tuned for 100k-row append-only workloads. Deferred to V2 once ADR-015 is mature.

## References

- DDD-008 Email Operations + addendum (Cleanup Planning subdomain)
- DDD-007 Rules + addendum (`RuleExecutionMode::EvaluateOnly`)
- ADR-018 Provider Folder/Label Operations (vocabulary reused)
- ADR-016 Security Middleware, ADR-017 GDPR Compliance (plan storage / expiry)
- ADR-015 Offline Sync (gates Phase F cross-device plan sync)
- `docs/research/cleanup-dry-run-due-diligence.md` — competitive landscape, provider technical limits, resolved open questions
- `docs/plan/cleanup-dry-run-implementation.md` — phased implementation plan
- `frontend/apps/web/src/features/inbox-cleaner/InboxCleaner.tsx` — current single-shot execute path
- `backend/src/email/unsubscribe.rs` — existing per-feature preview pattern
- `docs/user-interface-overview.md:60` — promised UX
- `docs/plan/inception.md:2236` — current wireframe (to be amended with Step 4.5)
