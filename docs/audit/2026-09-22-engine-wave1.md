# Private filing engine: first implementation wave

This wave establishes enforcement boundaries for the deterministic filing engine.
It does not yet connect the new policy planner to a durable, verified mailbox
worker. The 150,000-message / ten-hour objective remains unmeasured.

The [verification record](2026-09-22-engine-verification.json) pins the validated
code revision, lockfile hashes, executed checks, and untested boundaries.

Scheduled filing and its recovery monitor were paused at the user's request.
The initial audit and daily briefing schedules were already paused. No provider
write or live account cutover was performed during this implementation.

## Implemented behavior

- **Local access:** HTTP startup requires a valid local credential before the
  database, models, or mailbox workers start. Both REST and MCP enforce the same
  authentication and Origin checks. The browser exchanges the credential for a
  bounded, memory-only HttpOnly session; lock/restart revoke access. Stale requests
  cannot reopen a locked session. Request logs omit query strings, including
  encoded OAuth parameters. OAuth callback state is issued, bounded, expiring,
  and consumed once.
- **Credential custody:** OAuth and IMAP credential reads/writes require encryption.
  The former base64 fallback is removed. Undecryptable legacy accounts require
  reconnection; tests did not migrate or replace real credentials.
- **Inference privacy:** Cloud inference is disabled by default. Shared request-time
  enforcement checks explicit mode and persisted provider consent across ingestion,
  generation, routing, tool calls, and embeddings. Revocation waits for already
  admitted work. Local Ollama admission requires literal loopback, runtime evidence
  that cloud is disabled, and an installed model digest without remote-host/model
  metadata. Unsupported or unverifiable runtime responses fail closed. This trusts
  the local daemon; it is not protection against a malicious host. Gemini credentials
  use a header and are excluded from transport error URLs.
- **Cleanup integrity:** Predicates apply actual age/rule eligibility beyond the
  preview sample. Reviewed rules are bound to their original fingerprint. Protected
  sources and malformed source metadata are excluded conservatively. Persistent
  plan claims prevent concurrent apply and protect refresh, cancellation, save,
  and purge across server instances. Uncertain transactions retain ownership for
  reconciliation; persisted failures cannot be reported as successful completion.
- **Policy planning:** A typed, pure planner validates the 25-topic taxonomy,
  versioned account approval, exact account/message/revision identity, and topic
  membership. It preserves user labels and unresolved obligations, handles bills,
  urgency and unread retention in deterministic order, and excludes system mail
  locations. Plans cannot send, delete, unsubscribe, or change read state.
- **Integration configuration:** Explicit dotenv loading, isolated local/database
  modes, and manual provider credential tests use a documented test-only environment.
  See [local and GitHub setup](../testing/integration-environments.md).

## Evidence and review

Each implementation lane recorded failing regression tests before its fix. Peer
review found and corrected account/revision proposal binding, stale browser session
responses, encoded OAuth log leakage, and cleanup mutations outside the apply claim.
Ruflo held the coordination contract and checkpoints; native Codex workers made
bounded changes on separate branches. Actual Agentic-QE CLI runs executed the
frontend suites. AQE MCP health timeouts and its Rust-as-JavaScript scan were not
accepted as quality evidence.

The first combined application run after the cleanup boundary fix passed 1,389
default Rust tests, with eight explicitly ignored cases. Some PostgreSQL test bodies
return early without their URL, so that number alone does not prove PostgreSQL
support. The plan/job/audit bodies were separately executed against disposable
PostgreSQL. The integration runner additionally checks that each specifically
selected test actually executes; an earlier library-target invocation of the
binary-only cutoff test ran zero tests and is not evidence for that boundary.
The corrected exact binary invocation then executed and passed the cutoff test
against PostgreSQL; it was not skipped.

After adding the integration configuration, the documented local command passed
1,398 Rust tests and all ten assembled-backend checks. Its 12 ignored cases include
two live credential tests, three live IMAP tests, three ONNX-download tests, the
50,000-operation performance case, the separately invoked PostgreSQL cutoff test,
and two doctest examples.
The runner's 18 contracts passed with zero skips. A separate dotenv-driven
PostgreSQL run passed the library, binary audit, and exact cutoff selections.

Other recorded checks include 21 opt-in property tests, 402 frontend tests,
32 Chromium browser scenarios, production frontend compilation, type/lint checks,
Rust Clippy, and optional built-in-model compilation. The native backend probe
exercises actual REST routes, authenticated MCP initialization/tool discovery,
browser session creation/revocation, callback rejection, and log canaries against
an empty temporary SQLite database. Its report binds to the tested binary hash and
invalidates a prior success before a new run.

The full frontend dependency audit initially reported five advisory entries in
development tools. Vitest, Browserslist, and baseline-browser-mapping were updated
to patched versions; the subsequent full scan reported zero vulnerabilities.
The patch versions follow the [Vitest advisory](https://github.com/advisories/GHSA-82fw-gwwq-j7x9),
[Browserslist advisories](https://github.com/advisories/GHSA-c83g-rgw3-j3cx), and
[baseline mapping advisory](https://github.com/advisories/GHSA-w5vr-8v7q-w6rv).
The Rust lockfile scan reported zero vulnerabilities and a `ttf-parser`
maintenance warning. The vendored RuVector build also emits an unknown-lint warning.
Neither warning is silently converted into a clean maintenance bill.

## Remaining work before filing can resume

1. Build per-message durable operations, leases, provider revision checks, and
   reconciliation after crashes or uncertain writes. Preserve Outlook replacement
   IDs, freeze discovery windows, and persist watermarks safely. The current
   plan-level claim is not a complete recovery engine.
2. Wire actual approved account mappings and observed provider/obligation facts into
   the policy planner. Add constrained local classification and pinned model/template
   artifacts. The planner cannot establish the truth of caller-supplied facts.
3. Run dedicated provider accounts through read-only contracts, then isolated fixture
   mailbox mutation/verification tests. Real OAuth, token refresh, live mailbox
   writes, full privacy egress monitoring, and account recovery were not exercised
   with real credentials during this wave.
4. Fix and test the remaining UI behavior listed below. The browser suite reaches
   every routed feature area but is not complete functional or accessibility coverage.
5. Measure representative 1,000- and 10,000-message pilots before 150,000 messages.
   Count verified messages, exceptions, retries, and missed obligations separately.
   No throughput estimate follows from unit-test counts or an active schedule.

## UI coverage limits

| Area | Remaining evidence or implementation |
| --- | --- |
| Email | Draft persistence, attachments, forward/reply-all, bulk actions, HTML/remote-content handling |
| Cleanup | High-risk acknowledgment lifecycle, cancellation/reconnect, samples/pagination, actual audit outcomes |
| Rules | Template save, edit/delete/run, AI suggestion acceptance, metrics |
| Settings/privacy | Consent lifecycle, export/erasure, actual model load and denied-inference recovery |
| Chat | Tool approval/rejection, interruption, conversation persistence, incremental delivery |
| Accounts/search/insights | Live transport, token refresh, sync/reindex, search relevance and analytics |
| Platform | Actual proxy SSE timing, offline/PWA, responsive/cross-browser checks, accessibility |

Source review still finds a draft button without persistence, attachments omitted
from the send payload, templates with empty IDs routed through the update path,
and a high-risk group acknowledgment requirement without matching controls.
Nginx only explicitly disables buffering for ingestion; chat/cleanup streaming
through the proxy remains unverified. The new Compose session checks were linted,
but the complete Docker workflow was not executed locally in this wave.
