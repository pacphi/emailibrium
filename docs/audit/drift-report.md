# Documentation drift report

Evidence-backed inventory of where the doc corpus (cataloged in `inventory.md`) diverges from what
the code actually does. Phase 0 of the docs-accuracy-audit pipeline — **no remediation happens
here**; phases 1–5 fix these per persona, phase 6 gates the mechanical ones (routes, config keys)
in CI so they can't silently reopen.

> **Remediation status (as of the pipeline's optimization pass, phase 6 complete):** all 74
> findings below, across all six personas, have been fixed or formally amended. Findings are
> left exactly as originally worded — this is a point-in-time snapshot of what phase 0 found, not
> a live status board — but phase-by-phase remediation detail (what changed, why, and the
> Tier-3/qe-court evidence behind each fix) lives in `.autopilot/runs/docs-accuracy-audit.jsonl`
> (one firing record per phase) and, for the risk phase, `.autopilot/court/docs-accuracy-audit/
phase-6.md`. Two structural gaps found _during_ remediation (not originally in this report) are
> tracked separately as parking-lot items in `.autopilot/discovered/docs-accuracy-audit.jsonl` --
> they're real but out of this pipeline's scope, not unfixed findings from this list.

**Method:** six read-only investigation passes, one per persona, each required to cite a doc
`file:line` for the claim AND either a code `file:line` or an actually-executed command's output
for the contradicting reality — no unevidenced assertions. Two passes used the ground-truth
extraction scripts from this phase (`scripts/audit/extract-routes.sh`,
`scripts/audit/extract-config-keys.sh`) as their baseline. I personally re-ran the cited evidence
for 7 items across all 6 personas afterward (marked ✅ **spot-checked** below) and confirmed every
one; no fabricated citations were found.

## Summary

| Persona         |                   Findings | breaks-a-user | misleading | cosmetic |
| --------------- | -------------------------: | ------------: | ---------: | -------: |
| End user        |                         15 |             2 |         10 |        3 |
| Operator        |                         16 |             7 |          7 |        2 |
| API             | 19 (+2 confirmed-accurate) |             8 |          9 |        2 |
| Contributor     |                         13 |             2 |          9 |        2 |
| AI coding agent |                          5 |             1 |          4 |        0 |
| Decision reader |                          6 |             0 |          4 |        2 |
| **Total**       |                     **74** |        **20** |     **43** |   **11** |

_(Counted directly from every `- severity:` tag in this document's own body, not tallied by
hand — the phase-0 gate's Tier-3 review caught an earlier, manually-tallied version of this
table disagreeing with the findings below it. Re-derive per persona with
`grep -c '^- severity: breaks-a-user'` etc. inside each `## <persona>` section.)_

The single largest finding: **openapi.yaml documents 11 of the backend's 137 real routes (8%)** —
worse than the plan's original ~124-vs-12 estimate once nesting is fully resolved. See the API
section.

---

## End user

Docs: README.md, QUICKSTART.md, docs/user-guide.md, docs/user-interface-overview.md

### Nine keyboard shortcuts (/, C, R, F, E, #, J, K) are not implemented

- doc: docs/user-guide.md:191-198 — "`/` Focus search | `C` Compose new email | `R` Reply to current email | `F` Forward current email | `E` Archive current email | `#` Delete current email | `J` Next email in list | `K` Previous email in list"
- reality: `grep -rn "addEventListener('keydown'" frontend/apps/web/src` → only 5 listeners exist total: useCommandPalette.ts (Cmd+K, Escape), MoveDialog.tsx (Escape), a11y.ts and useFocusTrap.ts (Escape/Tab for modals), and an unused useKeyboard.ts hook. frontend/apps/web/src/features/email/EmailList.tsx:95-109 `handleKeyNavigation` only matches `ArrowDown`/`ArrowUp`, never 'j'/'k'. None of /, C, R, F, E, # are bound anywhere.
- persona: End user
- severity: misleading

### Cmd+Shift+A (select all) and ? (shortcut help) do nothing

- doc: docs/user-guide.md:201,203 — "`Cmd+Shift+A` Select all in current view" / "`?` Show keyboard shortcut help"
- reality: `grep -rlniE "keyboard.?shortcut|shortcut.?help" frontend/apps/web/src` → no shortcuts-help modal component exists; no keydown handler anywhere checks Shift+A or '?'.
- persona: End user
- severity: misleading

### Cmd+Enter only sends from Reply, not from Compose

- doc: docs/user-guide.md:200 — "`Cmd+Enter` Send email (in compose)"
- reality: frontend/apps/web/src/features/email/ComposeEmail.tsx:70-71 only handles `Escape`; no metaKey/ctrlKey check exists in that file. Only ReplyBox.tsx:37 implements `e.key === 'Enter' && (e.metaKey || e.ctrlKey)`.
- persona: End user
- severity: misleading

### Cmd+, does not open Settings

- doc: docs/user-guide.md:8,202 — "Navigate to Settings (gear icon in the sidebar or `Cmd+,`)" / "`Cmd+,` Open settings"
- reality: `grep -rn "addEventListener('keydown'" frontend/apps/web/src` (exhaustive list of 5 handlers, none referencing Settings navigation or the comma key) → no shortcut wired for Cmd+,.
- persona: End user
- severity: misleading

### "Embedding noise level (differential privacy)" setting does not exist

- doc: docs/user-guide.md:164 — "Configure embedding noise level (differential privacy)"
- reality: frontend/apps/web/src/features/settings/PrivacySettings.tsx (full file) has no noise/differential-privacy control; `grep -rniE "noise|differential" frontend/apps/web/src/features` and `backend/src` → zero relevant matches (only unrelated HDBSCAN clustering-noise terminology in backend/src/vectors/hdbscan.rs).
- persona: End user
- severity: misleading

### Privacy "Encryption at Rest" / master-password flow is a non-functional UI mock

- doc: docs/user-guide.md:163,251 — "Set master password for vector encryption" / "Enable/disable encryption at rest" / "Set a strong master password -- if lost, encrypted data cannot be recovered"
- reality: frontend/apps/web/src/features/settings/PrivacySettings.tsx:58-73 `handleSetPassword()` only calls `setEncryptionAtRest(true)`, never `setMasterPasswordHash`; code comment reads "// In production, hash the password and store via secure storage API." `grep -rn "encryptionAtRest|masterPasswordHash" backend/src` → zero matches; real encryption (backend/src/vectors/encryption.rs) is driven solely by backend/config.yaml's `encryption.enabled`/`master_password`, unrelated to this UI. The password is captured then discarded — there is nothing to "lose."
- persona: End user
- severity: breaks-a-user

### Privacy "audit log" is two hardcoded fake rows, not real activity

- doc: docs/user-guide.md:165,252 — "View audit log of vector store access" / "Review the audit log periodically in Settings > Privacy"
- reality: frontend/apps/web/src/features/settings/AccountSettings.tsx:32-47 — `const [auditLog] = useState<AuditLogEntry[]>([...two fixed entries...])` with comment "// In production, audit log entries would come from an API/react-query call." No fetch/query ever populates it.
- persona: End user
- severity: misleading

### Rules Studio has no "Category equals" condition in the backend model

- doc: docs/user-guide.md:139 — "Define conditions: ... Category equals"
- reality: backend/src/rules/types.rs:70-77 — `EmailField { From, To, Subject, Body, Labels, Date }` has no `Category` variant.
- persona: End user
- severity: misleading

### ✅ Rules Studio "Save and enable the rule" fails: frontend/backend JSON contract mismatch (spot-checked)

- doc: docs/user-guide.md:128-148 — "Creating a rule: ... 6. Save and enable the rule"
- reality: `frontend/apps/web/src/features/rules/RuleEditor.tsx:44-46` — `emptyCondition(): RuleCondition { return { field: 'from', operator: 'contains', value: '' }; }` (no `type` key) is posted as-is via frontend/packages/api/src/rulesApi.ts `createRule`/`validateRule`. `backend/src/rules/types.rs:38-39` — `#[serde(tag = "type", rename_all = "camelCase")] pub enum RuleCondition { ... }` requires a `type` discriminant; a body missing it fails JSON deserialization before the handler ever runs. Operator spellings also diverge (frontend `starts-with`/`matches-regex`/`not-contains` vs backend `startsWith`/`regex`, and `not-contains` has no backend equivalent at all). **Personally re-verified**: both cited files match exactly as described.
- persona: End user
- severity: breaks-a-user

### Settings AI page: wrong default provider, and no Gemini option

- doc: docs/user-guide.md:176 — "Generative AI provider: none (default), Ollama, or cloud (OpenAI, Anthropic, Gemini)"
- reality: frontend/apps/web/src/features/settings/hooks/useSettings.ts:159 — `DEFAULT_STATE.llmProvider = 'builtin'`, not `'none'`. frontend/apps/web/src/features/settings/AISettings.tsx:73-93 `LLM_PROVIDERS` array offers only None, Built-in (Local), Local (Ollama), OpenAI, Anthropic — no Gemini `<option>`, even though backend/src/vectors/generative.rs:560 fully supports `"gemini"` and onboarding's AISetup.tsx:339 does offer 'Google Gemini'.
- persona: End user
- severity: misleading

### README Tech Stack table: TypeScript version is stale

- doc: README.md:254 — "React 19, TypeScript 5.9, Vite 8, TanStack Router + Query, Zustand, Tailwind CSS"
- reality: `grep -n "\"typescript\"" frontend/apps/web/package.json` → `"typescript": "^6.0.3"`; consistent with the last several Dependabot-consolidation commits, so it is stale relative to the shipped major version.
- persona: End user
- severity: cosmetic

### Command Center "Quick actions" list doesn't match the real buttons

- doc: docs/user-guide.md:55 — "Quick actions: Start ingestion, open search, view insights"
- reality: frontend/apps/web/src/features/command-center/QuickActions.tsx:43-79 — actual actions are Clean Inbox, View Insights, Chat with AI, Manage Rules, Add Account, plus a separate Sync Now (Incremental/Full) split button. No "start ingestion" or "open search" quick action exists.
- persona: End user
- severity: cosmetic

### Compose editor is a plain Markdown textarea, not a rich text editor

- doc: docs/user-guide.md:126 — "Rich text editor with formatting toolbar"
- reality: frontend/apps/web/src/features/email/ComposeEmail.tsx:206-239 — body is a `<textarea placeholder="Compose your message (Markdown supported)...">` with only a Write/Preview toggle (rendered via `<ReactMarkdown>`); no bold/italic/list formatting toolbar exists.
- persona: End user
- severity: misleading

### Email reader shows no inline cluster/topic context

- doc: docs/user-interface-overview.md:58 — "Messages appear in a unified inbox spanning Gmail, Outlook, and IMAP, with cluster and topic context shown inline so you always know which project a thread belongs to."
- reality: `grep -niE "cluster|topic" frontend/apps/web/src/features/email/ThreadView.tsx` → no matches. Topic/cluster references exist only as sidebar filter groups and URL deep-link params (EmailSidebar.tsx, EmailClient.tsx), not inline in the reading pane.
- persona: End user
- severity: misleading

### Chat's default local model is GGUF/llama.cpp, not ONNX

- doc: docs/user-interface-overview.md:82 — "Backed by the tiered AI architecture — ONNX on-device by default, Ollama or cloud models opt-in."
- reality: `backend/src/vectors/generative_builtin.rs` header — "Built-in local LLM via llama-cpp-2 (ADR-021 addendum) ... Tier 0.5 generative model that loads a GGUF file directly into the backend process using the llama-cpp-2 crate," with `model_id: "qwen3-1.7b-q4km"` in backend/config.yaml. ONNX is the default _embedding_ provider, a separate tier from Chat's default generative model.
- persona: End user
- severity: cosmetic

---

## Operator

Docs: docs/setup-guide.md, docs/deployment-guide.md, docs/oauth-setup-guide.md, docs/configuration-reference.md, docker-compose.yml, docker-compose.dev.yml, secrets/README.md

### ✅ PostgreSQL is documented as a drop-in database swap, but the backend can't actually connect to it (spot-checked)

- doc: docs/deployment-guide.md:233 — "The backend uses SQLx with compile-time-checked queries that work against both SQLite and PostgreSQL -- switching is a matter of changing the `database_url` connection string."
- reality: `backend/Cargo.toml:37` → `sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "chrono", "uuid"] }` (no `postgres` feature); `backend/src/db/mod.rs:3,8,13-14` → `use sqlx::sqlite::{SqlitePool, SqlitePoolOptions}` / `pub pool: SqlitePool`. `grep -rln "Postgres\|PgPool" backend/src/` → no matches. The same doc's own recommendation table (lines 239-240) tells operators to use PostgreSQL for "Multi-user / team deployment" and gives `EMAILIBRIUM_DATABASE_URL="postgres://..."` (line 245) — the binary cannot act on any of it. **Personally re-verified**: `sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "chrono", "uuid"] }` confirmed at backend/Cargo.toml:37; `use sqlx::sqlite::{SqlitePool, SqlitePoolOptions}` confirmed in backend/src/db/mod.rs.
- persona: Operator
- severity: breaks-a-user

### setup-guide.md's Google/Microsoft OAuth redirect URIs don't correspond to any real route

- doc: docs/setup-guide.md:59,68,314-315 — "Add authorized redirect URI: `http://localhost:8080/api/auth/google/callback`" / "Redirect URI: **Web** > `http://localhost:8080/api/auth/microsoft/callback`"
- reality: backend/src/api/accounts.rs:7,39,168,202 → the only callback route is `GET /api/v1/auth/callback`, shared by both providers, built as `format!("{}/api/v1/auth/callback", oauth.redirect_base_url)`. Neither `/api/auth/google/callback` nor `/api/auth/microsoft/callback` exists in the router.
- persona: Operator
- severity: breaks-a-user

### deployment-guide.md's OAuth redirect URIs also don't match the real route (different wrong pattern)

- doc: docs/deployment-guide.md:141,155 — "Authorized redirect URI: `http://localhost:8080/api/v1/auth/gmail/callback` (development)..." / "Set the **Redirect URI**: `http://localhost:8080/api/v1/auth/outlook/callback`"
- reality: same as above — backend/src/api/accounts.rs:168,202 shows both Gmail and Outlook flows redirect to the single shared `/api/v1/auth/callback`. (docs/oauth-setup-guide.md:27,127,207 correctly documents `/api/v1/auth/callback`, confirming these two other docs are the ones out of sync.)
- persona: Operator
- severity: breaks-a-user

### configuration-reference.md documents the wrong default built-in LLM model id

- doc: docs/configuration-reference.md:186,211,219 — `generative.builtin.model_id` default `"qwen2.5-0.5b-q4km"`, and `EMAILIBRIUM_GENERATIVE_BUILTIN_MODEL_ID=qwen2.5-0.5b-q4km`
- reality: `backend/config.yaml:52` → `model_id: "qwen3-1.7b-q4km"`; `backend/src/vectors/config.rs:768-770` → `fn default_builtin_model() -> String { "qwen3-1.7b-q4km".to_string() }`, confirmed by test assertion at config.rs:1086. `qwen2.5-0.5b-q4km` doesn't even exist as an entry in `config/models-llm.yaml`.
- persona: Operator
- severity: breaks-a-user

### setup-guide.md repeats the same wrong default built-in model id

- doc: docs/setup-guide.md:83 — "**Classification**: Built-in LLM (`qwen2.5-0.5b-q4km`) — runs locally, downloads ~350 MB on first use"
- reality: same as above — actual default is `qwen3-1.7b-q4km` (backend/config.yaml:52, backend/src/vectors/config.rs:768-770).
- persona: Operator
- severity: misleading

### deployment-guide.md points operators at a `configs/` directory that doesn't exist

- doc: docs/deployment-guide.md:204-210 — "Docker Compose mounts environment-specific config from the `configs/` directory: `configs/ config.development.yaml ... config.production.yaml ...`"
- reality: `find . -iname configs` → no such directory. The real path is `config/environments/` — confirmed by docker-compose.yml:37 → `- ./config/environments/config.${APP_ENV:-development}.yaml:/app/config.yaml:ro`.
- persona: Operator
- severity: misleading

### setup-guide.md also cites the nonexistent `configs/` path

- doc: docs/setup-guide.md:177 — "Native dev uses SQLite by default (configured in `configs/config.development.yaml`)."
- reality: same as above — the file is at `config/environments/config.development.yaml`.
- persona: Operator
- severity: misleading

### `EMAILIBRIUM_ANTHROPIC_API_KEY` is documented but never read by the backend

- doc: docs/setup-guide.md:151,328 — "Anthropic | `EMAILIBRIUM_ANTHROPIC_API_KEY` | [console.anthropic.com]..."
- reality: `grep -rn "EMAILIBRIUM_ANTHROPIC_API_KEY" backend/src/` → zero matches. Anthropic is only reachable as a `generative.cloud.provider` value (backend/src/vectors/generative.rs:556-557), whose API key comes from `generative.cloud.api_key_env`, which defaults to `EMAILIBRIUM_CLOUD_API_KEY` (backend/src/vectors/config.rs:795-797), not a per-provider variable. Setting the documented variable has no effect.
- persona: Operator
- severity: misleading

### Docker OAuth secret files are placed under names the backend never reads

- doc: docs/deployment-guide.md:150,170 — "Or place them in `secrets/dev/google_client_id` and `secrets/dev/google_client_secret` for Docker." (and the Microsoft equivalent at line 170)
- reality: backend/entrypoint.sh:7-11 converts each `/run/secrets/<name>` file into an env var named `<NAME>` uppercased with no prefix (e.g. `GOOGLE_CLIENT_ID`). But config/app.yaml:99-104,121 and backend/src/vectors/config.rs (`GmailOAuthConfig`/`OutlookOAuthConfig` defaults) require `EMAILIBRIUM_GOOGLE_CLIENT_ID`, `EMAILIBRIUM_GOOGLE_CLIENT_SECRET`, `EMAILIBRIUM_MICROSOFT_CLIENT_ID`, `EMAILIBRIUM_MICROSOFT_CLIENT_SECRET`, `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD`. The unprefixed names the entrypoint actually produces are never consumed. Docker-deployed OAuth (and at-rest token encryption) cannot pick up these secrets as documented.
- persona: Operator
- severity: breaks-a-user

### `REDIS_URL` is documented/wired as the Redis knob, but the backend never reads that variable

- doc: docs/setup-guide.md:334 — `REDIS_URL` | `redis://redis:6379` | "Redis connection (Docker)"
- reality: docker-compose.yml:31 sets `REDIS_URL: redis://redis:6379` on the backend container, but backend/src/main.rs:320-321 reads `config.redis.url`/`config.redis.enabled`, sourced only via Figment's `EMAILIBRIUM_REDIS_URL`/`EMAILIBRIUM_REDIS_ENABLED`. `redis.enabled` defaults to `false` and nothing in the repo sets the `EMAILIBRIUM_`-prefixed pair, so Redis caching never actually turns on in the documented Docker stack.
- persona: Operator
- severity: misleading

### ✅ setup-guide.md lists "Make" as a prerequisite and never mentions `just` (spot-checked)

- doc: docs/setup-guide.md:15 — `| Make | 3.81+ | \`xcode-select --install\` (macOS) or \`apt install build-essential\` |`
- reality: `ls justfile Makefile` → `Makefile: No such file or directory`, `justfile` present. Every command in this same doc is `just ...`, yet `just` never appears in the prerequisites table. **Personally re-verified**: confirmed identical output — no Makefile anywhere in the repo root, only `justfile`.
- persona: Operator
- severity: breaks-a-user

### deployment-guide.md has the identical stale "Make" prerequisite

- doc: docs/deployment-guide.md:11 — `| Make | Any | Pre-installed on macOS/Linux |`
- reality: same as above — no Makefile exists, only `justfile`; every command shown in the "Build Commands Reference" table is a `just` invocation, but `just` is absent from the prerequisites table.
- persona: Operator
- severity: breaks-a-user

### ONNX model download size is wrong, and inconsistent within the same doc

- doc: docs/setup-guide.md:82,98,307 — "downloads ~23 MB on first use" (line 82), "ONNX embedding model (23 MB)" (line 98), vs. "Models download from Hugging Face (~30 MB for all-MiniLM-L6-v2)" (line 307)
- reality: `ls -la backend/.fastembed_cache/models--Qdrant--all-MiniLM-L6-v2-onnx/blobs/` → the cached `model.onnx` blob is `90387630` bytes (~86 MiB / ~90 MB) — roughly 3-4x larger than either quoted figure, and the doc contradicts itself (23 MB vs 30 MB) for the same model in the same file.
- persona: Operator
- severity: cosmetic

### `oauth.frontend_url` is a real, production-relevant config key with zero documentation

- doc: docs/configuration-reference.md:225-240 — the OAuth (`oauth.*`) key table lists `redirect_base_url`, `gmail.*`, `outlook.*` only; no `frontend_url` row anywhere.
- reality: backend/src/vectors/config.rs:837-838,933 → `OAuthConfig.frontend_url` (default `http://localhost:3000`); used in backend/src/api/accounts.rs:358,365,459 to build the post-OAuth redirect. A production deployment with a different frontend origin needs to override this via `EMAILIBRIUM_OAUTH_FRONTEND_URL`, undocumented.
- persona: Operator
- severity: misleading

### Rate-limiting / HSTS / CSP config surface is entirely undocumented in configuration-reference.md

- doc: docs/configuration-reference.md:252-257 — the "Security (`security.*`)" section documents only `security.allowed_origins` and `security.csp_enabled`.
- reality: `bash scripts/audit/extract-config-keys.sh` output shows real, additional keys `security.rate_limit.{enabled,requests_per_second,burst_size}`, `security.hsts.{enabled,max_age_secs,include_subdomains}`, plus a separate, real, operator-facing set of raw env vars: `RATE_LIMIT_PRESET`, `RATE_LIMIT_AUTH_START`, `RATE_LIMIT_AUTH_CALLBACK`, `RATE_LIMIT_SESSION_STATUS`, `RATE_LIMIT_TOKEN_REFRESH`, `RATE_LIMIT_REDIS_URL`, `RATE_LIMIT_ENABLE_REDIS`, `RATE_LIMIT_REDIS_FALLBACK`, `RATE_LIMIT_ENABLE_USER_LIMITS`, `RATE_LIMIT_USER_MULTIPLIER` (backend/src/middleware/rate_limit.rs:264-320), and `HSTS_MAX_AGE`, `HSTS_PRELOAD`, `CSP_REPORT_URI`, `CSP_ALLOW_INLINE_STYLES`, `CSP_CONNECT_SRC_ORIGINS` (backend/src/middleware/security_headers.rs:55-68). None of these production security knobs are mentioned anywhere in the reference doc.
- persona: Operator
- severity: misleading

### `qdrant` service is defined in docker-compose.yml but missing from deployment-guide.md's service table

- doc: docs/deployment-guide.md:195-200 — "Compose Services" table lists only `backend`, `frontend`, `postgres`, `redis`.
- reality: docker-compose.yml:200-223 defines a full `qdrant` service (profile-gated, healthcheck, named volume) that the same doc's "Vector Store Backend" section (lines 248-268) tells operators to switch to via `EMAILIBRIUM_STORE_BACKEND=qdrant` / `docker compose --profile qdrant up` — but the service itself is never listed in the table.
- persona: Operator
- severity: cosmetic

---

## API

Docs: docs/api/openapi.yaml (+ README.md's MCP tool table, docs/ADRs/ADR-028-mcp-tool-calling-chat.md)

**Coverage: 11/137 real routes documented in openapi.yaml (8.0%)** — computed by `bash
scripts/audit/extract-routes.sh` against `docs/api/openapi.yaml`'s `paths:` entries. Below,
undocumented modules are grouped rather than listed per-route (137 individual entries would dwarf
the rest of this report); the route lists in each group are the literal `extract-routes.sh` output.

### Entire `/ai/*` module (22 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: `bash scripts/audit/extract-routes.sh` + backend/src/api/mod.rs:35 → `/ai/chat`, `/ai/chat/confirm`, `/ai/chat/sessions`, `/ai/chat/sessions/{id}`, `/ai/chat/stream`, `/ai/config/app`, `/ai/config/classification`, `/ai/config/prompts`, `/ai/config/tuning`, `/ai/embedding-catalog`, `/ai/model-catalog`, `/ai/model-status/{model_id}`, `/ai/models`, `/ai/providers`, `/ai/providers/{provider}/disable`, `/ai/providers/{provider}/enable`, `/ai/reembed`, `/ai/reindex-status`, `/ai/settings`, `/ai/status`, `/ai/switch-model`, `/ai/system-info`
- persona: API
- severity: misleading

### Entire `/emails/*` module (22 routes) — the core CRUD domain — is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: `bash scripts/audit/extract-routes.sh` + backend/src/api/emails.rs:29-52 → list, get, delete, archive, star, read, move, spam/unspam, restore, reply, forward, send, thread, categories, counts, labels, trash, attachments incl. zip download — 22 routes, zero documented
- persona: API
- severity: breaks-a-user

### Entire `/auth/*` account-connection module (10 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/accounts.rs → `/auth/accounts`, `/auth/accounts/{id}`, `/auth/accounts/{id}/remove-labels`, `/auth/accounts/{id}/status`, `/auth/accounts/{id}/unarchive`, `/auth/callback`, `/auth/gmail/connect`, `/auth/imap/connect`, `/auth/imap/test`, `/auth/outlook/connect`
- persona: API
- severity: breaks-a-user

### Entire `/cleanup/*` module (10 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/cleanup/api → `/cleanup/apply/{id}`, `/cleanup/apply/{id}/cancel`, `/cleanup/apply/{id}/stream`, `/cleanup/plan`, `/cleanup/plan/{id}`, `/cleanup/plan/{id}/operations`, `/cleanup/plan/{id}/refresh`, `/cleanup/plan/{id}/sample`, `/cleanup/plans`, `/cleanup/telemetry` — the entire mailbox-cleanup wizard backend (ADR-030) has no public contract
- persona: API
- severity: breaks-a-user

### Entire `/consent/*` GDPR module (8 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/consent.rs → `/consent`, `/consent/{provider}`, `/consent/audit`, `/consent/erase`, `/consent/export`, `/consent/gdpr`, `/consent/gdpr/{consent_type}`, `/consent/privacy-audit` — the ADR-017 GDPR compliance surface has no OpenAPI coverage
- persona: API
- severity: breaks-a-user

### Entire `/wipe/*` data-destruction module (5 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/wipe.rs → `/wipe/all`, `/wipe/scheduled`, `/wipe/scheduled/{user_id}`, `/wipe/user/{user_id}`, `/wipe/vectors` — destructive endpoints with no public contract at all
- persona: API
- severity: breaks-a-user

### Entire `/evaluation/*` module (8 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/evaluation.rs → `/evaluation/ab-tests`, `/evaluation/ab-tests/{test_id}`, `/evaluation/ab-tests/{test_id}/conclude`, `/evaluation/ab-tests/{test_id}/observe`, `/evaluation/clustering-quality`, `/evaluation/ir-metrics`, `/evaluation/report`, `/evaluation/search-quality`
- persona: API
- severity: misleading

### `/ingestion/*` has 8 additional undocumented control/status routes beyond the 4 covered

- doc: docs/api/openapi.yaml:235-364 — only `/ingestion/status` (SSE), `/ingestion/start`, `/ingestion/pause`, `/ingestion/resume` are documented
- reality: backend/src/api/ingestion.rs:76-87 → `/ingestion/resume-checkpoint`, `/ingestion/checkpoint`, `/ingestion/embedding-status`, `/ingestion/poll-status`, `/ingestion/poll-toggle`, `/ingestion/progress`, `/ingestion/backfill-progress`, `/ingestion/lock-status` all exist and are unrouted in the spec
- persona: API
- severity: misleading

### Entire `/clustering/*` module (6 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/clustering.rs → `/clustering/clusters`, `/clustering/clusters/{id}`, `/clustering/clusters/{id}/pin`, `/clustering/clusters/{id}/unpin`, `/clustering/recluster`, `/clustering/status` — ADR-009 GNN clustering has no OpenAPI coverage
- persona: API
- severity: misleading

### Entire `/rules/*` module (6 routes) is completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/rules.rs → `/rules`, `/rules/{id}`, `/rules/{id}/run`, `/rules/suggestions`, `/rules/test`, `/rules/validate` — ADR-014 rule engine has no OpenAPI coverage
- persona: API
- severity: misleading

### `/interactions/*` (4) and `/learning/*` (4) modules are completely undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/interactions.rs → `/interactions/recent`, `/interactions/search`, `/interactions/{id}/click`, `/interactions/{id}/feedback`; backend/src/api/learning.rs → `/learning/consolidate`, `/learning/feedback`, `/learning/metrics`, `/learning/session` (ADR-004 SONA learning has no OpenAPI coverage)
- persona: API
- severity: misleading

### `/backup/*` (3), `/unsubscribe/*` (3), and remaining `/vectors/*` (3) routes are undocumented

- doc: docs/api/openapi.yaml — no `paths:` entry for any of these
- reality: backend/src/api/backup.rs (`/backup/restore`, `/backup/stats`, `/backup/trigger`), backend/src/api/unsubscribe.rs (`/unsubscribe`, `/unsubscribe/preview`, `/unsubscribe/undo/{batch_id}`), backend/src/api/vectors.rs:40-41 (`/vectors/models`, `/vectors/quantization`, `/vectors/search/hybrid`)
- persona: API
- severity: misleading

### The MCP mount `/api/v1/mcp` has no note in openapi.yaml explaining it exists outside `paths:`

- doc: docs/api/openapi.yaml — no mention
- reality: backend/src/main.rs, backend/src/mcp/mod.rs → `/api/v1/mcp` is an opaque Streamable-HTTP service mount, not a REST path — its absence from `paths:` isn't wrong, but nothing in the doc tells a reader this surface exists at all
- persona: API
- severity: cosmetic

### ✅ `/insights/recurring` is a hallucinated path — the real route is `/insights/recurring-senders` (spot-checked)

- doc: docs/api/openapi.yaml:403 — "`/insights/recurring:` ... summary: Recurring sender patterns"
- reality: backend/src/api/insights.rs:85 → `.route("/recurring-senders", get(recurring))`. A client calling the documented path gets a 404. **Personally re-verified**: both citations confirmed exactly as described.
- persona: API
- severity: breaks-a-user

### `/vectors/health`, `/vectors/stats`, `/vectors/search/semantic` response schemas document snake_case fields; real handlers serialize camelCase

- doc: docs/api/openapi.yaml:59-73,180-193,216-224 — examples use `store_healthy`, `total_vectors`, `memory_bytes`, `index_type`, `latency_ms`, `email_id`
- reality: backend/src/vectors/types.rs:149-167 (`HealthStatus`, `VectorStats`) and backend/src/api/vectors.rs:58-88 (`SearchResponse`, `SearchResultItem`) all carry `#[serde(rename_all = "camelCase")]` — real keys are `storeHealthy`, `totalVectors`, `memoryBytes`, `indexType`, `latencyMs`, `emailId`. The search request schema is also incomplete: the real `SemanticSearchRequest` (vectors.rs:46-56) accepts an undocumented `mode` field ("semantic"/"hybrid"/"keyword") that silently redirects to the hybrid engine.
- persona: API
- severity: breaks-a-user

### `/insights/subscriptions` and `/insights/report` response schemas document snake_case fields; real handlers serialize camelCase

- doc: docs/api/openapi.yaml:379-401,451-467 — `sender_name`, `email_count`, `recurrence_pattern`, `has_unsubscribe`, `first_seen`, `last_seen`, `estimated_read_rate`, `total_emails`, `top_senders`, `subscription_count`, `estimated_time_savings_minutes`, `generated_at`
- reality: backend/src/vectors/insights.rs:111-112,151-152 — `SubscriptionInsight`/`InboxReport` both carry `#[serde(rename_all = "camelCase")]`; real keys are `senderName`, `emailCount`, `recurrencePattern`, `hasUnsubscribe`, `firstSeen`, `lastSeen`, `estimatedReadRate`, `totalEmails`, `topSenders`, `subscriptionCount`, `estimatedTimeSavingsMinutes`, `generatedAt`
- persona: API
- severity: breaks-a-user

### Undocumented `/insights/*` routes beyond the mispathed `/recurring`: `temporal` and `topics`

- doc: docs/api/openapi.yaml:369-473 — only `/insights/subscriptions`, `/insights/recurring` (wrong path), `/insights/report` appear
- reality: backend/src/api/insights.rs:84-88 → `/insights/temporal` (day/hour/category volume analytics) and `/insights/topics` (AI-assigned topic clusters) both exist live with zero OpenAPI coverage
- persona: API
- severity: misleading

### `/ingestion/start` request schema omits a real field (`source`)

- doc: docs/api/openapi.yaml:646-654 — `StartRequest` schema lists only `account_id`, `full_sync`
- reality: backend/src/api/ingestion.rs:49-55 → real `StartRequest` also accepts optional `source` ("onboarding"/"manual_sync"/"inbox_clean"/"poll") used for job attribution, undocumented
- persona: API
- severity: cosmetic

### ADR-028's tool→REST mapping table is stale and points at wrong/removed endpoint shapes

- doc: docs/ADRs/ADR-028-mcp-tool-calling-chat.md:204,206 — `search_emails` maps to `GET /api/v1/vectors/search` + `GET /api/v1/emails`; `get_email_thread` maps to `GET /api/v1/emails/:id/thread`
- reality: `bash scripts/audit/extract-routes.sh` → no `/api/v1/vectors/search` route exists (real search routes are `/vectors/search/semantic`, `/vectors/search/hybrid`, `/vectors/search/similar/{email_id}`); the real thread route is `/api/v1/emails/thread/{thread_id}` (backend/src/api/emails.rs:40), not `/api/v1/emails/:id/thread`. The table was never corrected after the tool set grew 7→15 (the ADR's own later addendum at line 852 acknowledges the growth but not this table).
- persona: API
- severity: misleading

### CONFIRMED ACCURATE (not a defect) — `/vectors/classify` schema

- doc: docs/api/openapi.yaml:518-570 — `ClassifyRequest`/`ClassifyResponse` schemas
- reality: backend/src/api/vectors.rs:99-113,418-444 → no `rename_all`, so real JSON keys match exactly as documented. Recorded here because the phase-0 gate required spot-checking ≥5 documented endpoints against handlers, and this is one that passed.
- persona: API
- severity: n/a (verified accurate)

### CONFIRMED ACCURATE (not a defect) — MCP tools ARE documented and match the live registry; README.md is the real doc, not ADR-028

- doc: README.md:159-186 — 15 read-only tools grouped by domain, plus 3 resources and 2 prompts
- reality: backend/src/tools/registry.rs:49-153 `declarations()` → exactly 15 tools, every name matching a real `declare(...)` entry. This directly answers phase 0's brief to check whether MCP tool documentation exists: it does, and it's accurate — the gap is ADR-028's stale REST-mapping table (above), not the tool list itself.
- persona: API
- severity: n/a (verified accurate)

---

## Contributor

Docs: docs/maintainer-guide.md, docs/architecture.md, docs/releasing.md

### `axe-core` CI claim has no basis in the repo

- doc: docs/maintainer-guide.md:294 — "`axe-core` accessibility checks run in CI"
- reality: `grep -rn axe .github/workflows/ frontend/package.json frontend/apps/web/package.json` → no matches anywhere; CI's jobs contain no accessibility step
- persona: Contributor
- severity: misleading

### ✅ Documented Storybook launch command doesn't exist (spot-checked)

- doc: docs/architecture.md:217 — "Run via `pnpm storybook` in the web app."
- reality: frontend/apps/web/package.json — `scripts` block has no `storybook` entry (only a `storybook` devDependency); running `pnpm storybook` fails with "Missing script". Correct command is `npx storybook dev` (frontend/justfile:92-93, `just storybook`). **Personally re-verified**: grep of package.json shows `storybook` appears only in the devDependencies block, never in scripts.
- persona: Contributor
- severity: breaks-a-user

### ADR numbering instructions collide with existing ADRs

- doc: docs/maintainer-guide.md:552 — "Number it sequentially (next: ADR-011)."
- reality: `ls docs/ADRs/` → ADR-011 through ADR-032 already exist (29 files); following the doc literally collides with the existing ADR-011-onnx-runtime-embedding-provider.md
- persona: Contributor
- severity: breaks-a-user

### Backend `api/` handler count is understated

- doc: docs/maintainer-guide.md:130 — "`api/` \| 11 handler files [...]"
- reality: `ls backend/src/api/` → 17 handler files (missing from the doc: attachments.rs, emails.rs, provider_helpers.rs, rules.rs, unsubscribe.rs, wipe.rs)
- persona: Contributor
- severity: misleading

### Backend `vectors/` file count is more than double what's documented

- doc: docs/maintainer-guide.md:133 — "`vectors/` \| 22 files \| Core engine..."
- reality: `ls backend/src/vectors/ | wc -l` → 48 files (e.g. qdrant_store.rs, sqlite_store.rs, chat_orchestrator.rs, rag.rs, ewc.rs, model_catalog.rs, model_download.rs, model_integrity.rs, model_registry.rs, reranker.rs, tool_calling.rs all undocumented)
- persona: Contributor
- severity: misleading

### `architecture.md`'s Module Structure omits most of `backend/src/`

- doc: docs/architecture.md:143-195 — tree lists only `api/`, `db/`, `content/`, `vectors/`
- reality: `ls backend/src/` → also contains cache/, cleanup/, config/, email/, events/, mcp/, middleware/, rules/, tools/, sync_lock.rs — none appear in the doc's tree
- persona: Contributor
- severity: misleading

### Bounded-context count disagrees between the two docs and reality

- doc: docs/maintainer-guide.md:486 ("Five bounded contexts... table stops at DDD-005") vs. docs/architecture.md:48 ("seven bounded contexts, DDD-000 through DDD-007")
- reality: `ls docs/DDDs/` → DDD-000 through DDD-010 exist (10 numbered contexts plus the context map); both docs undercount and disagree with each other
- persona: Contributor
- severity: misleading

### ADR count is stale by nearly 3x

- doc: docs/maintainer-guide.md:469 — "10 Architecture Decision Records... (table lists ADR-001 through ADR-010 only)"
- reality: `ls docs/ADRs/` → 29 ADR files (ADR-001 through ADR-032, including ADR-032 documenting the very make-to-just migration this audit checks for elsewhere)
- persona: Contributor
- severity: misleading

### PostgreSQL described as an optional future upgrade, but it's a required service today

- doc: docs/maintainer-guide.md:379 — "If you need multi-user or multi-node deployment, consider replacing SQLite with PostgreSQL..."
- reality: docker-compose.yml:47-51 — backend service has `depends_on: postgres: condition: service_healthy` as a hard dependency; postgres is already required in the production compose stack, not hypothetical
- persona: Contributor
- severity: misleading

### Architecture's DATA TIER diagram omits PostgreSQL, Redis, and Qdrant

- doc: docs/architecture.md:38-43 — DATA TIER block lists only "SQLite", "Vector Store", "Moka Cache", "Sync Queue + Checkpoints"
- reality: docker-compose.yml:124-158 runs required postgres and redis services; backend/Cargo.toml:41 includes the redis crate; backend/src/vectors/qdrant_store.rs plus `EMAILIBRIUM_STORE_QDRANT_URL` show Qdrant is a real, wired backend option — none appear in the diagram
- persona: Contributor
- severity: misleading

### Node.js version prerequisite doesn't match the pinned toolchain

- doc: docs/maintainer-guide.md:84 — "Node.js \| 26+ \| Frontend toolchain"
- reality: `.nvmrc` → `24`; frontend/package.json → `"node": ">=24"`. CI does install Node 26, but the repo's own version-pin files say 24, contradicting the doc's local-setup prerequisite.
- persona: Contributor
- severity: misleading

### Storybook major version and story count are both stale

- doc: docs/architecture.md:217 — "use **Storybook 8**", "currently **9 stories**"
- reality: frontend/apps/web/package.json → `"storybook": "^10.4.6"`; `find frontend/apps/web -iname '*.stories.tsx'` → 11 files (undocumented: CleanupReview.stories.tsx, CleanupHistory.stories.tsx)
- persona: Contributor
- severity: cosmetic

### `docs/releasing.md` Useful Commands table has duplicate rows

- doc: docs/releasing.md:151-158 — `just release-check`, `just release-tag`, `just release-push` each appear twice
- reality: root justfile defines each recipe exactly once (lines 632, 637, 647) — the doc table over-represents them via copy-paste duplication
- persona: Contributor
- severity: cosmetic

---

## AI coding agent

Docs: CLAUDE.md, AGENTS.md

### Rust version stale in CLAUDE.md (1.96 vs actual 1.97)

- doc: CLAUDE.md:11 — "Rust 1.96 (edition 2021)"
- reality: backend/Cargo.toml (`rust-version = "1.97"`), backend/rust-toolchain.toml (`channel = "1.97.0"`). Commit `04b28e5` (2026-07-31) bumped Rust 1.96→1.97 across README/setup/deployment/maintainer-guide but explicitly skipped CLAUDE.md.
- persona: AI coding agent
- severity: misleading

### TypeScript version stale in CLAUDE.md (5.9 vs actual ^6.0.3)

- doc: CLAUDE.md:15 — "TS 5.9"
- reality: frontend/package.json and all frontend/packages/\*/package.json pin `"typescript": "^6.0.3"`. Dependabot commit `abe0b15` bumped 5.9.3→6.0.2 on 2026-03-27; CLAUDE.md was edited again on 2026-06-15 (`f59e50e`) without fixing this.
- persona: AI coding agent
- severity: misleading

### `just audit` documented as npm audit, actually runs pnpm audit

- doc: CLAUDE.md:30 — "`just audit       # cargo-audit + npm audit`"
- reality: frontend/justfile:151 — `@audit: {{ PNPM }} audit --prod`. No npm lockfile exists anywhere under `frontend/` (only pnpm-lock.yaml) — this is an explicitly pnpm/Turborepo monorepo per CLAUDE.md's own Layout table two lines earlier.
- persona: AI coding agent
- severity: misleading

### ✅ AGENTS.md CLI table cites 6 SKILL.md files that do not exist (spot-checked)

- doc: AGENTS.md:96-101 — CLI reference table pointing to `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md`, `gitnexus-impact-analysis/SKILL.md`, `gitnexus-debugging/SKILL.md`, `gitnexus-refactoring/SKILL.md`, `gitnexus-guide/SKILL.md`, `gitnexus-cli/SKILL.md`
- reality: `find . -iname "*gitnexus*"` (excluding node_modules/.git) → zero results anywhere in the repo; no `.claude/skills/gitnexus/` directory exists. **Personally re-verified**: identical zero-result output, and the exact 6-row table confirmed at AGENTS.md:96-101.
- persona: AI coding agent
- severity: breaks-a-user

### Claimed auto-refresh PostToolUse hook for GitNexus index doesn't exist

- doc: AGENTS.md:90 — "> Claude Code users: A PostToolUse hook handles this automatically after `git commit` and `git merge`."
- reality: `grep -rn -i "gitnexus" .claude/settings.json .claude/settings.local.json .claude/hooks/ .claude/helpers/` → zero matches. No PostToolUse matcher is tied to `git commit`/`git merge` invoking `gitnexus analyze`.
- persona: AI coding agent
- severity: misleading

---

## Decision reader

Docs: docs/ADRs/**, docs/DDDs/**, docs/plan/**, docs/research/**, OPTIMIZATION_SPEC.md (scoped investigation — see `inventory.md` methodology note; not every one of the 64 files was deep-read)

### ✅ ADR-005 claims a "Bundlewatch CI gate" that does not exist anywhere in the repo (spot-checked)

- doc: docs/ADRs/ADR-005-tauri-to-web-spa-migration.md:65 — "Initial bundle size (gzipped) | < 200KB | Bundlewatch CI gate"
- reality: `grep -rn -i "bundlewatch" .github/workflows/` → no matches in any workflow file. **Personally re-verified**: identical zero-result output; also confirmed the exact claim text at ADR-005:65.
- persona: Decision reader
- severity: misleading

### march-2026-audit.v2.md's CI job enumeration names jobs that don't exist / aren't in that workflow, even though the total count coincidentally still matches

- doc: docs/plan/march-2026-audit.v2.md:301 — "CI jobs: 11 (format, lint, clippy, test, audit, lighthouse, bundlewatch, markdown, yaml, shell)"
- reality: `.github/workflows/ci.yml` top-level `jobs:` keys → rust-format, rust-clippy, rust-build, rust-extra-checks, rust-test, rust-audit, frontend-install, frontend-quality, validate-markdown, validate-yaml, validate-shell = 11 jobs total, but no job is named/related to "bundlewatch" (doesn't exist, per the finding above) and "lighthouse" lives in a separate workflow file, not in ci.yml. The "11" figure is right only by coincidence — the underlying job list has drifted.
- persona: Decision reader
- severity: misleading

### ADR-003 still says "Status: Proposed" but RuVector is the live, wired default vector-store backend

- doc: docs/ADRs/ADR-003-ruvector-vector-database.md:3 — "**Status**: Proposed"
- reality: backend/Cargo.toml:114 (`ruvector-core` dependency, "ADR-003: RuVector as primary"); backend/src/vectors/mod.rs:189-206 (default match arm constructs `RuVectorStore::new(...)`) — the decision has clearly shipped, not merely proposed
- persona: Decision reader
- severity: misleading

### ADR-003's architecture diagram describes Qdrant fallback as "behind feature flag" but it's a runtime config match, not a Cargo feature

- doc: docs/ADRs/ADR-003-ruvector-vector-database.md:22 — "+-- QdrantStore (fallback, behind feature flag)"
- reality: backend/Cargo.toml `[features]` block has no `qdrant` feature; backend/src/vectors/mod.rs selects QdrantVectorStore via a runtime `match` on `config.store.backend == "qdrant"`, not `#[cfg(feature = "qdrant")]`
- persona: Decision reader
- severity: cosmetic

### Three ADR files collide on the number 021

- doc: docs/ADRs/ADR-021-addendum-rust-backend-llm.md, docs/ADRs/ADR-021-built-in-local-llm.md, docs/ADRs/ADR-021-clustering-performance.md — all three self-identify as "ADR-021"
- reality: `ls docs/ADRs/ADR-021*` → returns exactly these 3 files sharing the same number (already flagged in `inventory.md`'s closing note; formalized here as a drift item)
- persona: Decision reader
- severity: misleading

### Multiple pre-ADR-032 docs still reference `make` targets that no longer exist

- doc: docs/ADRs/ADR-013-ai-model-lifecycle-management.md:87 (`make clean-models`); docs/plan/predecessor-recommendations.md:36 (`make setup`); docs/plan/ci-potential-improvements.md:33,198 (`make test-ai`); docs/plan/march-2026-audit.md:348,368 (`make install`, `make dev`, `make docker-secrets`); docs/plan/llm-implementation-supplemental.md:470,490 (`make download-models`, `make dev`); docs/plan/model-catalog-externalization.md:272 (`make models`); docs/plan/inception.md:2738-2854 (16 more `make` targets)
- reality: `find . -maxdepth 2 -iname Makefile` → no matches (all three Makefiles deleted per ADR-032, Date: 2026-07-31, Status: Accepted); `find . -maxdepth 2 -iname justfile` → `./justfile`, `./frontend/justfile`, `./backend/justfile` exist instead
- persona: Decision reader
- severity: cosmetic

---

## Not yet investigated (explicitly out of phase-0 scope)

`docs/DDDs/**` (12 files) were inventoried but not individually drift-checked beyond the
bounded-context-count discrepancy above (Contributor section) — a full DDD-vs-code aggregate
audit is phase 4/5 work, not phase 0's. `docs/evaluation/**`, `docs/test-plan-group-by-sender.md`,
and `CHANGELOG.md` were cataloged in `inventory.md` but not drift-checked at all — no persona in
the plan names them as primary remediation targets, so they carry no findings here by omission,
not by confirmed accuracy.
