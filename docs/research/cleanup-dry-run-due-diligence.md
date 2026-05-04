# Cleanup Dry-Run — Product & Technical Due Diligence

| Field   | Value                                                                                                                                                                                |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Status  | Proposed for review                                                                                                                                                                  |
| Date    | 2026-05-04                                                                                                                                                                           |
| Informs | ADR-030 (Cleanup Dry-Run / Plan-Apply), DDD-008 Addendum (Cleanup Planning)                                                                                                          |
| Purpose | Validate the Plan/Apply design against competitor UX and provider constraints, and resolve the three open questions: plan size cap, per-account etag policy, cross-device plan sync. |

This document is the homework behind ADR-030. Read it as the "why" for the design choices already drafted, and as the input for the three deferred decisions. Sources are listed at the bottom.

---

## 1. Competitive landscape — what existing cleanup tools do

| Tool                           | Preview-before-commit                                                          | Undo                                                  | Cleanup model                                   | Notable gap (vs. our design)                                                                           |
| ------------------------------ | ------------------------------------------------------------------------------ | ----------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| **Mailstrom**                  | Yes — shows exactly what's in each pattern-grouped bundle before action        | Yes — explicit "comprehensive undo system" per action | Manual control: surfaces patterns, user decides | Per-bundle preview, not a unified cross-feature plan. No persisted plan artifact.                      |
| **Clean Email**                | Partial — group-level review                                                   | Yes (action history)                                  | Bulk operations + AI suggestions                | No materialized plan; preview is a list view, not a diff.                                              |
| **SaneBox**                    | No — operates server-side via folders, AI decides                              | Limited (move things back manually)                   | AI-driven server-side filing                    | Opposite philosophy: "trust the AI." We're closer to Mailstrom's "show me, then I decide."             |
| **Unroll.me**                  | Roll-up digest preview only                                                    | Limited                                               | Subscription rollup                             | Subscription-only; no cross-feature plan.                                                              |
| **mailtrim** (OSS, Gmail-only) | Yes — explicit `--dry-run` CLI flag                                            | N/A (CLI)                                             | Pattern-driven Gmail bulk delete                | Validates that "dry-run" is the right primitive even for ops-grade tools; ours adds UI + cross-source. |
| **Superhuman**                 | N/A (no bulk cleaner; per-message UX with rate-limit handling visible to user) | Toast-undo per action                                 | Inbox triage, not cleanup                       | Confirms toast-undo is the consumer-grade gold standard.                                               |
| **Gmail native**               | "Undo" toast within ~5–30s window only                                         | Toast (very short)                                    | Per-action                                      | Sets the floor: users expect at minimum a short undo toast.                                            |

**Net read.** The market splits into "AI auto-files for you" (SaneBox) and "show me everything then I act in bulk" (Mailstrom / Clean Email / mailtrim). **None of them present a single, unified, cross-feature plan that spans subscriptions + clusters + rules + archive strategy in one reviewable artifact** before commit. That gap is exactly what ADR-030's `CleanupPlan` aggregate fills, and is the strongest differentiator we have.

What we should adopt verbatim from the field:

1. **Per-row review with samples** (Mailstrom). Concretely: for any "archive 2,400 emails matching sender X" row, expose 3–5 representative messages on hover/expand.
2. **Trash-as-undo** as the default for archive/delete (everyone — Gmail, Outlook, Microsoft 365). Most providers retain Trash 14–30 days; that is our baseline safety net even before our own `reverse_op`.
3. **Toast-undo for the small-stakes actions** (Gmail / Superhuman). For Low-risk plan rows that complete fast, surface a short-lived "Undo" toast in addition to the post-apply page.
4. **Multi-step confirmation for destructive bulk** (NN/G, UX Movement, AWS guardrail patterns). Keep destructive and confirm buttons visually + spatially separated; require typing or explicit per-row acknowledgement on High-risk rows. Do **not** rely on dialog confirmation alone.

---

## 2. Provider technical constraints

### 2.1 Gmail

- **Change-tracking primitive**: `historyId` returned on every message; `users.history.list` provides incremental sync from a starting `historyId`. Validity is **"at least a week"** typically, "in rare cases only a few hours". Out-of-date or invalid `historyId` returns **HTTP 404** → app must fall back to full sync. _This is critical input to the etag policy decision below._
- **Batch operations**: `users.messages.batchModify` and `batchDelete`. JSON batch wrapper allows 100 sub-requests per batch but **each sub-request counts as 1 quota unit**. `batchDelete` is **50 units per call**, capping bulk delete at ~300 emails/min before throttling.
- **Per-user quota** (default project): 250 quota units / user / second; 1,000,000,000 / day. Translates to roughly 25 modifies/sec sustained.
- **429 handling**: truncated exponential backoff is the official guidance.

**Implication for our design.** Gmail's `historyId` is exactly the `account_state_etag` we need. Sustained throughput cap means Apply must be a long-running streaming job (already in ADR-030 §7). Plan-Apply drift detection should treat a 404 from `history.list` as "plan invalidated; rebuild required".

### 2.2 Microsoft Graph (Outlook / Microsoft 365)

- **Change-tracking primitive**: `delta` query on `/messages` and `/mailFolders` returns `@odata.deltaLink` containing an opaque `$deltatoken`. **Token validity is bounded by the size of an internal cache**, not a fixed TTL — older tokens evict as new ones are added. Practical guidance: tokens "may expire," handle invalid-token errors by re-doing a full sync.
- **Per-item ETag**: each message in the response carries `@odata.etag`, distinct from the delta token. Useful for optimistic concurrency on individual mutations.
- **Batch operations**: JSON batching capped at **20 sub-requests per batch**. For mailbox-targeted requests, Graph dispatches **only 4 concurrent** sub-requests to Exchange Online regardless of batch size (MailboxConcurrency limit). `dependsOn` enables sequential execution within a batch.
- **Throttling**: `MailboxConcurrency` 429 errors are common; per-mailbox concurrency is the dominant constraint, not per-app rate.

**Implication.** Outlook is a tighter funnel than Gmail at apply time (4 concurrent ops/mailbox). Our ApplyOrchestrator must:

- Schedule per-account (not per-app) workers with concurrency = 4.
- Use `dependsOn` to keep a batch ordered when needed.
- Treat delta-token invalid as plan-invalidating for that account, same as Gmail's 404.

### 2.3 IMAP

- **Change-tracking primitive**: RFC 4551 / RFC 7162 — **CONDSTORE / QRESYNC** with `MODSEQ`/`HIGHESTMODSEQ`. Server returns the highest mod-seq on `SELECT`; client compares against cached value. `UIDVALIDITY` change forces a full resync (mailbox renumbered).
- **No batch primitive**; UID SEARCH + UID STORE / UID MOVE per-message, but pipelining within a connection is allowed and well-supported in Rust IMAP clients.
- **Server-side rate limits vary** by host; generally generous on dedicated/IMAP-first servers, much tighter on shared (e.g., FastMail caps differ from yahoo.com).

**Implication.** `(UIDVALIDITY, HIGHESTMODSEQ)` together act as the IMAP `account_state_etag`. UIDVALIDITY change forces plan invalidation (entire mailbox renumbering). HIGHESTMODSEQ change forces row-level revalidation but not necessarily plan rebuild.

### 2.4 POP3

- No server-side state, no folders, no change tracking. Already noted in ADR-018; for cleanup planning, **POP3 deletes are always High risk** because they are irrecoverable by the protocol; no concept of Trash exists at the protocol level.

### 2.5 Cross-provider summary

| Provider | Etag primitive                 | Invalidation signal            | Batch cap      | Per-mailbox concurrency | Apply throughput (sustained)       |
| -------- | ------------------------------ | ------------------------------ | -------------- | ----------------------- | ---------------------------------- |
| Gmail    | `historyId`                    | 404 on `history.list`          | 100 / batch    | Quota-bound (~25/s)     | ~25 modifies/sec, ~300 deletes/min |
| Outlook  | `$deltatoken` + per-item etag  | invalid-token 410              | 20 / batch     | 4 concurrent            | ~4–8 ops/sec/mailbox practical     |
| IMAP     | `(UIDVALIDITY, HIGHESTMODSEQ)` | UIDVALIDITY change → full sync | n/a (pipeline) | Server-dependent        | Highly variable                    |
| POP3     | none                           | n/a                            | n/a            | 1                       | 1 op/sec (most servers)            |

---

## 3. Best-practice product design — deeper read

### 3.1 Destructive-action UX patterns

Synthesizing NN/G, UX Movement, PatternFly, and the AWS guardrails post:

1. **Visual + spatial separation** of confirm vs. destructive (Fitts's Law applied in reverse — make destructive _slightly_ harder, not impossible). Avoid "OK/Cancel" pairs that decay to muscle memory.
2. **Use a danger-color destructive button** (red), never the default primary color. Reserve "primary" for the safe path.
3. **Multi-stage confirmation only when warranted.** Modal-on-modal fatigue degrades trust. Reserve typed-confirmation ("type DELETE") for irrecoverable bulk operations (e.g., POP3 delete; archive of >5,000 across multiple accounts).
4. **Show the consequence, not the action.** "Archive 2,400 emails — they will move to Archive across 2 accounts" beats "Apply rule X."
5. **Undo > confirm where possible.** A short undo window plus easy restore from Trash beats a confirmation-modal-heavy flow. We use both because the wizard's blast radius is too large for undo-only.
6. **Reversibility transparency.** Tell the user upfront which rows are reversible, which are not, and how long the reversal window is (Gmail Trash 30 days; Outlook Deletions 14 days default, 30 max).

### 3.2 Local-first considerations

The user's data already lives locally (per Emailibrium's architecture). The plan artifact should follow the same principle: **the canonical plan is local; the cloud is a sync relay** when used at all. This both:

- Reduces server-side liability (no plan-content lives on Emailibrium's servers in cloud mode).
- Makes Plan fast and offline-capable (no round-trip to a planner service).
- Forces the cross-device sync question to be answered by either CRDT replication or "no sync, build per-device."

### 3.3 Trust-building affordances we should add

Beyond ADR-030's current scope, the following micro-affordances measurably increase user trust in destructive flows (drawn from NN/G & UX Psychology):

- **Per-row "What will happen?"** explainer. One-sentence translation of the planned operation in plain English, sourced from `PlanSource` + `target`.
- **"Show me 5 examples"** drill-down inline. Already in the addendum; emphasize this is **non-skippable** for plans > N rows.
- **Account-scoped summary tiles.** "Gmail: 2,400 archive, 12 unsubscribe. Outlook: 480 move-to-Archive." Account-bordered visual grouping reduces cross-account anxiety.
- **Risk legend** persistent on the Review screen.
- **"Apply only Low risk now, leave Medium/High for later"** affordance. Splits the plan and lets users build trust progressively.

This last point — partial-apply by risk — is genuinely novel in this market and worth elevating to a first-class feature.

---

## 4. Resolved recommendations for the three open questions

### 4.1 Plan size cap

**Recommendation: hybrid materialization, no hard cap, soft warnings at 100 k rows; predicate-operation lazy expansion for rule sources only.**

Reasoning:

- A user with a 250 k inbox who selects "archive everything older than 1 year" produces a plan of ~150 k rows. Materializing all rows at build time is feasible (SQLite handles this cheaply — < 2 s on local hardware) but risks UX problems on the Review screen.
- **Per-message materialization** for explicit user actions (subscription unsubscribe, cluster archive, manual selections). These are typically O(thousands), bounded by the user's clicks.
- **Predicate operations** for rule and archive-strategy sources. Store the predicate (`ruleId` / `strategy`) and the projected count; expand to per-message rows lazily during Apply, in pages of 1,000.
- **Soft warnings**: if any group exceeds 10 k rows, show a banner "Large operation — review the sample carefully." If total plan > 100 k rows, require explicit acknowledgement before Apply.
- **No hard cap**. Hard caps frustrate the user we most want to help (the "10,000 emails to zero" target user from the spec). Instead:
  - Apply is throttled by provider rate limits anyway — large plans just take longer, with progress visible.
  - GDPR purge runs on `valid_until + 7d`, so a 1 M-row plan that's never applied is auto-cleaned.

This adds two enums to the addendum:

```rust
pub enum PlannedOperation {
    Materialized(PlannedOperationRow),       // per-message
    Predicate(PlannedOperationPredicate),    // predicate-based, expanded at apply
}

pub struct PlannedOperationPredicate {
    seq: u64,
    account_id: AccountId,
    predicate: PlanPredicate,                // RuleId | ArchiveStrategy | LabelFilter
    projected_count: u64,                    // for UI summary
    sample_email_ids: Vec<EmailId>,          // 5–20 representatives, materialized at build
    target: Option<FolderOrLabel>,
    risk: RiskLevel,
}
```

### 4.2 Per-account etag policy

**Recommendation: per-account invalidation with row-level revalidation when possible. Full plan invalidation only on hard signals.**

Concretely:

| Provider | Soft drift (re-validate plan rows for that account)               | Hard drift (rebuild that account's portion of the plan) |
| -------- | ----------------------------------------------------------------- | ------------------------------------------------------- |
| Gmail    | `historyId` advanced but `history.list` returns valid changes     | `history.list` returns 404                              |
| Outlook  | `$deltatoken` advanced and returns deltas                         | Delta-token invalid (410)                               |
| IMAP     | `HIGHESTMODSEQ` advanced (CONDSTORE returns CHANGEDSINCE results) | `UIDVALIDITY` changed                                   |
| POP3     | n/a (single-shot)                                                 | Any prior delete                                        |

Rules of engagement:

1. **Account-scoped, not plan-scoped.** Drift on Gmail account A does not invalidate Outlook account B's rows. The plan stays valid in part.
2. **Soft-drift recovery is automatic.** ApplyOrchestrator re-evaluates each pending row against current state on dispatch — if the email's labels/folder no longer match the row's preconditions, mark `skipped` with reason `state_drift`. Surface aggregate count in progress UI.
3. **Hard-drift requires user acknowledgement.** Pause the apply, show "Account X changed substantially since you reviewed this plan. Refresh and review again?" with a "Refresh just this account" affordance.
4. **`account_state_etag` is a tuple, stored per-account in the plan envelope.** `{account_id, etag_kind, etag_value}` — kind is `historyId | deltaToken | (uidvalidity, modseq) | none`.
5. **Revalidation cost is low** — already-paid-for in our local sync. We re-read the local repository, not the provider, on row dispatch.

This keeps the plan useful in the realistic case where mail trickles in during the user's review period, without the brittleness of "any change anywhere kills the plan."

### 4.3 Cross-device plan sync

**Recommendation: defer — Phase E or later. In V1, plans are device-local. Provide a clear, non-magic "transfer plan" workflow if needed.**

Why not CRDT-sync from day one:

1. **Plans are short-lived (default 30 min).** The window of utility for cross-device sync is small.
2. **Plans are large** (potentially 100 k+ rows). Existing CRDT replication libraries (Yjs, Automerge) are tuned for collaborative documents, not 100 k-row append-only datasets — performance and storage tradeoffs would be non-trivial.
3. **Plans are single-author** by design. There's no concurrent-write merge conflict to solve, so CRDT's main advantage is wasted; what we'd actually want is "ship the plan from device A to device B."
4. **The wizard itself is short** (~10 minutes per the design intent). Cross-device handoff is a low-frequency need.
5. **ADR-015 (offline-sync)** is still maturing. Building cross-device plan sync on top of an unsettled foundation creates churn.

What to do instead in V1:

- **Plans stay local.** Builds, reviews, and applies all happen on the same device.
- **Apply progress is observable cross-device** because it writes to the synced email state (DDD-008's `EmailRepository`) — device B sees changes appear in real time, just not the plan artifact.
- **Manual plan export** for power users: "Export this plan as JSON" / "Import on another device." Low engineering cost, covers the rare case.

What to do in V2 (Phase E or later):

- Once ADR-015 sync layer is solid, introduce **plan replication via CRDT for ready/applying plans only** (not draft) — a `Y.Doc` per plan, with the operations stored as a `Y.Array` keyed by `seq` plus the envelope as a `Y.Map`.
- Status updates (pending → applied) are CRDT writes, naturally idempotent and concurrent-safe.
- **Don't** sync `cleanup_plan_operations.sample_email_ids` payloads cross-device — re-fetch locally from the synced email store. This keeps the synced doc small (envelope + per-row status + predicates ≪ raw row payload).
- Loro is worth evaluating alongside Yjs for V2 — it claims better large-list performance with faster operations on append-heavy workloads, which fits this use case.

This keeps V1 simple and shippable, and leaves the door open to add cross-device sync when (a) ADR-015 is stable and (b) usage data shows it's a real need.

---

## 5. Updated risk register

| Risk                                                         | Likelihood | Impact | Mitigation (now in plan)                                                    |
| ------------------------------------------------------------ | ---------- | ------ | --------------------------------------------------------------------------- |
| User reviews plan, mailbox drifts, applies anyway            | High       | Med    | Per-account etag check on Apply; soft-drift skip; hard-drift pause          |
| User builds 1M-row plan and freezes the UI                   | Med        | Med    | Predicate-operation lazy materialization; > 100 k acknowledgement gate      |
| Apply hits provider 429s and looks stuck                     | High       | Low    | Per-account concurrency caps (Outlook=4); SSE progress shows backoff state  |
| User cancels mid-apply, half operations done                 | High       | Low    | Per-row status; clear "X applied, Y skipped" report; resumable              |
| User wants to apply only the safe parts                      | Med        | Low    | "Apply Low risk only" affordance (new feature in §3.3 above)                |
| User loses trust because preview UI feels generic            | Med        | High   | Per-row plain-English explainer + sample-emails drill-down; account-grouped |
| POP3 user permanently deletes emails irrecoverably           | Low        | High   | Mark all POP3 deletes as High risk; require typed confirmation              |
| Plans accumulate on disk and bloat SQLite                    | Low        | Low    | `valid_until + 7d` GC; user-visible plans list with delete                  |
| Cross-device user-mental-model break (plan only on device A) | Low        | Low    | Document clearly; opt-in JSON export/import for V1                          |

---

## 6. Status — folded into ADR-030 and DDD-008 addendum

The seven deltas originally proposed in this section have all been incorporated into the canonical specs as of 2026-05-04:

| Resolution                                          | Now lives in                                            |
| --------------------------------------------------- | ------------------------------------------------------- |
| Materialized + Predicate operation kinds            | DDD-008 addendum §Aggregates §3; ADR-030 §3             |
| RiskClassifier (POP3 + bulk thresholds)             | DDD-008 addendum §Domain services; ADR-030 §6           |
| Risk-scoped partial apply as first-class            | ADR-030 §7                                              |
| `?risk_max=low\|medium\|high` query param           | ADR-030 §9 / DDD-008 addendum §Commands                 |
| Cross-device sync deferred to Phase F (V2)          | ADR-030 §12 / Implementation plan Phase F               |
| Account-scoped drift invariant                      | DDD-008 addendum §Aggregates §3 §Invariants; ADR-030 §8 |
| All three originally-deferred open questions closed | This document §4; canonical specs                       |

This document remains the audit trail / due-diligence reference. For current design, read ADR-030 and the DDD-008 addendum; for delivery, read `docs/plan/cleanup-dry-run-implementation.md`.

---

## 7. Summary for review

- **Differentiator confirmed.** No competitor offers a unified, cross-source, materialized, partially-appliable plan. ADR-030 fills a real market gap.
- **Provider technicals all line up** with ADR-030's `account_state_etag` abstraction. Gmail's `historyId`, Outlook's `$deltatoken`, IMAP's `(UIDVALIDITY, HIGHESTMODSEQ)` map cleanly. POP3 is the outlier (always High risk).
- **Plan size**: hybrid materialized/predicate, soft 100 k threshold, no hard cap.
- **Etag policy**: per-account; soft drift recovers, hard drift pauses.
- **Cross-device sync**: deferred to Phase E behind ADR-015; export/import for V1.
- **Net adds to the design**: predicate operations, partial-apply by risk, account-grouped review tiles, and explicit POP3 risk handling.

---

## Sources

- [Best Email Cleanup Tools 2026 (Mailstrom)](https://mailstrom.co/articles/best-email-cleanup-tools-2026/)
- [9 Best Email Cleaner Apps 2026 (Clean Email blog)](https://clean.email/blog/email-management/best-email-cleaner-app)
- [SaneBox: What makes us different](https://www.sanebox.com/help/90-what-makes-sanebox-different-from-other-services)
- [Mailstrom vs SaneBox 2026](https://mailstrom.co/articles/mailstrom-vs-sanebox/)
- [mailtrim — open-source Gmail cleanup CLI with --dry-run](https://github.com/sadhgurutech/mailtrim)
- [Gmail API: Synchronize clients (history-based)](https://developers.google.com/workspace/gmail/api/guides/sync)
- [Gmail API: users.history.list](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.history/list)
- [Gmail API: Usage limits / quota](https://developers.google.com/workspace/gmail/api/reference/quota)
- [Gmail API: Batch requests guide](https://developers.google.com/workspace/gmail/api/guides/batch)
- [Gmail API: users.messages.batchModify](https://developers.google.com/gmail/api/reference/rest/v1/users.messages/batchModify)
- [Gmail API rate limits 2026 reference (Prospeo)](https://prospeo.io/s/gmail-rate-limits)
- [Microsoft Graph: Delta query overview](https://learn.microsoft.com/en-us/graph/delta-query-overview)
- [Microsoft Graph: message: delta](https://learn.microsoft.com/en-us/graph/api/message-delta?view=graph-rest-1.0)
- [Microsoft Graph: JSON batching](https://learn.microsoft.com/en-us/graph/json-batching)
- [Microsoft Graph: Service-specific throttling limits](https://learn.microsoft.com/en-us/graph/throttling-limits)
- [Microsoft Graph: Throttling guidance](https://learn.microsoft.com/en-us/graph/throttling)
- [MailboxConcurrency limit & batching (Glen's Exchange Dev)](https://gsexdev.blogspot.com/2020/09/the-mailboxconcurrency-limit-and-using.html)
- [RFC 7162 — IMAP CONDSTORE / QRESYNC](https://datatracker.ietf.org/doc/html/rfc7162)
- [RFC 4551 — IMAP Conditional STORE](https://datatracker.ietf.org/doc/html/rfc4551)
- [Microsoft 365 Exchange data deletion (retention)](https://learn.microsoft.com/en-us/compliance/assurance/assurance-exchange-online-data-deletion)
- [NN/g — Dangerous UX: Consequential Options Close to Benign Options](https://www.nngroup.com/articles/proximity-consequential-options/)
- [UX Psychology — How to design better destructive action modals](https://uxpsychology.substack.com/p/how-to-design-better-destructive)
- [UX Movement — How to design destructive actions that prevent data loss](https://uxmovement.com/buttons/how-to-design-destructive-actions-that-prevent-data-loss/)
- [Bulk action UX: 8 design guidelines (Eleken)](https://www.eleken.co/blog-posts/bulk-actions-ux)
- [PatternFly — Modal design guidelines](https://www.patternfly.org/components/modal/design-guidelines/)
- [AWS CLI Action-Level Guardrails (hoop.dev)](https://hoop.dev/blog/aws-cli-action-level-guardrails-prevent-costly-mistakes-before-they-happen/)
- [Yjs documentation](https://docs.yjs.dev/)
- [Going local-first with Automerge and Convex](https://stack.convex.dev/automerge-and-convex)
- [Best CRDT Libraries 2025 (Velt)](https://velt.dev/blog/best-crdt-libraries-real-time-data-sync)
- [RxDB — Why Local-First Software Is the Future](https://rxdb.info/articles/local-first-future.html)
- [Offline-first sync patterns (Developer's Voice)](https://developersvoice.com/blog/mobile/offline-first-sync-patterns/)
