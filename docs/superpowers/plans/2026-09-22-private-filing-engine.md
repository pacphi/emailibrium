# Private Filing Engine Implementation Plan

**Goal:** Process 150,000 eligible messages across approved accounts in less than ten
hours, including provider verification, with local inference and deterministic policy.

**Architecture:** Persistent Rust workers own discovery, claims, policy, provider writes,
verification and retries. Models produce constrained classification proposals; they do
not choose credentials or execute tools. SQLite remains supported; PostgreSQL must pass
the same operation contracts. The existing UI is the control surface, with MCP as an
optional interface rather than the high-volume execution loop.

**Base:** `03e39c50157aafd9a4f7cff90081072830666b04` on develop.
**Authorization:** User approved implementation, Ruflo/AQE coordination, TDD, isolated
branches and small commits on September 22. Scheduled filing and its progress monitor
were paused; no live mailbox cutover is part of the first implementation gate.

## Wave 1: enforced safety and policy foundation

| Owner | Files | Deliverable and gate |
| --- | --- | --- |
| Privacy | `backend/src/vectors/**`, `api/consent.rs` | Request-time private/consent guard; revoked and restarted jobs cannot reach denied inference endpoints |
| Cleanup | `backend/src/cleanup/**` | Predicates match only eligible messages; unsupported forms fail closed; persistent exclusive claim before dispatch |
| Security | `middleware/**`, `main.rs`, `email/oauth.rs`, connection UI/client and deployment settings | Auth covers REST and MCP; origin checks; encrypted credential custody; usable authenticated browser connection |
| Lead | `backend/src/triage/**`, `lib.rs`, integration and reports | Typed policy admission/plan generation preserving approved topics, account mappings, attention and system state |

Each lane writes a failing regression, records the intended failure, implements the
minimal change, reruns relevant tests and commits independently. GitNexus impact is
checked before existing symbol changes. Shared build-cache use is serialized; source
edits do not overlap. A peer reviews each security/correctness boundary before integration.

### Policy admission acceptance

- Exactly 25 unique topic keys and version-matched approved account mappings.
- Reject wrong message IDs, stale policy versions, unknown topics and unapproved accounts.
- Exclude Sent, Drafts, Outbox, Spam, Trash and chats regardless of model output.
- Derive organization from a separately resolved approved identity; held identities remain
  Review and cannot gain an organization label from a model guess.
- Preserve tracked unresolved obligations, unpaid bills and urgency over a proposed Done.
- Apply received-age/current-unread retention only after the exception checks.
- Plans can add owned classifications, replace known owned attention labels, and remove
  Inbox membership. They cannot send, delete, unsubscribe or change read/unread state.
- Return exact account/message/revision identity and explicit reason codes for verification.

### Integration gate

Run combined library, binary and integration suites; PostgreSQL round trips against a
disposable database; property tests; frontend units/types/build; isolated browser tests;
format/lint and advisory checks. Run actual Agentic-QE CLI check-ins. AQE MCP health and
Rust branch scanning were unreliable in the prior audit; do not turn those into a pass.
Do not use a generated score or configuration marker as a substitute for execution.

### Local and hosted integration configuration

The September 22 follow-up requested explicit local dotenv loading and GitHub
secrets for real integration tests. Add a Node-built-in dotenv entry point, a
committed empty example, and a manual workflow backed by a dedicated
`live-integration` environment. Generated HTTP/database credentials stay ephemeral.
Dedicated provider tests refresh tokens, verify expected account identity, and
read label/category metadata through the actual adapters. They never mutate mail.
Missing credentials and zero selected tests must fail. Local runner contracts run
without secrets on ordinary CI; actual provider execution requires the dedicated
test-account credentials and an explicit invocation.

## Wave 2: verified, resumable execution

1. Introduce per-message operation identities and atomic leases with durable expiry and
   recovery. Freeze discovery windows and persist every cursor before advancing watermarks.
2. Unify provider operations with a typed response carrying current provider ID/revision.
   Preserve Outlook replacement IDs. Add bounded Gmail batching and serial dependency
   ordering within each message; honor provider Retry-After per account.
3. Persist before-state and intended delta before writing. Verify actual provider state
   afterward; uncertain results enter reconciliation. Never count a response as verified.
4. Simulate crash before/after each boundary, partial batches, concurrent apply, user edits,
   token refresh and invalid cursors. Every unique message remains accounted for.

## Wave 3: private model and policy integration

Import the existing approved taxonomy/account mappings from a local policy file; never
commit personal mailbox metadata, credentials or source-message IDs. Replace the single
category assumption with multiple topics plus organization, attention and urgency.
Add schema-constrained local classification with exact IDs, bounded tokens, finite timeouts,
model/template/artifact pins and cached results. Benchmark current Qwen3.8 candidates and
compact speed controls; no model is selected solely because it is newer.

## Wave 4: obligation state and UI completion

Implement conversation-scoped actions and matching completion evidence; opening mail
never resolves obligations. Persist immutable briefing snapshots and preserve later
arrivals during bulk Done. Complete UI gaps documented by the audit (drafts, attachments,
high-risk acknowledgment lifecycle and template save) with browser regressions and
seeded real-backend contract tests. Include meaningful accessibility checks.

## Wave 5: measured acceptance and cutover

Use representative 1,000-message and sustained 10,000-message pilots before the 150,000
run. Report end-to-end verified messages/sec, per-account coverage, exceptions, retries,
memory/thermal behavior, model accuracy and missed obligations. More than 4.17 messages/sec
sustained is necessary for ten hours; plan headroom rather than promising a multiplier.
Unresolved Review items are reported separately and do not count as complete classification.
Stop and reconcile every prior writer before an account is transferred. Enable native
incremental maintenance and local briefing schedules only after their completion gates.
