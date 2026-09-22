# Deterministic browser coverage

Run from `frontend/` with the repository's pinned pnpm:

```sh
pnpm install --frozen-lockfile
pnpm --filter @emailibrium/web exec playwright install --with-deps chromium
pnpm --filter @emailibrium/web test:e2e
```

`test:e2e` type-checks the fixtures and tests before running Chromium. CI installs the
pinned browser and retains screenshots, traces, and the HTML report on failure.
There are no conditional passes, retries, fixed sleeps, or empty-test success flags.

## Isolation contract

The suite starts its own Vite server on `127.0.0.1:4173`; it refuses to reuse an
existing server. A test-only middleware rejects every `/api` request with 503
before any production proxy can forward it. `isolation.spec.ts` verifies that
protection using a request which bypasses browser interception.

Each test gets a new browser context and its own `Mailbox`. Browser HTTP requests
are intercepted at the network boundary. Only explicit synthetic API handlers
are accepted; unexpected API paths, external origins, and uncaught browser errors
fail the test. Service workers are blocked. Messages and credentials use
`example.test`; no live account, provider, model, or mail write is used.

The React screens, routing, query cache, state stores, forms, and SSE parsers are
real. The HTTP responses, mailbox mutations, and stream events are fixtures.
These are **browser feature tests with a simulated backend**, not proof of live
provider or full-stack integration.

## Feature matrix

| Area | Browser checks | Exercised behavior |
| --- | ---: | --- |
| Onboarding | 4 | Skip services, choose an archive strategy, provider/back navigation, offline backend, connect synthetic IMAP and continue |
| Command center/search | 4 | Palette result opens a thread, keyboard open/dismiss/navigation, stale-result exclusion, dashboard search modes, empty results and service errors |
| Email | 4 | Read a thread and reply, choose a sender and send, preserve draft with visible send failure, thread-load error |
| Inbox cleaner | 4 | Ingestion completion, subscription selection, build/review before apply, low-risk apply SSE completion, build failure, history/detail read-only view, high-risk acknowledgment gate |
| Insights | 1 | Overview, subscriptions, sender filter/empty state, topics empty state, trends |
| Rules | 3 | Condition/action editing, test/validate/create, template/cancel, rules-load error |
| Settings | 2 | Persist general/appearance choices across reload, update account strategy, model catalog/provider choices, privacy and consent tabs |
| Chat | 2 | Stream answer, clear conversation, service error and readiness to retry |
| App navigation | 1 | Sidebar links reach feature content including cleanup history and chat |
| Fixture isolation | 1 | Unmocked API writes are rejected by the test server |

## Regression evidence

The audit first observed the missing Playwright executable. After installing the
runner and its browser, new browser assertions exposed these production defects:

- The command palette received a matching server result but displayed “No results
  found” because cmdk filtered its opaque email ID. Email results now retain the
  server's ranking; action commands retain their normal text filtering. Previous results are removed while a new query is debouncing or loading so Enter cannot select a stale email.
- The email screen passed an empty account array to the composer, so Send made no
  request. It now loads active accounts and the composer chooses a valid sender
  when those accounts arrive asynchronously. Component tests also verify that account reordering or disconnection never silently changes the sender of a draft.
- Failed send requests left no visible feedback. The composer now shows an alert
  while keeping the unsent recipient, subject, and body for retry.

Each failing browser case was recorded before its corresponding production fix,
then rerun successfully. Existing unit tests remain a separate gate.

## Remaining gaps

This matrix covers each routed feature area, not every interaction. It does not
verify OAuth redirects/real credentials, actual Gmail/Outlook/IMAP transport,
backend authorization/schema parity, real LLM inference/downloads, provider
rate limits or token refresh, attachment upload/download fidelity, draft
persistence, forward/reply-all delivery, every bulk action, every rule suggestion
or execution path, high-risk apply completion/cancellation/reconnect, consent
export/erasure, desktop integration, offline/PWA behavior, cross-browser behavior,
or a full accessibility audit. Those require additional focused tests and, for
provider integrations, dedicated test accounts and a separately authorized run.

Tests deliberately do not claim that clicking an unimplemented UI control proves
its intended side effect. Keep this distinction when adding coverage.
