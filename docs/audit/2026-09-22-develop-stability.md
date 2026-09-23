# Develop stability and integrity audit — September 22, 2026

## Verdict

Base revision: `881868f7c57ef98552612a6cf15edde15cdd43aa` (`develop`).

The application has substantial working foundations and a passing baseline unit suite.
It is not yet ready to guarantee private, unattended classification and verified filing
of 150,000 messages in under ten hours. That readiness requires provider-result
verification, authorization and privacy enforcement, full policy parity, and a measured
end-to-end benchmark. Passing the existing tests does not establish those properties.

This audit combines source tracing, local tests, current dependency advisories,
Agentic-QE execution, and separate backend/UI/model workers. Changes are isolated in
worktrees and integrated as small commits. No live mailbox operations or model downloads
were performed by the audit workers.

## Baseline evidence

| Check                                | Observed result                     | What it establishes                                      |
| ------------------------------------ | ----------------------------------- | -------------------------------------------------------- |
| Frontend unit tests                  | 398 passed: web 331, API 58, core 9 | Existing unit assertions pass                            |
| Frontend type checks                 | Five packages passed                | Existing TypeScript checks pass                          |
| Frontend production build            | Passed                              | SPA bundles successfully                                 |
| Backend library tests                | 1,088 passed, four ignored          | Library tests pass; excludes cleanup worker binary tests |
| Frontend production dependency audit | Zero reported vulnerabilities       | Current production lockfile advisory result              |
| Backend dependency audit             | One vulnerability at baseline       | Rustls 0.23.40 affected by RUSTSEC-2026-0285             |
| Agentic-QE web test execution        | 331 passed, zero skipped            | Actual AQE CLI test run, not a generated score           |
| Agentic-QE focused execution         | Seven tests passed, zero skipped    | Actual runner check against selectAll tests              |

The baseline library run does not establish PostgreSQL support: three PostgreSQL-only
tests return early when their dedicated connection setting is absent and are still
reported as passing. The separate disposable-database check must execute those bodies
and verify persisted migration evidence.

The narrow dependency update selects Rustls 0.23.45 and rustls-webpki 0.103.15;
the post-update advisory scan reports zero vulnerabilities. Unmaintained/yanked
dependency warnings remain distinct from vulnerabilities and are not silently waived.
See [the RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0285.html).

## Confirmed correctness findings

### P1: Cleanup can omit operations beyond the first page

`cleanup/orchestrator/account_worker.rs` requests `u32::MAX` operations and ignores
the next cursor. `cleanup/repository/plan_repo.rs` clamps pages to 1,000. The plan
loader has the same issue: a subsequent save can remove operations omitted from the
loaded aggregate. Regression tests must include more than 1,000 rows and an account
whose first operation occurs after page one. The new regression reproduced 1,000
returned rows for a 1,005-row plan before the fix.

The fix follows all cursor pages. It preserves complete-snapshot semantics; it does
not claim to implement bounded-memory streaming for arbitrarily large plans.

### P1: Provider absence can be reported as successful application

The cleanup worker's missing-provider branch returns success. This must fail closed
and must never increment applied counts. Test scaffolding must supply an explicit
successful recording provider instead of depending on the missing-provider behavior.

### P1: Local model selection differs from the request sent

`vectors/generative.rs` stores classification and chat model names separately but the
shared generation helper always transmits the chat model. Single and batch
classification therefore use the wrong configured model, while reported provenance
names the classification model. Loopback HTTP tests should assert the transmitted
model for chat, single classification, batch classification and parse-fallback calls.

### P1: Applied does not establish remote verification

The cleanup worker generally marks a provider success response as applied without
fetching the resulting mailbox state. Precondition replay remains a TODO.
Outlook move/archive paths discard returned message objects while the provider trait
returns only `Result<()>`; replacement message IDs cannot be propagated reliably.

Required next gate: a persisted operation journal with before-state, intended delta,
provider response/new identity, independent read-back, and explicit reconcile state.
An uncertain outcome must not become either verified or blindly retried.

### P1: Predicate expansion does not enforce the selected rule or age policy

`cleanup/orchestrator/expander.rs:58-69` enumerates all messages in the account and
slices the result into pages; it does not evaluate the rule or archive-age predicate.
Those children inherit the selected action. This is a blocker for enabling predicate
cleanup on real mail. The same full-account read is repeated for every expansion page,
which also undermines large-mailbox performance.

Required next gate: each supported predicate must prove its selection against mixed
eligible/ineligible records; unsupported predicate kinds must fail closed. An
older-than-90-days selection must never materialize a recent message. The current
pagination fix repairs aggregate completeness, not predicate eligibility.

### P1: Work ownership and restart recovery need stronger guarantees

The apply entry point checks a caller-loaded plan status before spawning work; it does
not atomically claim the plan in persistent storage. The offline queue selects then
updates rows rather than atomically claiming them. Its separate scheduler contains
placeholder completion logic and is not the production startup worker.

The job-level Finished state can still coexist with failed operation counts, and
malformed persisted operation payloads can be skipped by repository decoding. These
contracts need explicit failure/reconciliation semantics; the missing-provider fix
corrects per-operation results, not the entire job-state contract.

Required next gate: concurrent callers cannot dispatch the same operation; crashing
after a remote write but before persistence must reconcile without duplicate effects.

## Privacy and authorization findings

### P1: Inference consent is not enforced at every execution path

Startup registers configured cloud generation without checking persisted consent.
Ingestion retains a raw generator and bypasses router enable/disable state; revoking
consent through the router does not necessarily affect already-created background
jobs. Provider identity mapping and OpenRouter revocation coverage also need correction.
These are source findings, not a claim that mail was transmitted during this audit.

Required next gate: one request-time policy boundary covers generation, embeddings,
batch classification, chat, failover, restart, and in-flight job policy changes.
Private mode must reject remote endpoints and cloud-backed model tags.

### P1: Endpoint and credential protections are incomplete

The prior PostgreSQL audit records missing endpoint authentication. The shipped
environment configurations bind all interfaces. Token storage falls back to base64
when no encryption key is configured; base64 is not encryption.

Required next gate: authenticated API/MCP access, account authorization, explicit
loopback/private-network deployment policy, and encrypted token custody that fails
closed when the key is unavailable. OAuth provider consent is not authentication to
Emailibrium's own API.

## UI coverage and test integrity

The baseline contains six Playwright spec files, but the web package does not declare
the runner dependency. Several tests assert only that the page/body exists or use
conditional visibility checks that can pass without performing the intended action.
CI runs frontend type checking, formatting, lint and build, but neither Vitest nor
Playwright. The audit adds explicit unit and browser jobs and a reproducible synthetic
API boundary for browser feature tests.

Browser coverage must include all feature areas: onboarding, command center/search,
email reading/compose/reply, cleanup planning/review/progress, cleanup history/detail,
insights, rules, settings/accounts/models and chat. Each test must assert a user-visible
outcome, fail on unexpected API requests, and never depend on a real mailbox.

Fixture browser coverage proves frontend behavior against the declared fixture
contract. It does not prove production backend response compatibility, OAuth,
provider mutation, real model inference, or complete accessibility. Follow it with
seeded real-backend browser tests and separately authorized provider canaries.

## Agentic-QE and code-intelligence integrity

- AQE MCP `fleet_status` timed out after 300 seconds. Local AQE CLI initialization and
  real frontend test execution succeeded. No MCP health success is claimed.
- AQE's mechanical branch scanner classified Rust files as JavaScript and emitted
  JavaScript-specific advice. Those Rust results were rejected, not used as coverage
  evidence. The TypeScript report is advisory; an empty array is not automatically a bug.
- No paid frontier-judge gate or coverage percentage is claimed. Unit-test counts and
  branch scans do not establish assertion quality or live-system safety.
- GitNexus impact analysis reports lower-bound graphs with unresolved calls. The plan
  loader change has HIGH impact; callers and cleanup flows require regression checks.
- Each worker owns a separate branch and file area. Native Codex workers execute the
  bounded tasks; Ruflo records coordination and checkpoints. Registration alone is
  not evidence that work ran.

## Policy parity required before migration

Emailibrium's current category result is one of twelve hard-coded values. The approved
triage contract requires 25 overlapping topics plus organization, attention and urgency.
Import the approved taxonomy/mappings as versioned data, with content-sensitive rules
and explicit collision/platform holds. Do not replace that policy with broad sender
or keyword guesses.

Implement conversation-scoped NeedsReply/NeedsAction/Waiting/ReadLater/Review/Done,
independent urgency, immutable briefing/action snapshots, the received-age-and-unread
90-day policy with unresolved-obligation exceptions, and preservation of unrelated
labels/read state. Sending, deletion and unsubscribe must remain disabled for the
approved filing profile.

## Forward implementation gates

1. **Safety boundary:** fail closed on missing providers, enforce authentication,
   encrypted token custody and per-request private inference policy.
2. **Durable engine:** atomic leases, complete pagination, bounded batches, persistent
   retries, read-back verification, new Outlook IDs and restart reconciliation.
3. **Policy parity:** versioned taxonomy, independent attention/urgency and exact
   snapshot-scoped completion. Convert each rule into positive and negative examples.
4. **Model evaluation:** fixed model/runtime/template/policy artifacts; held-out
   mailbox evaluation with sender/template leakage controlled. See the companion
   private-model evaluation document.
5. **UI integrity:** mandatory component/unit and browser jobs, full feature matrix,
   seeded real-backend tests, accessibility checks and failure-path assertions.
6. **Throughput acceptance:** representative 1,000-message test, sustained 10,000 run,
   then 150,000 in under ten hours including reads, writes, verification and retries.
   Report unresolved review items separately; moving them aside is not full completion.
7. **Cutover:** stop/reconcile the old writer per account before enabling the engine;
   migrate scheduled briefings after action-state parity is demonstrated.

Every production change follows RED (the intended regression fails), GREEN (minimal
fix), then refactor. AQE check-ins accompany the phase evidence. Do not reset baselines
or lower thresholds merely to obtain a passing badge.

## What this audit did not test

No live Gmail/Outlook/IMAP mutations, end-to-end OAuth flow, real LLM/embedding download
or inference, 150,000-message throughput, sustained Apple GPU memory/thermal behavior,
production privacy-egress test, complete accessibility audit, or deployment occurred.
The optional built-in LLM feature was compile-checked, but no model was loaded.
The dedicated PostgreSQL round trips ran on a disposable local PostgreSQL 16.15
instance; this is distinct from a production deployment. GitHub CI has not been run
for these local integrated commits. Final validation is recorded below.

## Integration validation

The integrated code passed the following local checks after applying sibling commits:

| Check                                                   | Result                                                                            |
| ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Backend default library, binary and integration targets | 1,318 passed, seven ignored, zero failed                                          |
| Opt-in property tests                                   | 21 passed across four test binaries                                               |
| Dedicated PostgreSQL round trips                        | Three passed; 26 successful migrations verified; server stopped                   |
| Optional builtin-llm compilation                        | Passed; no runtime/model compatibility claim                                      |
| CI-equivalent Rust Clippy                               | Passed with the repository's existing allowances                                  |
| Frontend units                                          | 400 passed (web 333, API 58, core 9)                                              |
| Frontend type checks                                    | All five packages passed                                                          |
| Browser suite                                           | 26 passed, zero retries, isolated Chromium and synthetic HTTP/SSE                 |
| Browser test types                                      | Passed through test:e2e precheck                                                  |
| Frontend production build                               | Passed                                                                            |
| Agentic-QE post-integration web tests                   | 333 passed, zero skipped                                                          |
| Production dependency audits                            | Zero reported vulnerabilities in both lockfiles; Rust maintenance warnings remain |
| CI workflow validation                                  | actionlint and yamllint passed                                                    |

The UI matrix and its intentionally untested interactions are documented in
[the browser suite README](../../frontend/apps/web/e2e/README.md). In particular,
draft saving/attachment delivery, high-only acknowledgment completion, and saving
from a rule template need targeted follow-up; route coverage must not be read as
proof those unfinished paths work.

Independent peer review caught and corrected three edge cases before integration:
empty-page cursor regression, sender switching during account refresh, and stale
search results intercepting keyboard selection. Test-first fixes also cover complete
pagination, missing-provider failure, terminal replay, configured model selection,
loaded sender accounts and visible send failures.

See [machine-readable verification](2026-09-22-verification.json) and
[model integration findings](2026-09-22-model-integration-findings.md).
