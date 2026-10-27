# Postgres/autopilot delta — follow-up plan (27 Oct 2026)

Written as part of retiring `.autopilot/` before the mail-agent chief-of-staff work starts. Scope: what was planned for Postgres/SeaORM vs. what the autopilot tool itself recorded vs. what the code actually does today, plus what's left over in `.autopilot/`'s backlog that outlives the folder.

## 1. Postgres/SeaORM: done, not pending

The mail-agent plan (`mail-agent-2026-10.md`, section 10) described three things as still needing work. All three are already finished on `develop`, confirmed against both the ADRs and the code directly (that plan has been corrected in place):

| What was thought pending | Actual status |
| --- | --- |
| `Database::Postgres` as a real backend option | Done — `backend/src/db/mod.rs:106`, URL-dispatched `enum Database { Sqlite, Postgres }` |
| SeaORM dialect layer | Done — 84 files reference `sea_orm`/`SeaOrm`; ADR-035 (hand-rolled translator) was superseded same-day by ADR-036 (SeaORM) before the hand-rolled version ever shipped |
| Three PG-only tests returning early in CI instead of running for real | Done — `postgres_full_trait_round_trip`, `postgres_job_lifecycle_round_trip`, `postgres_audit_round_trip` run against a live `postgres:16-alpine` CI service (`.github/workflows/ci.yml:216`), not skipped |

All of it passed autopilot's own adversarial review (court verdict **SHIP**, two overturn-and-remediate rounds along the way — 39 charges filed, 26 fixed, 7 parked as pre-existing, 0 left unaddressed). It's on `develop`, not yet on `main` — same "merge develop into main" step the mail-agent plan already calls for before anything else.

## 2. The one real gap: full-text search on Postgres

ADR-034 accepted the *design* for BM25 full-text search via `ruvector-postgres`, but adoption was deliberately split out as its own phase and never started — confirmed via `.autopilot/queued/postgres-fulltext-search.pipeline.yml`, parked since 2026-08-06. A Postgres-backed deployment today has no keyword search (the three SQLite FTS5 migrations are intentionally absent from `migrations/postgres/`).

This doesn't block the chief-of-staff plan — nothing in it depends on Postgres full-text search — but it's a known, honest gap if anyone runs emailibrium on Postgres instead of SQLite.

## 3. What's left in `.autopilot/`'s backlog

Four projects in `.autopilot/` are finished (postgres-support above, plus `ci-build-optimization`, `docs-accuracy-audit`, `keyboard-shortcuts` — each closed with only minor open backlog notes, nothing blocking).

Six more were planned but **never started** — pure backlog, not history:

| Pipeline | Note |
| --- | --- |
| `security-baseline` | Its own file says it "should run first — ahead of correctness-debt and everything else." Nobody acted on that. |
| `mcp-identity-authz` | Overlaps directly with this plan's authority-gated MCP write surface (§4 of the mail-agent plan) — worth reading before building that surface, not after. |
| `mcp-write-parity` | Same overlap — about what write actions an MCP tool is and isn't allowed to take. |
| `correctness-debt` | Not reviewed in depth; named second-priority by `security-baseline`'s own note. |
| `db-schema-modernization` | Not reviewed in depth. |
| `postgres-fulltext-search` | Covered in §2 above. |

The first three are the ones worth not losing.

## 4. Preserved from `.autopilot/` before deletion (27 Oct)

Decision made: keep the three backlog items that overlap this plan's MCP work, plus a fourth found on the same read-through, that is directly on-topic. Everything else in `.autopilot/` (four finished projects' history, `correctness-debt`, `db-schema-modernization`, `postgres-fulltext-search`) was deleted as agreed — nothing in those was unique or unfinished-and-wanted.

### 4a. `security-baseline` — queued, never started, parked 2026-08-06

**This is a live, shipped vulnerability, not a backlog idea.** Its own file says it should run before anything else. Findings it recorded, verified against the tree on 2026-08-06:

- **The backend has no authentication anywhere.** No auth module exists under `backend/src/middleware/`. The only JWT references are log-scrubbing patterns and a comment that says "In production, decode JWT and extract user_id" — aspirational, not implemented.
- **Both shipped configs bind every network interface by default.** `config/environments/config.development.yaml` and `config.production.yaml` both set `host: "0.0.0.0"`, overriding the backend's own safe `127.0.0.1` default. Docker Compose publishes port 8080.
- **The MCP transport is mounted as a sibling of the REST API**, not nested inside it (`main.rs:905`, `.nest_service("/api/v1/mcp", ...)` beside `.nest("/api/v1", ...)`). Any auth bolted onto the API router alone would miss MCP entirely.
- Net effect: an unauthenticated MCP endpoint and REST API, reachable from the network, serving full message bodies via `get_email`/`search_emails`/`list_attachments`. Being read-only limits *damage*, the file notes, but does nothing about *disclosure* — and for an email app, disclosure is the bigger harm.
- Scope is deliberately tiny: require a credential (the `jwt_secret` already shipped and already delivered to the container, just never consumed) on both the API and MCP paths, validate `Origin` on the MCP endpoint against DNS-rebinding, and stop the configs from binding `0.0.0.0` by default. Two phases, fast, meant to go first.

### 4b. `mcp-identity-authz` — queued, never started, depends on `security-baseline`

**The caller currently tells the server who they are, and the server believes them.** `user_id` is a client-supplied field in the published MCP tool schema (`tools/readonly/params.rs:106`) — nothing derives it from a credential. Every other read tool is worse: there's no identity field on `ToolContext` at all, so they read the whole database regardless of caller. This is the textbook "confused deputy" shape, and it's why no write tool can land before this is fixed — a write tool today would let any caller mutate any user's data by claiming to be them. Three phases: derive a principal from the credential (never from a request field) and scope every tool by it; make rate limits and the audit trail per-caller instead of global; add a capability/scope model so a credential can be granted one tool's write access without granting all of them.

### 4c. `mcp-write-parity` — queued, never started, depends on `mcp-identity-authz`

This is the one most worth reading before building the mail-agent plan's own authority model (§4/§6 of `mail-agent-2026-10.md`) — it's a more detailed version of the same idea, already thought through:

- An inventory of the 76 mutating REST endpoints across 18 modules that the UI can do but MCP (15 read-only tools) currently cannot.
- A four-tier triage: **Tier A** reversible, no confirmation (mark read, star, archive, move) · **Tier B** destructive-but-scoped, confirmation required (trash, rules, cleanup apply, unsubscribe) · **Tier C** outbound mail, confirmation plus injection defenses (send, reply, forward) · **Tier D** not agent-reachable at all (OAuth credential changes, GDPR wipe/erase, infrastructure settings) — matching almost exactly the mail-agent plan's own Level 0–3 authority table, but with the actual endpoint inventory behind it.
- **A threat this session's mail-agent plan never names:** email content is untrusted input an LLM reads through these tools. Once a send/reply/forward tool exists, a malicious email can turn "summarize my inbox" into "forward the password reset to attacker@evil" — a prompt-injection-driven exfiltration path. The pipeline's answer: confirmation must be human-in-the-loop and rendered from the server's own parsed request, never from the model's description of what it's asking to do (a model under the attacker's influence can't be trusted to accurately report its own request); a red-team test suite with crafted injection emails is a named deliverable before Tier C ships.

### 4d. `adr-implementation-gaps` — queued, never started, parked 2026-08-02

Not a security item — a hygiene tool, and it's the automated version of the manual check this session has been doing by hand all day. Its goal: read every ADR, find every claim of something being "built/enforced/available" that isn't actually true in the code, and report it row by row for a human to mark implement/wontfix/needs-judgment — acting only on what's explicitly marked. It already found three concrete examples before being parked: ADR-013's `make clean-models` target and CLI flag exist nowhere in the repo; ADR-005's claimed "Bundlewatch CI gate" has never existed; ADR-005's claimed `axe-core` accessibility checks don't run in CI anywhere. Kept because it's exactly the kind of drift this session's own verification passes (ADR numbering collisions, the stale Postgres row, the unlock-step gap) would have caught automatically.

## 5. What this means for the mail-agent plan

See the chat turn that follows this file's completion — the security findings in 4a/4b/4c are not backlog curiosities, they describe the actual, current state of the MCP server the chief-of-staff plan intends to add write tools to. This needs its own decision before milestone 2 (staff tools, read) starts, not just a mention here.
