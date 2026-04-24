# Changelog

All notable changes to Emailibrium are documented here.
This project follows [Semantic Versioning](https://semver.org/).

<!-- This file is auto-maintained by the release workflow on each tag. -->

## [v0.1.0] - 2026-04-24

### Bug Fixes

- Wire all services, remove runtime stubs, harden infrastructure

Complete service wiring — all 10 modules now initialized in VectorService
and exposed via API endpoints. Removes all runtime stubs, mock data, and
TODO/FIXME comments from production code.

    Service wiring:
    - VectorService initializes all 10 services: embedding, store, categorizer,
      hybrid_search, cluster_engine, learning_engine, interaction_tracker,
      insight_engine, backup_service, quantization_engine, ingestion_pipeline
    - Backup restore on startup when enabled
    - 5 new API route files: clustering, learning, interactions, evaluation, backup
    - vectors.rs upgraded with hybrid search endpoint + interaction tracking
    - ingestion.rs wired to real IngestionPipeline (was stub)

    Runtime stub removal (backend):
    - OllamaEmbeddingModel: real reqwest HTTP client replacing "not yet integrated" stub
    - MockEmbeddingModel: only used when config.provider="mock" (no silent fallback)
    - Content extractors: honest ADR-006 references replacing TODO comments
    - Zero TODO/FIXME/stub remaining in non-test backend code

    Runtime stub removal (frontend):
    - RecentActivity: empty state replacing hardcoded mock events
    - ClusterVisualization: empty state replacing mock cluster data
    - OnboardingFlow: real OAuth callback replacing mockAccount creation
    - EmailClient: reclassify/move handlers wired to learning feedback API
    - SubscriptionsPanel: bulk unsubscribe wired to real API
    - TopicsPanel: cluster navigation wired
    - New learningApi.ts in @emailibrium/api package

    Toolchain and version alignment:
    - Rust 1.94.0 pinned via rust-toolchain.toml (MSRV in Cargo.toml)
    - Node.js 24+ across all configs, Dockerfiles, and CI workflows
    - pnpm 10.32+ via packageManager field
    - reqwest 0.12 added for Ollama HTTP client

    Infrastructure additions:
    - Dependabot config (5 ecosystems with dependency grouping)
    - Husky pre-commit hooks with lint-staged
    - GitHub Actions: enhanced CI (9 jobs), link checking, release workflow
    - Changelog generation script + CHANGELOG.md
    - Tailwind, PostCSS, ESLint flat configs
    - 3-tier Makefiles reorganized by task group (sindri/mimir pattern)
    - Docker lifecycle targets (15 make targets)
    - Release targets (make release VERSION=x.y.z)
    - Root package.json for husky/lint-staged
    - .lychee.toml for link checking config

- Resolve all frontend typecheck, lint, and formatting issues

- Add missing tsconfig.json for types, api, core, and ui packages
  (typecheck was failing with tsc usage dump)
  - Add DOM lib to api package tsconfig for Request, EventSource,
    localStorage types
  - Wrap emails array in useMemo in EmailClient to stabilize dependency
  - Wrap subscriptions array in useMemo in useInboxCleaner to stabilize
    two dependency arrays
  - Remove stale eslint-disable react/no-danger directive in MessageBubble
  - Export imapSchema in ImapConnect to fix "only used as type" warning
  - Remove stale eslint-disable react-hooks/rules-of-hooks directive in
    useDeferredValue (React 19 always has native useDeferredValue)
  - 5/5 packages typecheck clean, 0 lint errors, 0 lint warnings

- Add root devDependencies so pre-commit hook resolves lint-staged and prettier

The husky pre-commit hook ran `pnpm lint-staged` which invokes
`prettier`, but neither package was installed at the repo root,
causing ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL.

- Resolve clippy type_complexity and wire dead code into call paths

- Extract ConsentRow, AuditRow, EmailExportRow type aliases in privacy.rs
  to satisfy clippy::type_complexity (4 inline tuples → 3 named aliases)
  - Wire IMAP helpers (parse_fetch_response, envelope_to_message) into trait
    impl methods; remove unused http field from ImapProvider
  - Wire checkpoint saves into sync_scheduler.process_batch for crash recovery
  - Rename UndoEntry.batch_id → \_batch_id (stored but not read directly)
  - Apply rate_limit and log_scrubbing middleware to Axum router with
    ConnectInfo<SocketAddr> for IP extraction

- Use std::io::Error::other() per clippy::io_other_error

- Rename FTS5 column email_id to id to match emails table schema

The FTS5 external-content table requires column names matching the source
table. The emails table uses `id` (not `email_id`), causing migration 4
to fail with "no such column: T.email_id" on rebuild.

    Also fixes trigger SQL in search.rs test helpers.

- Clean shutdown on Ctrl+C — kill backend and frontend process group

The dev target backgrounded both servers with & but had no trap,
so Ctrl+C only killed make while child processes kept ports 3000
and 8080 occupied. Now uses trap 'kill 0' INT TERM EXIT + wait
to kill the entire process group on any signal.

- Renumber migrations to unique sequential versions (004-012)

SQLx derives migration version from the numeric prefix and enforces
uniqueness. Three pairs had duplicate version numbers: - 004: accounts + fts5_search → 004, 005 - 005: ingestion_checkpoints + per_user_learning → 006, 007 - 008: gdpr_consent + sync_queue → 010, 011

    Renumbered to 001-012 with no gaps or duplicates.

- SyncStatusIndicator infinite re-render loop

useSyncExternalStore requires getSnapshot to return a referentially
stable value when the underlying data hasn't changed. The previous
implementation called JSON.parse on every invocation, producing a
new array object each time ([] !== []), triggering infinite renders.

    Cache the raw localStorage string and only re-parse when it changes.

- Unwrap consent API response — backend returns { decisions: [] } not raw array

The GET /consent/gdpr endpoint returns GdprConsentListResponse with a
decisions field, but the frontend expected a bare GdprConsent[]. This
caused consents.find is not a function on the Settings page.

- Wire sidebar position setting into layout — Right now moves sidebar

The Appearance settings stored sidebarPosition but the Layout component
never read it. Now uses flex-row-reverse when position is 'right' and
flips the border from border-r to border-l.

- Add missing /chat route to frontend router

The sidebar linked to /chat but the route was never registered in
Router.tsx, showing "Not Found". Added lazy-loaded ChatInterface
route matching the pattern of all other feature routes.

- Vertically center send button with chat input box

Changed flex container from items-end to items-center so the send
button aligns with the middle of the textarea instead of sitting
below it.

- Onboarding health check uses correct endpoint /vectors/health

The onboarding page checked /api/v1/health which doesn't exist,
causing "Backend offline" even when the server is running. The actual
health endpoint is /api/v1/vectors/health (matching the API client).

- Load OAuth secrets from secrets/dev/ as env vars in make dev

The setup wizard writes credentials to secrets/dev/ files, but the
backend reads EMAILIBRIUM_GOOGLE_CLIENT_ID etc. from environment
variables. The dev target now exports all secret files as env vars
before starting the backend.

- Improve Gmail OAuth error handling and setup instructions

Check HTTP status from Gmail profile API before extracting emailAddress,
surfacing the actual Google error (e.g., "Gmail API not enabled") instead
of the opaque "Missing emailAddress in profile" message.

    Also fix incorrect redirect URIs in setup-secrets.sh and add missing
    steps for enabling the Gmail API and configuring Data Access scopes.

- Align FTS5 test schema with migration (email_id → id)

The test helper created the email_fts virtual table with column
"email_id" but the migration (005) and triggers use "id" to match
the source emails table. Also fixed the fts_search query to SELECT
id instead of email_id. Fixes all 5 search test failures.

- Implement email list filters (accountId, category, isRead)

The ListEmailsParams fields were deserialized but not used in the
SQL query, causing dead_code warnings. Now properly builds dynamic
WHERE clause with bind parameters for all filter fields.

- Reorder email routes so /labels and /thread match before /{id}

Static paths must be registered before the catch-all /{id} parameter
route, otherwise "labels" and "thread" are matched as email IDs,
returning 404 "Email not found".

- Clean up Gmail labels, MoveDialog UX, and from-address parsing

Gmail list*folders: - Filter out STARRED, UNREAD, CHAT, YELLOW_STAR, and all superstar
system labels that aren't valid move targets - Filter out CATEGORY*\* auto-labels - Title-case friendly names (Inbox, Sent, Trash, Spam, Drafts)

- Move provider badge before sender name and clean up avatar initials

- Provider badge (G/M/I) now appears before the sender name, not after
  - Avatar initials only use alphanumeric characters (A-Z, 0-9)
  - Special characters like quotes and brackets are stripped
  - Single-word names show one initial instead of two

- Sync starred status from Gmail STARRED label during email fetch

The email sync now checks if the message labels include "STARRED"
and sets is_starred accordingly in the local DB. Previously all
emails were inserted with is_starred=false regardless of Gmail state.

    Also ran a one-time fix on existing emails to set is_starred from
    the labels column (found 7 starred emails out of 2095).

- Normalize Outlook flagged messages to STARRED label

Outlook uses flag.flagStatus="flagged" instead of a STARRED label.
The parse_message now injects "STARRED" into the labels array when
the flag is set, so the sync code's is_starred detection works
consistently across Gmail (native STARRED label) and Outlook
(flagStatus). IMAP would use the \Flagged flag similarly.

- Remove redundant unread indicator dot from email list items

The blue dot unread indicator was removed as the visual treatment for
unread emails is handled elsewhere in the component styling.

- Remove redundant unread indicator dot from email list items

The blue dot unread indicator was removed as the visual treatment for
unread emails is handled elsewhere in the component styling.

- Insights data alignment and UX improvements

- Wire email list density and font size settings to email view

The appearance settings (Compact/Comfortable/Spacious density and Font
Size slider) were stored but never consumed by the email list components.
Now EmailList, GroupedEmailList, and EmailListItem read from the settings
store to apply density-based padding/row-heights and user-configured
font size.

- Make setup scripts compatible with bash 3.2 (macOS default)

Replace bash 4+ features (declare -A, ${var,,}, ${var^^}) with
portable alternatives (temp file, tr) so scripts work on stock macOS.

- Default LLM provider to built-in on first visit via persist migration

Users with older persisted settings had llmProvider set to 'none', which
overrode the new 'builtin' default. Add a version migration (v0→v1) so
the store upgrades existing localStorage data on first load.

- Add missing TopicCluster fields in tests and handle IMAP crate absence

Tests failed because TopicCluster gained `top_terms` and
`representative_email_ids` fields that were not provided in 9 test
initializers, and the IMAP integration test unwrapped an expected error.

- Standardize on Node.js 24 LTS across project

- Dockerfile: revert node:25-alpine back to node:24-alpine (fixes
  corepack removal in Node 25 that broke CI docker build)
  - @types/node: downgrade from ^25.5.0 to ^24.0.0
  - README.md: update prerequisite from Node.js 22.12+ to 24 (LTS)+
  - setup-guide.md: update to Node.js 24, replace corepack install
    with npm install -g pnpm@10

- Exclude .claude/skills from link checker

The skill-builder SKILL.md contains template placeholder links
(docs/API_REFERENCE.md, resources/examples/, related-skill-1, etc.)
that are intentional boilerplate, not broken documentation.

- Patch brace-expansion CVE via pnpm overrides

Override vulnerable transitive dependency brace-expansion: - 1.1.12 -> 1.1.13 (via minimatch@3) - 5.0.4 -> 5.0.5 (via minimatch@10)

- Add submodules: true to all Rust CI jobs

Backend depends on ruvector submodule. Without submodules: true,
actions/checkout@v6 doesn't fetch it, causing cargo to fail with
"No such file or directory" for ruvector-collections.

- Correct GGUF model repo_id and filename for HuggingFace downloads

- qwen3-1.7b: Qwen/Qwen3-1.7B-GGUF has no Q4_K_M, switch to
  unsloth/Qwen3-1.7B-GGUF which does
  - All Qwen3 models: filenames use uppercase (Qwen3-4B-Q4_K_M.gguf
    not qwen3-4b-q4_k_m.gguf)
  - Phi-4: filename is microsoft*Phi-4-mini-instruct-Q4_K_M.gguf
    (includes microsoft* prefix)

  All 11 builtin model URLs verified against HuggingFace API.

- Scope cargo fmt to emailibrium package only

cargo fmt --all includes the ruvector submodule which has its own
formatting conventions. Scope to --package emailibrium to only check
our code.

- Upgrade all GitHub Actions to latest 2026 versions, fix annotations

Actions upgraded: - actions/cache v4 -> v5 (Node.js 24) - actions/download-artifact v4 -> v7 (Node.js 24) - pnpm/action-setup v4 -> v5 (Node.js 24) - docker/setup-buildx-action v3 -> v4 - docker/login-action v3 -> v4 - rustsec/audit-check: add required token input

    Code fixes:
    - built-in-llm-manager.ts: remove unused initial assignment to modelPath
    - syncStore.test.ts: remove unused callCount variable

- Update default model from qwen2.5-0.5b to qwen3-1.7b

The old default model qwen2.5-0.5b-q4km was removed from the model
catalog (models-llm.yaml) during the 2026 leaderboard update but
references remained in config.yaml, config.rs, model_catalog.rs,
useSettings.ts, and AISettings.tsx.

    Updated all defaults to qwen3-1.7b-q4km (smallest model in current
    catalog, sourced from unsloth/Qwen3-1.7B-GGUF).

- Model download progress stuck on "Starting download..."

- Rate limiter too aggressive for local development

The default rate limit of 60 req/min (1/sec) per IP was causing
cascading 429s in development where the React frontend legitimately
makes 3+ concurrent API calls (emails, accounts, ingestion status,
model status).

    - Set RATE_LIMIT_PRESET=development in dev/dev-llm Makefile targets
    - Development mode now uses 200 req/min default for unknown endpoints
      (vs 60 in production), matching the session_status_limit
    - Production behavior unchanged (enable_user_limits=true → 60 default)

- Strip <think> blocks from Qwen 3 classification responses

Qwen 3 models emit <think>...</think> chain-of-thought reasoning
before answering. The classify() method now strips these blocks
before matching against categories. Also increased max_tokens from
50 to 200 to allow room for thinking + answer.

    Works with all models — stripping is a no-op for non-thinking models.

- Strip <think> from chat, inject date/time, clean up proptest warnings

- Inject current date into chat prompt loaded from YAML

The system prompt was being overridden by prompts.yaml via
with_system_prompt(), so the date injection in chat.rs was never
used. Now prepends "The current date and time is: ..." to the
YAML prompt at service construction in main.rs.

    Also updated prompts.yaml with rules 5-6:
    - Don't include internal reasoning/thinking
    - Use the current date for time-relative questions

- Plumb chat_max_tokens from YAML config instead of hardcoding 256

ChatService was hardcoding max_tokens=256 for chat responses, causing
truncated answers. Now reads from tuning.yaml (llm.chat_max_tokens)
with per-model override from models-llm.yaml (tuning.max_tokens).

    - Add configured_max_tokens() to GenerativeModel trait
    - Wire chat_max_tokens through ChatService constructor from YAML config
    - Builtin model resolves per-model tuning at construction time
    - GenerativeRouter delegates to active provider's configured value
    - Bump global default from 256 to 2048
    - Scale per-model limits by context window: 2K-8K based on capability
    - Update related docs

- Plumb RAG config from tuning.yaml instead of hardcoded defaults

RagPipeline now builds RagConfig from tuning.yaml via From<&RagTuning>,
ensuring top_k, min_relevance_score, max_context_tokens, include_body,
and max_body_chars are all driven by config. Fixed format_email_truncated
to respect include_body and max_body_chars settings.

- Include embedding.rs batch Redis changes from P7

The MGET/MSET changes in embed_batch() were part of the P7
optimization but were missed when staging the previous commit.
Replaces 2N individual Redis round-trips with 2 batch calls.

- Eliminate redundant YAML config reloads from disk on every request

model_catalog and switch-model handlers were calling load_yaml_config()
per-request (re-reading 6 YAML files from disk each time). Added
get_model_catalog_with_config() that accepts the pre-loaded YamlConfig
from AppState, avoiding redundant I/O on every API call.

- Update switch-model for builtin-llm feature, fix clippy warning

Updated switch-model handler inside #[cfg(feature = "builtin-llm")]
to use with_params_and_prompts() (with_prompts was removed during
config externalization). Added #[allow(clippy::too_many_arguments)]
to generate_sync() which takes 8 params for plumbed config values.

    Zero clippy warnings in both default and builtin-llm feature modes.

- Add exponential backoff retry for Gmail API rate limits (403)

Gmail's per-user quota (~250 queries/min) causes 403 errors during
bulk email fetching. Now retries rate-limited pages with exponential
backoff (5s, 15s, 45s) and gracefully degrades by processing whatever
pages were successfully fetched if retries are exhausted.

    - gmail.rs: detect 403 "quota"/"rate limit" as ProviderError::RateLimited
    - sync.rs: RetryConfig struct, fetch_page_with_retry() with exp backoff,
      inter-page throttle delay (fetch_page_delay_ms, default 200ms)
    - Graceful degradation: partial results returned instead of total failure
    - Config: sync.fetch_page_delay_ms in app.yaml, error_recovery params
      from tuning.yaml

- Skip trash/spam emails during ingestion embedding (Phase 3)

Added WHERE is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL to
the pending-embedding query so trashed, spam, and soft-deleted emails
are not embedded or classified — saving compute on garbage.

- Increase rate limit capacity to prevent self-throttling during ingestion

Frontend makes rapid API calls during onboarding (SSE progress polling,
email counts, list refreshes) — easily 10+ req/sec. Previous settings
(capacity=60, refill=1/sec) exhausted the bucket in 6 seconds, blocking
the frontend with 429s from our own rate limiter.

    Bumped to capacity=500, refill=20/sec — sufficient for active frontend
    polling during bulk ingestion. Production deployments can override via
    RATE_LIMIT_CAPACITY and RATE_LIMIT_REFILL_PER_SEC env vars.

- Align frontend search API calls with backend vector routes

Frontend was calling POST /api/v1/search (404) instead of the correct
/api/v1/vectors/search/hybrid endpoint. Also fixes field name mismatch
(text→query), findSimilar HTTP method (GET→POST), classify path, and
adds camelCase serialization to backend response structs.

- Email filtering, sidebar counts, search results, and Insights Topics

- Fix spam/trash filters: add is_spam/is_trash to backend ListEmailsParams
  and exclude spam/trash from default inbox queries
  - Fix sidebar counts: show total emailCount (not unreadCount) for
    Categories, Labels, and Subscriptions; invalidate all sidebar queries
    on every mutation
  - Fix label filtering: handle Gmail $-prefixed labels with OR clause in SQL
  - Fix search "Unknown" sender: read from_addr metadata key instead of from
  - Add spam_count/trash_count to /emails/counts endpoint; exclude spam/trash
    from all count queries
  - Fix Insights Topics: replace broken subscription-heuristic grouping with
    dedicated /insights/topics endpoint using AI-assigned categories, real
    subjects, and proper counts
  - Add search result deep-linking: EmailClient reads ?id= param, fetches
    email directly, shows "Viewing search result" banner
  - Add "Show in inbox" scroll-to: virtualizer smooth-scrolls to selected email
  - Add topic card deep-linking: clicking a topic navigates to
    /email?group=cat-{name} with sidebar pre-selected

- Repair 3 failing ingestion tests

- Use db.run_migrations() in test_db() instead of only the initial schema,
  so is_spam/is_trash columns from migration 016 exist in the test DB
  - Use COALESCE(is_trash, 0) / COALESCE(is_spam, 0) in the pending emails
    query for NULL safety

- Adapt to rand 0.10 API renames

- rand::RngCore → rand::Rng (trait renamed in 0.10)
  - fill_bytes() now lives on the renamed Rng trait, no call-site changes needed

- Highlight and scroll to selected email in inbox list from all paths

- Fix CSS specificity bug where read/unread backgrounds overrode selection highlight
  - Add stronger visual indicator (indigo left border + bg) for selected email
  - Add progressive page loading to find and scroll to emails from search deep-links
  - Fix "Back to search" link navigating to Dashboard instead of Search view
  - Add .playwright-mcp/ to .gitignore

- Improve dark mode for sidebar logo and email list backgrounds

Invert sidebar logo SVGs in dark mode so the dark-blue brand colors
become readable against the dark background. Replace the invalid
Tailwind class `dark:bg-gray-850` with `dark:bg-gray-900/50` so read
emails no longer show a light highlight in dark mode.

- Unify category taxonomy, add centroid fallback, persist reclassification

- Align EmailCategory enum, YAML config, and frontend to 12 unified
  categories (added Travel to enum, Alerts/Promotions to YAML/config)
  - Handle empty built-in LLM responses gracefully (debug log instead of
    warn, defer to centroid fallback)
  - Use low-confidence centroid match as final fallback instead of always
    returning Uncategorized when LLM fails
  - Persist category change to DB on user reclassification so the UI
    reflects the change immediately on refetch

- Speed up make lint/format and resolve all lint errors

Constrain yamllint to use find+prune instead of scanning entire tree
(avoids walking node_modules/ and target/). Fix markdownlint fallback
message on lint errors. Add CARGO_INCREMENTAL=1 for faster clippy.
Exclude .agentic-qe/ from markdownlint, add language specifiers to
fenced code blocks, and fix React Hook exhaustive-deps warning.

- Model switch routing and chat template support

The generative router accumulated duplicate providers on model switch,
causing the original startup model (e.g. Qwen3-1.7B) to always win
routing over a newly activated model (e.g. Gemma 4). Fix by replacing
existing providers of the same type on register.

    Add per-model chat template formatting so Gemma models get their native
    <start_of_turn>/<end_of_turn> format instead of ChatML. The template is
    resolved from the chat_template field in models-llm.yaml.

    Also fix test failures: missing EmailMessage fields, sync tests needing
    tokio runtime, flaky pause-phase assertion, and IMAP tests requiring a
    live server.

- Use delta sync instead of local insert after send

Replace manual DB inserts in send/reply/forward handlers with a call
to sync_emails_from_provider(). This avoids duplicate rows when the
background sync also ingests the same sent message from the provider.

- Use individual API calls for grouped bulk actions

Bulk archive/delete in grouped email view called non-existent
/api/v1/actions/bulk-archive and bulk-delete endpoints, causing
silent failures. Now loops through individual working endpoints
(/emails/:id/archive and DELETE /emails/:id) matching the pattern
already used by mark-read and move actions.

- Update storybook test imports for v10 package consolidation

@storybook/test was absorbed into the storybook package in v10;
update all story file imports from '@storybook/test' to 'storybook/test'.

- Restore bash-3.2-compatible pre-commit hook

The previous commit (7d348ea) accidentally landed the interim v1 of
.husky/pre-commit — which used `mapfile` and failed on macOS's default
bash 3.2 with "mapfile: command not found". The working-copy fix
(while-read/$STAGED\_\* strings instead of arrays) was unstaged at
commit time because lint-staged flagged the file as partially staged
and hid the unstaged portion.

    This commits the actual bash-3.2-compatible version.

- Remove unused RED variable in setup-ai.sh

Resolves shellcheck SC2034 violation causing Validate Shell Scripts CI job to fail.

- Pass --tag to git-cliff so unreleased timestamp resolves

Without --tag, the unreleased section has a null timestamp and the
cliff template's `{{ timestamp | date(...) }}` filter errors out with:
Filter `date` received an incorrect type for arg `value`: got `Null`

- Run prettier on CHANGELOG before commit

Pre-commit lint-staged runs prettier --check on staged markdown, which
rejects git-cliff output. Format with prettier --write before staging
so the release commit passes pre-commit.

- Run markdownlint --fix on CHANGELOG before commit

Pre-commit also runs markdownlint-cli2 on staged markdown. Cliff output
can contain asterisk bullets from commit bodies which fail MD004.
Auto-fix after prettier to keep the release commit self-contained.

- Checkout ruvector submodule in ci-gate and docker jobs

Backend tests and Docker backend build need ruvector path crates which
live in a git submodule; without submodules: true the workflow failed
with "unable to update .../ruvector/crates/ruvector-collections".

- Scope cargo fmt to emailibrium package only

cargo fmt --all recurses into the ruvector submodule workspace and
fails on unrelated formatting. Match ci.yml and use
--package emailibrium.

### Dependencies

- Bump frontend deps and fix format-check hanging on 22GB Rust artifacts

Consolidate 4 Dependabot PRs (#24-#27): - turbo 2.8.20 → 2.9.3 - @types/node 24.x → 25.5.0 - typescript-eslint 8.57.2 → 8.58.0 - @tanstack/react-query 5.95.2 → 5.96.0 - @tanstack/react-router 1.168.7 → 1.168.10

    Fix make format-check hanging indefinitely: prettier's fast-glob walker
    enters all directories before filtering via .prettierignore, causing it
    to traverse 22GB of Rust target/ dirs. Replace with find -prune | xargs
    which skips heavy dirs at the OS level (~3.8s vs infinite hang).

- Bulk apply 24 Dependabot PRs (#28-#53)

Backend (Rust): - rand 0.10.0 → 0.10.1 - llama-cpp-4 0.2.26 → 0.2.43 - fastembed 5.13.0 → 5.13.2 - schemars 0.8.22 → 1.2.1 - async-imap 0.10.4 → 0.11.2 (fix: updated read_response API) - async-native-tls 0.5.0 → 0.6.0 - redis 1.1.0 → 1.2.0 - mail-parser 0.9.4 → 0.11.2 - axum-test 19.1.1 → 20.0.0 - tokio 1.50.0 → 1.51.0

    Frontend (npm):
    - @tanstack/react-query + @tanstack/react-router (minor)
    - vite 8.0.3 → 8.0.7
    - turbo 2.9.3 → 2.9.5
    - jsdom 29.0.1 → 29.0.2
    - ky 1.14.3 → 2.0.0 (fix: prefixUrl→prefix, hook signature update)
    - react-hook-form 7.72.0 → 7.72.1
    - postcss 8.5.8 → 8.5.9
    - eslint group updates
    - storybook 10.3.3 → 10.3.5
    - vitest + msw updates

- Apply Dependabot PRs #55/#56 and fix security vulnerabilities

- Bump @tanstack/react-virtual ^3.13.23 → ^3.13.24 (PR #56)
  - Bump eslint-plugin-react-hooks ^7.0.1 → ^7.1.1 (PR #55)
  - PR #52 (rand 0.10.1) was already applied in Cargo.lock
  - Add pnpm override to force lodash >=4.18.1 (fixes CVE alerts #4/#5)
  - Fix MD032 lint errors in AGENTS.md and CLAUDE.md (blank line before lists)
  - Add AGENTS.md (GitNexus config, previously untracked)

- Bulk apply 12 Dependabot PRs + Rust 1.95 sweep (#69)

- deps: bulk apply 12 Dependabot PRs + Rust 1.95 sweep

  Frontend (pnpm monorepo):
  - react 19.2.4 -> 19.2.5 (#67)
  - react-hook-form 7.72.1 -> 7.73.1 (#66)
  - ky 2.0.1 -> 2.0.2 (#65)
  - lucide-react 1.7.0 -> 1.8.0 (#62)
  - nanoid 5.1.7 -> 5.1.9 (#59)
  - typescript 6.0.2 -> 6.0.3 (#64)
  - @types/node 25.5.0 -> 25.6.0 (#63)
  - tailwindcss 4.2.2 -> 4.2.4 (#61)
  - prettier 3.8.1 -> 3.8.3 (#60)
  - vitest 4.1.4 -> 4.1.5 (#58)

  Backend (cargo):
  - rand 0.10.0 -> 0.10.1 (#52)
  - Rust toolchain 1.94 -> 1.95 (Dockerfile #68, rust-toolchain.toml, Cargo.toml MSRV)

  Doc/config sweep for Rust 1.95:
  - README.md, docker-compose.yml, scripts/setup-prereqs.sh
  - docs/{setup,deployment,maintainer}-guide.md
  - docs/plan/{ci-potential-improvements,rust-builtin-llm-implementation}.md

  Historical/changelog references in docs/plan/{inception,implementation,march-2026-audit}.md
  intentionally left as-is (they document past state).

- Upgrade frontend peer deps and fix storybook/eslint conflicts

- Replace @storybook/test@8 with storybook@^10.3.5 (test utils moved into
  the storybook package in v10)
  - Allow eslint-plugin-react with eslint@10 via peerDependencyRules
  - Approve esbuild build scripts via onlyBuiltDependencies
  - Cargo.lock updated via cargo update

### Documentation

- Add OAuth setup guide for Google and Microsoft with citations

Comprehensive guide covering: - Google: credentials, consent screen (Internal vs External), Gmail API
scopes (non-sensitive/sensitive/restricted), Testing vs Production,
verification requirements, CASA security assessment - Microsoft: Entra app registration, Graph API permissions, tenant config,
delegated vs application permissions - Configuration reference mapping secrets/dev/ files to env vars - Troubleshooting: redirect_uri_mismatch, org_internal, missing env vars

    All sections cite official documentation:
    - developers.google.com/identity/protocols/oauth2
    - developers.google.com/workspace/gmail/api/auth/scopes
    - appdefensealliance.dev/casa
    - learn.microsoft.com/en-us/entra/identity-platform

- Add ADR-018 and DDD-008 for email operations domain

- Add test plan for group-by-sender feature

- Add March 2026 audit v2 — AQE quality, security, licensing review

Comprehensive project audit covering implementation quality, test coverage,
documentation, security posture (SAST + cargo-audit + npm audit), and
commercial licensing exposure across all 1,392 dependencies.

- Add UI overview guide with screenshot gallery

- Add research on alternative authentication pathways

### Features

- Initial implementation of Emailibrium — vector-native email intelligence platform

A complete, working implementation spanning 7 development sprints, delivering
a Rust backend and React TypeScript frontend for privacy-first, semantic email
management.

    Backend (Rust — 36 source files, 387 passing tests):
    - Vector store facade with pluggable backends (ADR-003)
    - Embedding pipeline with fallback chain and Moka cache (ADR-002)
    - Hybrid search: FTS5 + HNSW vector search fused via Reciprocal Rank Fusion (ADR-001)
    - VectorCategorizer with EMA centroid classification and LLM fallback (ADR-004)
    - SONA 3-tier adaptive learning: instant feedback, session preferences, long-term consolidation
    - GraphSAGE-inspired clustering with K-means++, silhouette scoring, and stability guardrails (ADR-009)
    - Adaptive quantization: scalar (int8), product (PQ), and binary with auto-tier selection (ADR-007)
    - AES-256-GCM encryption at rest with Argon2id key derivation and zeroize (ADR-008)
    - Multi-asset content extraction: HTML, images, attachments, link classification, tracking detection
    - 6-stage ingestion pipeline with SSE progress streaming and pause/resume
    - Subscription detection with frequency analysis and actionability scoring
    - Ingest-tag-archive pipeline with configurable timing and safety mechanisms (ADR-010)
    - IR evaluation metrics: Recall@K, NDCG, MRR, Precision, macro-F1, confusion matrix, ARI, silhouette
    - SQLite vector backup with encrypted persistence
    - Criterion benchmarks: search scaling (1K-100K), quantization comparison, ingestion throughput
    - Security audit tests: encryption roundtrip, nonce randomness, embedding invertibility, CSP/CORS

    Frontend (React 19 / TypeScript — 154 source files, 0 type errors):
    - Monorepo: Turborepo + pnpm with 4 shared packages (types, api, core, ui)
    - 8 feature modules: command-center, email, inbox-cleaner, insights, rules, settings, onboarding, chat
    - Command palette (cmdk) with debounced semantic search and filter sidebar
    - Inbox Cleaner: 4-step wizard with subscription review, topic cleanup, and batch actions
    - Insights Explorer: 5-tab dashboard with Recharts (pie, line, bar), health score gauge
    - Email client: 3-panel layout with virtual scrolling, thread view, compose/reply
    - Rules Studio: AI suggestions, semantic conditions, rule builder, metrics dashboard
    - Settings: 5 tabs (general, accounts, AI/LLM, privacy, appearance)
    - 10 shared UI components: Button, Card, Badge, Input, Select, Toggle, Spinner, Avatar, EmptyState, Skeleton
    - PWA: service worker, install prompt, offline indicator
    - Accessibility: focus trapping, ARIA live regions, skip-to-content, keyboard shortcuts
    - Error handling: retry with exponential backoff, error boundaries, toast notifications
    - Secure storage: Web Crypto API (AES-GCM) + IndexedDB with non-extractable keys
    - 6 Playwright E2E spec files, 9 Storybook stories

    Architecture & Documentation (30+ documents):
    - Academic research evaluation with 30 citations (RESEARCH.md)
    - 10 Architecture Decision Records (ADR-001 through ADR-010)
    - 6 Domain-Driven Design bounded context documents
    - Primary implementation plan: 8 sprints, 47 tasks, risk register, success metrics
    - OpenAPI 3.0 specification for all 12 API endpoints
    - User guide, deployment guide, maintainer guide, configuration reference, releasing guide
    - Evaluation methodology: search quality, classification accuracy, clustering, performance, inbox zero protocol

- Vector-native email intelligence with tiered AI, semantic search, and guided inbox cleanup

- Rust backend (Axum 0.8) with 16 vector intelligence modules: embedding,
  HNSW search, categorization, clustering, ingestion, learning, insights,
  encryption, quantization, backup, metrics, consent, generative AI, reindexing
  - Tiered AI architecture: ONNX-first local embeddings (fastembed), Ollama and
    cloud providers as opt-in tiers — no data leaves the machine by default
  - Hybrid search engine combining FTS5 + HNSW + Reciprocal Rank Fusion with
    SONA adaptive re-ranking for sub-50ms semantic queries
  - SONA 3-tier adaptive learning system that improves classification and search
    from every user interaction, with A/B experiment control
  - GraphSAGE-inspired topic clustering via K-means++ with GNN-style neighbor
    aggregation for automatic email organization
  - AES-256-GCM encryption at rest with Argon2id KDF, zeroize memory, per-field
    encryption, and key rotation support
  - React 19 frontend with 8 feature modules: command center (Cmd+K palette),
    inbox cleaner (4-step wizard), email client (thread view, compose, reply),
    rules studio (AI-suggested semantic conditions), insights explorer (charts,
    health score), chat interface, onboarding, and settings
  - Multi-account onboarding supporting Gmail OAuth, Outlook, and IMAP with
    configurable archive strategies
  - 3 SQL migrations covering schema, AI consent tracking, and AI metadata
  - 5 backend evaluation test suites: classification accuracy, clustering quality,
    domain adaptation, search quality, and security audit
  - 6 Playwright E2E specs: navigation, email, inbox-cleaner, rules, search,
    onboarding
  - OpenAPI 3.0 specification for all 12 REST endpoints with full request/response
    schemas
  - 13 Architecture Decision Records and 7 Domain-Driven Design bounded context
    documents
  - Full infrastructure: Makefile, Docker Compose (dev + prod), 4 GitHub Actions
    workflows (CI, Docker publish, release, link-check), Dependabot
  - Comprehensive documentation: user guide, deployment guide, configuration
    reference, maintainer guide, releasing guide, and research papers with 30+
    academic citations
  - Security hardening: .gitignore excludes ML caches and session state, secrets
    management via env vars, CSP headers, no hardcoded credentials

- Wire dead-code infrastructure into production paths, fix lint

- Wire SONA Tier 2 session learning into SessionState with pub accessors
  (session_id, age), preference_vector (mean clicked − mean skipped), and
  rerank_boost (gamma × cosine similarity) — all real math, no stubs
  - Add quantize_vector/dequantize_vector dispatchers routing to Scalar,
    Binary, or raw fp32 based on QuantizationTier (ADR-007)
  - Add QuantizedVectorStore wrapper with store_with_quantization,
    get_dequantized, and auto-tier transition on insert
  - Wire EmbeddingStatus into ingestion via EmailEmbeddingRecord with
    Pending→Embedded/Failed state machine and mark_stale for re-embedding
  - Fix JobStatus lifecycle: jobs now create as Pending then transition to
    Running before background task spawns
  - Wire model manifest validation into EmbeddingPipeline::new — dimensions
    checked against get_manifest on init with warning on mismatch
  - Add EmbeddingPipeline::available_models and validated_dimensions public API
  - Add EvaluationReport and generate_evaluation_report aggregating ARI,
    silhouette, macro-F1, accuracy, and detection metrics (Sprint 7 ready)
  - Fix clippy: Iterator::last→next_back, manual %→is_multiple_of,
    type_complexity via type aliases
  - cargo fmt across all touched files
  - 212 tests passing, 0 non-dead-code clippy warnings

- Wire all dead-code infrastructure into API endpoints, eliminate all warnings

- Add GET /vectors/quantization endpoint exercising QuantizedVectorStore,
  quantize_vector/dequantize_vector, ScalarQuantizer, BinaryQuantizer,
  ProductQuantizer, PQCode, simple_kmeans, euclidean_distance_sq
  - Add GET /vectors/models endpoint using EmbeddingPipeline::available_models
    and validated_dimensions with model manifest lookup
  - Add GET /evaluation/report aggregating ARI, silhouette, macro-F1,
    accuracy, and subscription detection via generate_evaluation_report,
    ConfusionMatrix, adjusted_rand_index, detection_metrics, and
    reciprocal_rank_fusion
  - Add GET /learning/session exposing SONA Tier 2 SessionState with
    session_id, age, preference_vector, and rerank_boost
  - Add GET /ingestion/embedding-status exercising EmbeddingStatus and
    EmailEmbeddingRecord lifecycle (pending/embedded/failed/stale)
  - Make SessionState Clone-able, add LearningEngine::get_session()
  - Make euclidean_distance_sq and simple_kmeans pub for API access
  - Fix remaining clippy: is_multiple_of, repeat_n
  - 226 tests passing, 0 warnings, 0 clippy issues

- Redesign AI settings with provider-aware models, API keys, dark mode, and Outlook icon fix

- Redesign AI/LLM settings with ONNX-first defaults: embedding provider
  selector (Built-in ONNX / Ollama / OpenAI) with provider-filtered
  model dropdown showing dimensions and descriptions
  - Change default embedding from text-embedding-3-small (OpenAI) to
    all-MiniLM-L6-v2 (ONNX) matching the privacy-first backend default
  - Add "None (Rule-based)" as default LLM provider matching backend's
    Tier 0 generative config, with provider-specific model lists
  - Add API key inputs for OpenAI and Anthropic with masked password
    fields, show/hide toggle, and "Key saved" confirmation
  - Add Ollama base URL input with live model discovery via GET /api/tags
    showing model name, parameter count, and disk size
  - Add Ollama connection status indicator (connecting/connected/error)
  - Wire dark mode: add useThemeEffect in App.tsx that applies the dark
    class to <html> based on theme setting with OS matchMedia listener
  - Fix Outlook onboarding icon: replace garbled single-path SVG with
    proper multi-layer Outlook brand icon (envelope body, flap, O mark)
  - Add openaiApiKey, anthropicApiKey, ollamaBaseUrl to persisted settings

- Add OAuth configuration scaffolding for Gmail and Outlook (DDD-005)

- Add OAuthConfig, GmailOAuthConfig, OutlookOAuthConfig structs to
  backend config with env-var-based credential loading (never config files)
  - Gmail config: client ID/secret env vars, Gmail API scopes (modify,
    labels, userinfo.email), Google auth/token URLs
  - Outlook config: client ID/secret env vars, tenant ID (common default),
    Microsoft Graph scopes (Mail.ReadWrite, Mail.Send, offline_access,
    User.Read), dynamic auth/token URL builder per tenant
  - Add oauth section to config.yaml with all provider settings
  - Add 4 secrets templates: google_client_id, google_client_secret,
    microsoft_client_id, microsoft_client_secret
  - Mount OAuth secrets in docker-compose.yml backend service
  - Add "Email Provider OAuth Setup" section to deployment guide with
    step-by-step for Google Cloud Console, Microsoft Entra, and IMAP
  - Document all OAuth env vars (EMAILIBRIUM_GOOGLE_CLIENT_ID, etc.)
  - Update secrets/README.md with OAuth credential setup commands
  - 4 new config tests for OAuth defaults and URL construction

- Emailibrium — vector-native email intelligence platform with tiered AI, semantic search, and guided inbox cleanup

Major capabilities authored:

    - Axum REST API with 11 route groups (vectors, ingestion, insights, clustering, learning, interactions, evaluation, backup, AI, consent, auth)
    - Vector embedding engine with ONNX Runtime default, Ollama, and cloud provider tiers (ADR-011, ADR-012)
    - Hybrid search pipeline combining FTS5 full-text + HNSW vector search fused via Reciprocal Rank Fusion (ADR-001)
    - SONA adaptive learning with per-user personalization and EWC catastrophic-forgetting prevention
    - GNN-based email clustering using GraphSAGE on HNSW graphs (ADR-009)
    - Multi-asset content extraction (HTML, attachments, images, tracking pixels, link analysis)
    - Email provider integration with Gmail and Outlook OAuth scaffolding (DDD-005)
    - Ingest-tag-archive pipeline with SSE broadcast and checkpoint resumption (ADR-010)
    - Privacy architecture with embedding encryption, AI consent, and cloud API audit logging (ADR-008)
    - Model lifecycle management with integrity verification, registry, and CLI download (ADR-013)
    - Adaptive quantization for memory-constrained deployments (ADR-007)
    - Generative AI router for summarization, reply drafting, and inbox-zero recommendations
    - React SPA frontend with onboarding flow, AI settings, insights dashboard, and inbox cleaner
    - Vite 8 (Rolldown) build with Tailwind CSS v4 CSS-first configuration
    - SQLite with 9 migrations (schema, consent, metadata, accounts, FTS5, checkpoints, learning, audit, A/B tests)
    - Docker Compose production and dev stacks with security hardening (read-only, no-new-privileges, cap-drop ALL)
    - Redis caching layer with connection management
    - Event bus for domain event propagation across bounded contexts
    - 13 Architecture Decision Records and 7 Domain-Driven Design bounded context documents
    - CI pipeline: Rust format, Clippy, build, frontend lint/typecheck, link checking, Docker build, release workflows
    - Root Makefile with CI, Docker, release, and docs targets
    - Setup scripts for prerequisites, Docker, secrets, AI models, and validation
    - RuVector submodule integration as primary vector database (ADR-003)

- Complete all low-priority audit items (40-54) with vector fallbacks, remote wipe, HDBSCAN, and doc updates

Implements all 10 remaining low-priority items from the March 2026 audit: - ADR-005/DDD-000 doc updates, undocumented features in architecture.md - Makefile help fix, CHANGELOG population, model identifier reconciliation - QdrantVectorStore and SqliteVectorStore fallback backends (ADR-003) - RemoteWipeService with 5 API endpoints (ADR-008) - HDBSCAN alternative clustering algorithm (ADR-009) - Frontend tests in release CI gate, Docker Qdrant profile - Renamed INCEPTION.md/PRIMARY-IMPLEMENTATION-PLAN.md with link fixes

- Implement predecessor recommendations R-01 through R-10 with rule engine, offline sync, security middleware, and GDPR compliance

Backend (Rust — 3,824 new lines, 117 tests): - Rule engine with JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, and 7 REST endpoints (CRUD, validate, test) - IMAP email provider implementing EmailProvider trait with config validation and FETCH response parsing - Gmail incremental sync via history.list API with typed HistoryResponse and concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync via Graph delta query with typed DeltaResponse and pagination - Offline-first sync queue with FIFO dequeue, retry-until-max logic, and four conflict resolution strategies (LastWriterWins, LocalWins, RemoteWins, Manual) - Background sync scheduler with exponential backoff and configurable batch size - Processing checkpoints for crash recovery with save/resume/cleanup and 30-day retention - Token-bucket rate limiter per IP address with configurable burst size, automatic stale bucket cleanup, and 429 + Retry-After responses - HSTS header middleware and log scrubbing (redacts Bearer tokens, API keys, client secrets, passwords) - AI chat service with session management, sliding-window message history, TTL-based cleanup, and SSE streaming endpoints - Hot-reload configuration via mtime polling and Arc<RwLock<Arc<T>>> swap pattern - GDPR consent persistence (consent_decisions + privacy_audit_log tables), data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe with RFC 2369/8058 List-Unsubscribe header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer, and false-positive engagement guards - Property-based tests for content extraction, config parsing, search queries, and scalar quantization round-trips (feature-gated behind proptest) - Three new SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 657 new lines + 1,972 modified):
    - Rules Studio wired to backend: validate-before-save, test-against-sample-email with inline result display
    - Unsubscribe flow: preview dialog with per-sender method/impact, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming via POST-based ReadableStream with stop button, session tracking, and animated cursor indicator
    - GDPR consent settings tab with toggle switches per purpose, consent history table, data export (JSON/CSV), and two-step data erasure confirmation
    - Sync status indicator in Command Center header showing online/offline/pending states with cross-tab reactivity via useSyncExternalStore
    - New shared types (chat, consent, unsubscribe) and API client functions (chatApi, consentApi, unsubscribeApi)

    Documentation (942 new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR Compliance)
    - DDD-007 (Rules bounded context with aggregates, commands, events, domain services)
    - Updated architecture.md with rule engine, sync, security, and privacy sections
    - Predecessor comparison analysis with 12 recommendations (docs/plan/predecessor-recommendations.md)

- Implement predecessor recommendations R-01 through R-10 with rule engine, offline sync, security middleware, and GDPR compliance

Backend (Rust — 3,824 new lines, 117 tests): - Rule engine with JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, and 7 REST endpoints (CRUD, validate, test) - IMAP email provider implementing EmailProvider trait with config validation and FETCH response parsing - Gmail incremental sync via history.list API with typed HistoryResponse and concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync via Graph delta query with typed DeltaResponse and pagination - Offline-first sync queue with FIFO dequeue, retry-until-max logic, and four conflict resolution strategies (LastWriterWins, LocalWins, RemoteWins, Manual) - Background sync scheduler with exponential backoff and configurable batch size - Processing checkpoints for crash recovery with save/resume/cleanup and 30-day retention - Token-bucket rate limiter per IP address with configurable burst size, automatic stale bucket cleanup, and 429 + Retry-After responses - HSTS header middleware and log scrubbing (redacts Bearer tokens, API keys, client secrets, passwords) - AI chat service with session management, sliding-window message history, TTL-based cleanup, and SSE streaming endpoints - Hot-reload configuration via mtime polling and Arc<RwLock<Arc<T>>> swap pattern - GDPR consent persistence (consent_decisions + privacy_audit_log tables), data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe with RFC 2369/8058 List-Unsubscribe header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer, and false-positive engagement guards - Property-based tests for content extraction, config parsing, search queries, and scalar quantization round-trips (feature-gated behind proptest) - Three new SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 657 new lines + 1,972 modified):
    - Rules Studio wired to backend: validate-before-save, test-against-sample-email with inline result display
    - Unsubscribe flow: preview dialog with per-sender method/impact, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming via POST-based ReadableStream with stop button, session tracking, and animated cursor indicator
    - GDPR consent settings tab with toggle switches per purpose, consent history table, data export (JSON/CSV), and two-step data erasure confirmation
    - Sync status indicator in Command Center header showing online/offline/pending states with cross-tab reactivity via useSyncExternalStore
    - New shared types (chat, consent, unsubscribe) and API client functions (chatApi, consentApi, unsubscribeApi)

    Documentation (942 new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR Compliance)
    - DDD-007 (Rules bounded context with aggregates, commands, events, domain services)
    - Updated architecture.md with rule engine, sync, security, and privacy sections
    - Predecessor comparison analysis with 12 recommendations (docs/plan/predecessor-recommendations.md)

- Implement predecessor recommendations R-01 through R-10 with full-stack wiring

Backend (Rust — 3,800+ new lines, 117 tests): - Rule engine: JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, 7 REST endpoints (CRUD, validate, test) - IMAP provider: EmailProvider trait impl with FETCH command builder and response parsing pipeline (TCP connection pending `imap` crate) - Gmail incremental sync: history.list API with typed HistoryResponse, concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync: Graph delta query with typed DeltaResponse and full pagination - Offline-first sync: FIFO queue with retry logic, four conflict resolution strategies (LastWriterWins/LocalWins/RemoteWins/Manual), background scheduler with exponential backoff - Processing checkpoints: crash recovery with save/resume/cleanup, wired into sync scheduler batch processing - Rate limiting: token-bucket per IP, wired as Axum middleware with ConnectInfo<SocketAddr>, 429 + Retry-After responses - Security headers: HSTS middleware, log scrubbing (redacts Bearer tokens, API keys, secrets) wired into router - AI chat: session management with sliding-window history, SSE streaming endpoints - Hot-reload config: mtime polling with Arc<RwLock<Arc<T>>> swap pattern - GDPR compliance: consent_decisions + privacy_audit_log tables, data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe: RFC 2369/8058 header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer - Property-based tests: content extraction, config parsing, search queries, quantization (feature-gated) - Three SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 650+ new lines, 1,900+ modified):
    - Rules Studio: validate-before-save, test-against-sample-email with inline results
    - Unsubscribe flow: preview dialog, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming: POST-based ReadableStream with stop button, animated cursor
    - GDPR consent tab in Settings: toggle switches, consent history, data export/erase
    - Sync status indicator in Command Center: online/offline/pending with cross-tab reactivity
    - Shared types (chat, consent, unsubscribe) and API client functions

    Documentation (940+ new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR)
    - DDD-007 (Rules bounded context)
    - Updated architecture.md and config.yaml
    - Predecessor comparison analysis (docs/plan/predecessor-recommendations.md)

- Wire dead code into live call paths — reduce warnings from 90 to 2

Rules engine wiring: - api/rules.rs create handler calls json_parser::parse_condition() and parse_natural_language() - validate handler calls validate_rules() for cross-rule loop detection - test handler calls process_email() for full pipeline evaluation - list handler uses RuleEngine struct (set_rules/rules methods)

    Middleware wiring:
    - Replaced 5 manual SetResponseHeaderLayer calls with single security_headers_middleware
    - log_scrubbing_middleware now calls scrub_query_params, scrub_headers, scrub_error_message
    - RateLimitConfig::from_env() with RateLimitPreset replaces manual config construction

    Vector services wiring:
    - CloudApiAuditLogger instantiated in VectorService, used by chat endpoint via AuditTimer
    - EvaluationEngine wired with 6 new /evaluation endpoints (A/B tests, IR metrics)
    - GenerativeRouter in VectorService with provider failover for ChatService
    - InferenceSessionManager tracks chat session lifecycle (start/complete/fail)
    - compute_mrr, compute_ndcg, compute_precision_at_k, compute_recall_at_k called from /ir-metrics

    Scaffolded modules (allow(dead_code) retained — 10 files):
    - ewc.rs, user_learning.rs, model_registry.rs, model_download.rs, model_integrity.rs
    - hdbscan.rs, ruvector_store.rs, sync_scheduler.rs, unsubscribe.rs, api/wipe.rs

- Wire provider enable/disable into AI API and consent system — zero warnings

- Add GET /ai/providers listing all registered providers with status
  - Add POST /ai/providers/:provider/disable and /enable for runtime control
  - Consent system auto-toggles cloud providers on GDPR consent changes
  - 3 new tests: list status, disable removes from selection, enable restores
  - Eliminates the last 2 dead_code warnings in the entire codebase

- Complete account settings, email sync, and ingestion pipeline

- Add emails API, sync progress logging, and dashboard UX improvements

- Global sync state via Zustand store for persistent sync UX

Move sync state (syncing, status, error, hasAccounts) from local
useState in CommandCenter into a global Zustand store (syncStore.ts).

    This means:
    - Auto-sync after OAuth uses the same store, so Dashboard shows
      progress immediately when it mounts
    - Navigating away from Dashboard and back preserves sync state —
      banner and animated Sync Now tile remain visible while sync runs
    - Sync cannot be double-triggered (store guards against it)
    - Stats auto-refetch when sync completes

- Add thread endpoint for email thread view

Add GET /api/v1/emails/thread/:thread_id that returns all emails
in a thread sorted by date, with participants and last activity.
Falls back to treating the thread_id as a message ID for single-
message threads (common with Gmail where thread_id == message_id).

- Add email action endpoints and widen inbox pane

- Add tooltips to email actions and move-to-folder dropdown

All hover action icons now have consistent title tooltips (Star,
Archive, Delete, Move to folder). The Move button opens a dropdown
with Archive, Spam, and Trash destinations. Closes on outside click.

- Implement provider-aware move-to-folder/label and star operations

This implements full cross-provider folder/label management:

- Auto-refresh expired OAuth access tokens

get_access_token now checks token_expires_at before returning.
When expired (or within 60s of expiry), it automatically refreshes
using the stored refresh token, persists the new tokens, and returns
the fresh access token. Supports both Gmail and Outlook providers.

    This prevents 401/502 errors that occurred after the 1-hour access
    token lifetime expired.

- Replace inline move dropdown with rich MoveDialog modal

The Move button now opens a proper centered modal dialog with: - Search/filter input for finding folders and labels - System folders section (INBOX, SENT, TRASH, SPAM, etc.) - Custom labels section with divider - "Create new label" input at the bottom - Keyboard support (Escape to close) - Backdrop click to dismiss - Email subject preview in header

    Replaces the clipped inline dropdown that was hard to use.

- Fix inbox count, add infinite scroll, and total emails tile

Email list: - Replace useQuery with useInfiniteQuery for paginated loading - Inbox badge now shows total email count from API (not page count) - Infinite scroll: automatically loads next page when scrolling
near the bottom of the virtualized list - Shows "Loading more..." spinner during page fetches

    Command Center:
    - Add "Emails" tile showing total ingested email count
    - Auto-refreshes every 10 seconds to reflect sync progress
    - Grid now 7 columns to accommodate the new tile

- Add distinct vector icon and emails tile to dashboard

- Disable nav items requiring LLM when none configured

Inbox Cleaner and Chat nav items are grayed out with an "LLM" badge
when llmProvider is "none" in settings. Hovering shows a tooltip
directing to Settings > AI / LLM. Items become clickable when an
LLM provider is configured.

    Also removes unused MailIcon from StatsCards.

- Collapsible sidebar with icons for all nav items

Sidebar now has: - Collapse/expand toggle button (chevron in header) - Lucide icons for each nav item (LayoutDashboard, Mail, Sparkles,
BarChart3, ListChecks, MessageSquare, Cog) - Collapsed mode: 64px wide, icons only with hover tooltips - Expanded mode: 208px wide, icons + labels - Smooth width transition animation - LLM-required items (Inbox Cleaner, Chat) grayed out with "LLM"
badge when no provider configured - All icons visible in both collapsed and expanded states

- Auto-mark-as-read on click with mark-unread option

- Add filter pills for All, Unread, Read, and Starred emails

Pill-based toggle buttons below the inbox header filter emails: - All: shows all emails (default) - Unread: shows only unread emails (isRead=false) - Read: shows only read emails (isRead=true) - Starred: shows only starred emails (isStarred=true)

- Add 2-level group-by-sender view with domain→sender→email hierarchy

Adds a "Grouped" pill to the email list that organizes emails into a
collapsible 2-level hierarchy: domains (A-Z) → senders (A-Z) → emails
(newest-first). Includes RFC 2822 address parsing, subdomain-to-root
normalization, virtual scrolling with variable row heights, infinite
scroll, and bulk hover actions (archive, delete, move, mark read/unread)
at both domain and sender levels. Removes redundant unread indicator dot.

- Implement email body HTML rendering with sandboxed iframe security

Extract body_html from Gmail (recursive MIME traversal) and Outlook
(contentType distinction) APIs, sanitize with ammonia email-specific
whitelist, and render in triple-layer sandboxed iframe (sandbox + CSP +
referrer policy). Removes vulnerable regex-based SanitizedHtml component,
fixes body truncation (200-char preview with subject fallback), and adds
ADR-019/020, DDD-009, research doc, and prioritized implementation plan.

- Add attachment management, fix body_html extraction, async sync

- Replace insights stubs with real database-driven analytics

- Email UI refinements - real counts, dynamic categories, search filter

Show actual email count with comma formatting instead of 999+ cap.
Replace hardcoded sidebar categories/topics/subscriptions with real
data from a new /api/v1/emails/categories endpoint. Add client-side
search filter bar with field selector (from/to/cc/subject/body).

- Add built-in local LLM tier (ADR-021) with quality gate fixes

Built-in Local LLM (Tier 0.5) — Backend - Add BuiltInLlmConfig to GenerativeConfig with model_id, context_size,
gpu_layers, idle_timeout, and cache_dir fields (vectors/config.rs) - Register BuiltIn variant in ProviderType enum with FromStr/Display
(vectors/model_registry.rs) - Change default generative provider from "none" to "builtin" - Add GGUF model-cached detection and AI diagnostic logging at startup
(main.rs) - Remove inline tracing from VectorService match arms; consolidate
provider logging in main.rs (vectors/mod.rs) - Add generative.builtin config block to config.yaml and
config.development.yaml

    Built-in Local LLM — Frontend AI Services (new)
    - built-in-llm-adapter.ts: node-llama-cpp wrapper with classify(),
      chat(), tokenCount(), idle auto-unload
    - built-in-llm-manager.ts: singleton lifecycle manager for model
      load/unload/status
    - generative-router.ts: priority-based provider router
      (builtin > ollama > cloud > rule-based)
    - hardware-detector.ts: GPU/CPU backend detection for Metal, CUDA, Vulkan
    - model-cache.ts: GGUF cache directory management with size reporting
    - model-downloader.ts: streaming model download with progress callbacks
    - model-manifest.ts: GGUF model registry (qwen2.5-0.5b-q4km default)
    - error-recovery.ts: retry logic with exponential backoff and
      circuit-breaker for LLM calls
    - electron-config.ts: Electron/Tauri IPC path resolution
    - useGenerativeRouter.ts: React hook exposing provider/classify/chat

    Built-in Local LLM — Frontend Test Suite (new, 6 files)
    - built-in-llm-adapter.test.ts: adapter load, classify, chat, dispose,
      error paths (469 lines)
    - generative-router.test.ts: priority routing, fallback, provider
      selection
    - hardware-detector.test.ts: backend detection mocking
    - model-management.test.ts: download, cache, manifest lifecycle
    - integration.test.ts: end-to-end adapter+router integration
    - built-in-llm-bench.ts: performance benchmark script (tok/sec,
      first-token latency)

    Built-in Local LLM — Frontend UI Integration
    - AISetup.tsx: add "Built-in LLM" tier card with icon, description, and
      model download prompt in onboarding wizard
    - AISettings.tsx: surface built-in LLM status, model info, and download
      controls in settings panel
    - ModelDownloadProgress.tsx: new component with progress bar, download
      size, and status indicators
    - useSettings.ts: add builtInLlm config fields to settings hook
    - ChatInterface.tsx: show "Powered by built-in AI (local)" badge when
      using builtin provider; add router.chat() integration comment
    - InboxCleaner.tsx: show "Powered by built-in AI" badge; add
      router.classify() integration comment
    - SetupComplete.tsx: display built-in LLM status in setup summary

    Tooling & Scripts
    - scripts/models.ts: new CLI for GGUF model management
      (list/download/delete/info)
    - scripts/setup-ai.sh: add Step 2 for built-in LLM with interactive
      download prompt; renumber Ollama to Step 3, Cloud to Step 4; update
      summary to show built-in LLM cache status
    - Makefile: add download-models and diagnose targets with help entries

    Build & Config
    - vite.config.ts: add node-llama-cpp to optimizeDeps.exclude
    - tailwind.config.ts: new Tailwind v4 config file
    - package.json: add node-llama-cpp dependency
    - pnpm-lock.yaml: lockfile refresh for new dependencies
    - .pnpm-approve-builds.json: new approved build list

    Documentation (new)
    - ADR-021-built-in-local-llm.md: architecture decision for Tier 0.5
    - ADR-021-addendum-rust-backend-llm.md: proposed Rust-side llama-cpp-2
      integration path
    - DDD-006-ai-providers-addendum-built-in-llm.md: domain design for
      built-in LLM bounded context
    - built-in-llm-implementation.md: original frontend implementation plan
    - rust-builtin-llm-implementation.md: Rust backend implementation plan
    - ci-potential-improvements.md: CI pipeline improvement notes

    Documentation (updated)
    - configuration-reference.md: document generative.builtin config block
    - deployment-guide.md: add GGUF model pre-download instructions
    - setup-guide.md: add built-in LLM section to AI provider setup

    Code Quality & Formatting
    - cargo fmt: fix match-arm formatting in vectors/mod.rs
    - clippy: map_or(false, ...) → is_some_and(...) in main.rs
    - prettier: auto-format 26 frontend files (imports, JSX, arrow fns)
    - eslint: fix react-hooks/exhaustive-deps in EmailClient, EmailList,
      GroupedEmailList (extract virtualizer deps); suppress no-console in
      bench script; remove stale eslint-disable directives
    - All quality gates green: cargo fmt, clippy, prettier, eslint, tsc,
      lychee

- Fix ingestion/categorization pipeline and add real-data sidebar navigation

Addresses 7 defects and 6 feature gaps identified during live testing with
2,100 synced Gmail emails where all categories showed "Uncategorized" and
sidebar navigation was non-functional.

    Critical Pipeline Fixes (DEFECT-1 through DEFECT-4):
    - Remove silent MockEmbeddingModel fallback in production; gate behind
      #[cfg(test)] so ONNX failures surface as errors instead of producing
      garbage vectors that poison search and classification
    - Wire categorize_with_fallback() into ingestion pipeline (was calling
      categorize() only, bypassing the entire LLM→rule-based fallback chain
      from ADR-012)
    - Load/seed category centroids on startup from DB; fresh installs get
      10 canonical centroids from embedded category descriptions so vector
      classification works from first sync instead of always returning
      "Uncategorized"
    - Add explicit "builtin" and "none" arms to generative provider match;
      previously fell through to wildcard, silently disabling LLM tier
    - Inject generative model into IngestionPipeline via set_generative()
      so the full centroid→LLM→rules fallback chain is live

    Ingestion & Progress Fixes (DEFECT-5 through DEFECT-7):
    - Remove premature Complete broadcast from outer ingestion handler;
      inner pipeline's own progress channel is now sole source of truth
    - Detect stale embedding status on startup: if vector store is empty
      but DB has emails marked 'embedded', reset to 'pending' for
      re-processing on next sync
    - Add clustering phase threshold check (>= 50 embedded emails) with
      TODO for ClusterEngine wiring

    Gmail Label Resolution:
    - Resolve Gmail label IDs (e.g. Label_356207...) to human-readable
      names by fetching label map during sync and passing through
      parse_message()
    - Add label_map parameter to parse_message() with Arc<HashMap> for
      concurrent access across buffered message fetches

    New Backend API Endpoints:
    - GET /emails/labels/all — cross-account label aggregation with counts,
      system label detection, and Title Case normalization
    - GET /emails/categories/enriched — categories with backend-assigned
      group (subscription vs category) and per-category unread counts
    - GET /emails/counts — accurate total/unread/per-category counts from
      dedicated SQL queries instead of client-side page counting
    - Add ?label=X filter parameter to GET /emails list endpoint with
      parameterized CSV-contains SQL

    Enhanced Rule-Based Classification (Gap 2):
    - Add 15+ sender domain rules: slack/discord→Notification,
      twitter/instagram/tiktok→Social, shopify→Shopping,
      mint/venmo→Finance, mailchimp/sendgrid→Marketing
    - Add sender prefix rules: noreply/no-reply→Notification,
      newsletter/digest→Newsletter
    - Add 8+ keyword rules: security alert→Alerts, your order→Shopping,
      weekly report→Work, you're invited→Personal
    - Add Gmail CATEGORY_* label mapper (category_from_gmail_label)

    Frontend Sidebar Overhaul:
    - Replace hardcoded SUBSCRIPTION_CATEGORIES and TOPIC_CATEGORIES sets
      with dynamic enriched categories from backend group field
    - Add Labels section to EmailSidebar with collapsible panel for
      provider-native labels (works without any AI)
    - Wire getAllLabels(), getEnrichedCategories(), getEmailCounts() APIs
      with React Query (30s stale, 30s refetch for counts)
    - Remove client-side count derivation from paginated email list;
      use server-side counts endpoint for accurate unread badges
    - Add 'label' icon type to SidebarGroup interface

    Command Center & Stats:
    - Reduce stats staleTime from 60s→10s and refetchInterval from
      120s→30s so vector counts appear promptly after embedding
    - Add formatIndexType() to display human-friendly names for vector
      index types (ruvector_hnsw→HNSW, etc.)
    - Add title tooltip and truncation to stat card values

    Serialization & Types:
    - Add #[serde(rename_all = "camelCase")] to HealthStatus and
      VectorStats for consistent frontend/backend field naming
    - Export AggregatedLabel, EnrichedCategory, EmailCounts types from
      @emailibrium/api package
    - Add test-vectors feature flag to Cargo.toml for dev-dependency
      access to MockEmbeddingModel in integration tests

- Add background email polling, incremental sync, and Command Center enhancements

Introduces a background poll scheduler that automatically checks each
connected account for new mail on a configurable interval, using
provider-native incremental sync (Gmail history.list, Outlook delta
queries) to fetch only new/changed messages instead of re-listing the
entire mailbox.

    Background Poll Scheduler (new module):
    - Add poll_scheduler.rs: Tokio background task that ticks every 15s,
      checking which accounts are due for a sync based on their individual
      sync_frequency setting
    - Per-account tracking: last poll time, backoff state, in-progress flag
      to prevent overlapping syncs for the same account
    - Exponential backoff on failures: 30s → 60s → 120s → ... → 10min cap,
      with automatic reset on success
    - Callback-based architecture: sync closure created in main.rs bridges
      the lib-crate scheduler to the binary-crate ingestion code
    - API endpoints: GET /ingestion/poll-status for monitoring, POST
      /ingestion/poll-toggle to enable/disable at runtime
    - Spawned at server startup in main.rs, handle stored in AppState

    Incremental Sync (two-path sync_emails_from_provider):
    - Full sync (onboarding): No history_id in sync_state — paginates all
      messages, then captures Gmail's historyId via profile endpoint for
      future incremental syncs
    - Incremental sync (polling): Has history_id — calls Gmail history.list
      or Outlook delta query to detect only added/updated/deleted messages,
      fetches full details for just those IDs
    - Graceful fallback: if delta fails (expired history_id, Gmail 404),
      clears the marker and falls through to full sync automatically
    - Handles remote deletions: messages deleted on the provider are removed
      from local DB during incremental sync
    - Extracted upsert_email() and fetch_provider_history_id() helpers to
      deduplicate code between full and incremental paths

    Sidebar Filter Fixes:
    - Fix category/subscription filter case mismatch: sidebar IDs used
      toLowerCase() but DB stores Title Case — removed lowercasing so
      filter values match exactly (e.g. "Alerts" not "alerts")
    - Add COLLATE NOCASE to backend category WHERE clause as safety net
    - Add label field to GetEmailsParams TypeScript interface for type
      safety (was passing through via cast but not in the type)

    Inbox Count Corrections:
    - Inbox pill now shows unread count (consistent with category/label
      pills) instead of total count
    - Removed subtitle feature that was added then removed per user feedback

    Command Center Enhancements:
    - Add "Unread" stat tile between Emails and Total Vectors, powered by
      the /emails/counts endpoint instead of polling getEmails({limit:1})
    - Wire Topic Clusters panel to GET /api/v1/clustering/clusters via new
      clusterApi.ts client; add camelCase serde to ClusterListResponse and
      ClusterSummary backend response types
    - Replace static "Recent Activity" placeholder with real "Category
      Breakdown" panel showing enriched categories with bar charts, email
      counts, unread counts, and group badges
    - Add "Last Updated" timestamp in header subtitle using TanStack Query's
      dataUpdatedAt, formatted as "Updated Mar 26, 2:45 PM"
    - Expand stats grid from 7 to 8 columns for the new Unread tile

    Settings & Configuration:
    - Fix sync_frequency values: dropdown was storing minutes (1, 5, 15, 60)
      but backend interprets as seconds — changed to 60, 120, 300, 900, 3600
    - Add "2 min" option to sync frequency dropdown
    - Fix backend validation: replace allowlist [1, 5, 15, 60] with range
      check 60..=86400 seconds to accept the corrected values
    - Add migration 015_sync_frequency_seconds.sql to convert existing
      accounts from minutes to seconds (values < 60 multiplied by 60)

    Code Quality (clippy + formatting):
    - Fix manual_range_contains: use !(60..=86400).contains(&f)
    - Fix needless_borrows_for_generic_args: remove & from .bind() call
    - Fix borrowed_box: &Box<dyn EmailProvider> → &dyn EmailProvider with
      &*provider at call sites
    - Run cargo fmt and prettier on all changed files

- Add RAG pipeline, externalize config to YAML, and enhance built-in LLM

Introduces email-aware RAG chat, moves hardcoded model catalogs and tuning
parameters to external YAML config files, and adds llama.cpp built-in LLM
integration with hardware-aware model selection.

    RAG Pipeline (ADR-022, DDD-010):
    - Add rag.rs with hybrid search retrieval and token-budgeted context injection
    - Integrate RAG context into chat and chat/stream endpoints
    - Auto-generate session IDs when not provided by the client
    - Rewrite SSE protocol to emit {type:"token"/"done"/"error"} events

    Config Externalization:
    - Add config/ directory with 6 YAML files: app, classification, models-llm,
      models-embedding, prompts, tuning
    - Add yaml_config.rs loader with typed structs and serde defaults
    - Add new API endpoints: /model-catalog, /embedding-catalog, /system-info,
      /config/prompts, /config/classification, /config/tuning
    - Remove all hardcoded model arrays from frontend (AISettings, model-manifest)
    - Frontend now fetches model/embedding catalogs dynamically from backend API
    - Add provider validation summary at startup

    Built-in LLM Enhancements:
    - Add generative_builtin.rs with llama-cpp-2 Rust bindings (gated behind
      builtin-llm Cargo feature)
    - Add model_catalog.rs for hardware-aware model recommendation
    - Add --download-model <id> CLI flag for targeted model downloads
    - Add OpenRouter as a generative provider option
    - Update default model to qwen3-1.7b-q4km

    Backend Infrastructure:
    - Add backend/Makefile with build, test, lint, download, and run targets
    - Update root Makefile with new backend integration targets
    - Update Cargo.toml with new dependencies (uuid, llama-cpp-2, etc.)
    - Always run ingestion pipeline on poll (handles incomplete prior runs)
    - Always create ChatService (frontend handles local chat when no backend provider)
    - Load chat session TTL and history limits from YAML tuning config

    Frontend Enhancements:
    - Refactor AISettings to load models from API instead of hardcoded arrays
    - Update ClusterVisualization, StatsCards, and CommandCenter components
    - Update ModelDownloadProgress for API-driven catalog
    - Add branding assets (SVG logo/icon/text) to public/
    - Update Layout, useChat hook, AccountSettings
    - Add ingestion API exports and vector types

- Merge 20 dependabot PRs, fix breaking changes, add 248 tests

Dependency upgrades (20 PRs merged): - CI: actions/checkout v6, setup-node v6, upload-artifact v7,
docker/build-push-action v7, markdownlint-cli2-action v23 - Backend: sha2 0.11, rand 0.9, redis 1.1, hf-hub 0.5,
criterion 0.8, async_zip 0.0.18, axum-test 19.1 - Frontend: TypeScript 6, vitest 4, zod 4, recharts 3,
storybook 10, jsdom 29, eslint 10 - Docker: node 24-alpine -> 25-alpine

    Breaking change fixes:
    - sha2 0.11: use byte slice iteration for hex formatting
    - rand 0.9: rename thread_rng() to rng()
    - TypeScript 6: add ignoreDeprecations, node types, vite-env.d.ts
    - Recharts 3: fix Tooltip formatter type, nullish coalescing
    - Storybook 10: add React.ComponentType to decorator params
    - Vitest 4: migrate all mocks to vi.hoisted() pattern

    New tests (248 total):
    - Backend middleware: rate limiting (15), log scrubbing (15),
      security headers (15)
    - Backend API integration: 31 HTTP tests with in-memory SQLite
    - Backend RAG pipeline (12) + poll scheduler (14)
    - Frontend security: secureStorage AES-GCM (9), error-recovery (13),
      retry utility (10)
    - Frontend hooks: useEmails (16), useConsent (10), useSettings (11),
      useChat (10), syncStore (11)
    - Frontend API client + 5 modules (56)

- Show active model name in chat header for all providers

The chat header now displays the active model name instead of the
generic "Powered by built-in AI (local)": - builtin: shows model ID (e.g., "qwen3-1.7b-q4km") - local: shows "Ollama" - openai: shows "OpenAI" - anthropic: shows "Anthropic" - none: shows "Rule-based"

- Soft-delete, spam/trash management, filter toggle, and UI actions

- Auto-purge expired trash/spam emails (Phase 9)

Hourly background task deletes emails that have been in trash or spam
longer than configured retention period (default 30 days each).

    - Added EmailConfig struct with trash/spam retention_days,
      skip_trash/spam_embedding, default_folder_filter
    - Added email section to AppConfig
    - Spawns tokio task at startup, runs hourly
    - Logs purge counts when emails are removed
    - Config in app.yaml: email.trash_retention_days, spam_retention_days

- Add background label repair and grouped thread links

Add periodic background task to re-resolve unresolved Gmail label IDs
(e.g. Label_356207529) to human-readable names. Refactor ThreadView
link extraction to deduplicate, classify, and group links by type
(actions, documents, social, unsubscribe, tracking).

- Add Sent mail filter pill and fix sidebar count accuracy

Add folder-based filtering across the full stack so users can view sent
mail via a new "Sent" pill in the Email Tools filter bar. Fix sidebar
counts that were showing total email counts as if they were unread —
now correctly displays both total (gray pill) and unread (indigo pill
with envelope icon) counts for categories, subscriptions, and labels.

- Topic clusters dashboard with full sync, re-embed, and incremental reclustering

- Add Topic Clusters visualization with word clouds and representative emails
  - Add clustering status to AI Readiness panel (embedding + clustering progress)
  - Persist clusters to SQLite so they survive server restarts (migration 017)
  - Add configurable min_k/max_k for silhouette K-selection (tuning.yaml)
  - Add LLM warmup inference after model load to prevent cold-start empty responses
  - Fix stale-emails bug: fetch_pending_emails now includes 'stale' status
  - Add selective re-embed modes (all/failed/stale) with auto-trigger ingestion
  - Add clear_all to VectorStoreBackend trait (all 5 backends)
  - Add email_id dedup in batch_insert to prevent duplicate vectors on re-embed
  - Clear clusters from SQLite + memory during full re-embed
  - Add Full Sync mode to Sync Now (clears vectors/clusters + rebuilds everything)
  - Add incremental reclustering: auto-recluster when 50+ new emails accumulate
  - Externalize all timeouts and polling intervals to config/app.yaml
  - Add useAppConfig hook for frontend config consumption via GET /ai/config/app
  - Data-driven polling: faster refresh (5s) when embedding is in progress

- Add pipeline concurrency locks, List-Unsubscribe header support, and clustering performance tuning

Pipeline Concurrency Control: - Add per-account pipeline locking (sync_lock.rs) preventing concurrent sync/ingestion runs - Return HTTP 409 with structured PipelineBusyResponse when pipeline is already active - Add /api/v1/ingestion/lock-status endpoint for frontend pre-flight checks - Integrate lock acquisition/release in both manual ingestion and poll scheduler - Frontend: PipelineBusyError class, syncStore surfaces conflict messages with phase details - InboxCleaner pre-flight lock check with user-facing conflict alert banner

    RFC 2369/8058 List-Unsubscribe Headers:
    - Add list_unsubscribe and list_unsubscribe_post columns to emails table (migration 018)
    - Extract headers from Gmail (payload.headers), Outlook (internetMessageHeaders), and IMAP (FETCH fields)
    - Shared UnsubscribeHeaders utility in email/types.rs with per-provider adapters
    - Persist headers during upsert with COALESCE to preserve existing values
    - Surface headers in SubscriptionInsight for smarter unsubscribe method selection
    - Exclude user's own email addresses from subscription detection (sent mail false positives)

    Unsubscribe API Realignment:
    - Switch from subscriptionIds to UnsubscribeTarget with sender + header fields
    - Add camelCase serde for SubscriptionTarget, UnsubscribeResult, BatchResult, UnsubscribePreview
    - Frontend types: UnsubscribeTarget, updated UnsubscribeResult/Preview to match backend schema
    - SubscriptionsPanel: dual Unsubscribe/Keep buttons per row, forward headers to preview/batch APIs

    Clustering & Ingestion Performance (ADR-021):
    - Pipeline channel buffer: 2 → 32 (aligns with Tokio internal block size)
    - Silhouette sampling: 3000 → 500 (configurable via tuning.yaml)
    - KMeans probe iterations: 50 → 15, final iterations: 100 → 30
    - Precompute global TF-IDF document-frequency table once instead of per-cluster O(K×N×V)
    - All tuning parameters externalized in config/tuning.yaml

    Dashboard Real-Time Pipeline Awareness:
    - StatsCards polls /ingestion/progress for live phase + count display
    - ClusterVisualization uses pipeline phase for accurate empty-state messaging
    - CommandCenter invalidates dashboard queries on sync completion
    - syncStore: phase-aware progress (syncing → embedding → categorizing → clustering → complete)

    Rules Engine Foundation:
    - Add engine.rs (rule evaluation), parser.rs (YAML parsing), validator.rs (JSON Schema validation)
    - Wire list_unsubscribe fields into rule test email context

- Real-time pipeline observability, adaptive polling, and dashboard UX improvements

Backend — backfill progress tracking: - Add BackfillProgress struct and shared state to IngestionPipeline/Handle - New GET /api/v1/ingestion/backfill-progress endpoint for polling LLM backfill state - Track total, categorized, and failed counts during background backfill - Populate total count from pending_backfill query before starting batches - Broadcast accurate total in SSE progress events (was previously 0)

    Frontend — persistent pipeline status banner (CommandCenter):
    - Replace simple sync banner with phase-aware pipeline banner driven by ingestion progress API
    - Show per-phase labels (Fetching, Embedding, Categorizing, Clustering, AI categorization, etc.)
    - Display account name, percentage, counts, ETA, and inline progress bar for embedding phase
    - Green checkmark + auto-dismiss after 10s on completion
    - Separate error banner with dismiss button

    Frontend — adaptive polling and cache tuning:
    - Add 4 new cache config keys: ingestionActiveRefetchIntervalMs, ingestionActiveStaleTimeMs,
      statsRefetchIntervalMs, statsActiveRefetchIntervalMs
    - useStats accepts isActive flag to switch between idle and active polling intervals
    - StatsCards: poll email counts and vector stats faster during active ingestion; invalidate
      queries reactively when ingestion processed/embedded counts change
    - StatsCards: poll backfill progress and invalidate categories-enriched-cc as categorization advances
    - Email counts query uses faster stale/refetch times when pipeline is active
    - Embedding query polls at active rate during pipeline run

    Frontend — AI Readiness and cluster visualization:
    - Show AI Readiness card during active pipeline even when no emails are embedded yet
    - Add backfilling phase to "past embedding" detection (treat as 100% embedded)
    - Add categorization sub-section to AI Readiness driven by backfill progress
    - ClusterVisualization: suppress stale clusters during pre-clustering phases (syncing, embedding,
      categorizing, analyzing) and show progress message instead
    - SyncStatusIndicator: show "Processing" dot during active pipeline or sync; restructure into
      clear priority branches (pipeline → offline → pending → synced)

    Frontend — config and plumbing:
    - Add snakeToCamel transform in useAppConfig to handle backend YAML snake_case keys
    - Deep-merge remote config with defaults so every key has a guaranteed fallback
    - Export getBackfillProgress and BackfillProgressResponse from @emailibrium/api
    - Add backfilling phase label to syncStore phase map
    - Remove delayed "Sync complete!" Zustand message; pipeline banner handles completion now
    - Fix QuickActions "Add Account" href from /settings to /onboarding

- Add email sync progress banner, full IMAP provider, and Outlook $count support

Backend — Email sync progress visibility: - Add `last_progress` cache to `IngestionBroadcast` so the polling endpoint
(`/api/v1/ingestion/progress`) returns sync-phase progress before the
pipeline creates a job — previously the endpoint returned `active: false`
during the entire syncing phase, making the banner invisible - Broadcast per-page progress inside the full-sync pagination loop with
running count of fetched emails and provider's estimated total - Update `ingestion_progress_json` to fall back to broadcast cache when
the pipeline has no active job, covering the syncing phase gap - Clear broadcast cache on pipeline lock release to prevent stale data

    Backend — Outlook provider enhancements:
    - Add `$count=true` and `ConsistencyLevel: eventual` header on first page
      of Graph API message list requests to get total message count
    - Extract `@odata.count` into `result_size_estimate` for determinate
      progress bar during Outlook email sync (was previously `None`)

    Backend — Full IMAP provider implementation:
    - Add `async-imap` (0.10, runtime-tokio), `async-native-tls` (0.5,
      runtime-tokio), and `mail-parser` (0.9) dependencies
    - Complete rewrite of IMAP provider with real async TLS connections,
      replacing the previous stub that returned errors for all operations
    - Implement all 16 EmailProvider trait methods with full parity to
      Gmail/Outlook: authenticate, list_messages, get_message, archive,
      label/remove_labels, create/delete_label, list_labels, list_folders,
      move_message, unarchive, mark_read, star_message
    - UID-based descending pagination with EXISTS count for progress bar
    - RFC822 body parsing via mail-parser for robust MIME/header extraction
    - Per-operation connection lifecycle (connect → operate → logout)
    - 22 unit tests covering config validation and trait method gating

    Frontend — Command Center dashboard:
    - Add syncing-phase progress bar to the pipeline banner — determinate
      with percentage when provider gives total estimate (Gmail/Outlook),
      indeterminate pulsing bar otherwise (fallback)
    - Show "Fetching emails (X% — N / ~total)" status text during sync
    - Fix banner not appearing at all during email ingestion phase

- Improve search with FTS5 query sanitizer, from_name indexing, and sender name matching

Add FTS5 query sanitization to strip stop words and join terms with OR for
natural-language queries. Index from_name in FTS5 so sender display names are
searchable. Improve LIKE fallback to search per-keyword independently. Match
sender filters against from_name in addition to from_addr. Simplify sync
progress banner to indeterminate bar since provider estimates are unreliable.
Speed up hook-handler for pre-bash/post-bash by skipping stdin timeout.

- Add MCP server, tool-calling orchestrator, and enhanced RAG pipeline

Implement ADR-028 (MCP + tool-calling chat) and ADR-029 (enhanced RAG):

    - MCP Streamable HTTP server at /api/v1/mcp with rate limiting and audit logging
    - Chat orchestrator with human-in-the-loop tool confirmation flow
    - Tool-calling provider abstraction for cloud LLMs (OpenAI, Anthropic)
    - Reranker, extractive snippet extraction, and query intent parser
    - Thread grouping for conversation-aware search results
    - Weighted Reciprocal Rank Fusion with per-retriever weights
    - Frontend: ToolCallIndicator, ConfirmationDialog, SSE event types
    - Migrations: FTS5 BM25 weights, thread_key column, MCP tool audit table
    - Consolidate configs/ into config/environments/

- Disable nav items until an email account is onboarded

Only Command Center and Settings remain active before onboarding.
Other nav items are grayed out with a tooltip prompting the user
to connect an account first.

- Add tool_calling capability to model catalog

Surface a `tool_calling` boolean across backend structs, the YAML model
catalog, and the API response so the frontend can show which models
support native function/tool calling.

    - Add `tool_calling` field to ModelInfo, LlmModelEntry, and API mapping
    - Tag every model entry in models-llm.yaml with tool_calling capability
    - Add Gemma 4 (E4B, 26B MoE, 31B), Nemotron 3 Nano (4B, 30B MoE) models
    - Refine tuning params (top_p, repeat_penalty) and fix context sizes
    - Show Tools badge in ChatInterface when active model supports tools
    - Show tool-calling indicator in AISettings model selector
    - Resolve toolCalling state in useGenerativeRouter per provider type

- Persist settings, improve RAG sender matching, upgrade llama-cpp

- Add app_settings table and GET/PUT /settings endpoints so user
  preferences (e.g. selected LLM model) survive server restarts
  - Frontend bidirectional settings sync with debounced auto-push
  - Support multi-word sender names in query parser ("Josh Bob",
    "Mind Valley") with trailing punctuation stripping
  - Inject extracted sender names into RAG search text and add
    space-collapsed variants for fuzzy from_name matching
  - Lower context_sufficiency_threshold from 0.01 to 0.005
  - Upgrade llama-cpp-2 to llama-cpp-4 (0.1→0.2), fix UTF-8 safe
    phrase repetition detection, strip leaked chat template markers
  - Add daily rotating file logs via tracing-appender
  - Promote RAG debug logging to info for better observability

- Add send, reply, and forward email endpoints

Implement the missing backend API routes that the frontend was already
calling (POST /emails/send, /emails/:id/reply, /emails/:id/forward).
Add send_message, reply_to_message, and forward_message to the
EmailProvider trait with implementations for Gmail (RFC 2822 via
messages.send), Outlook (Graph sendMail/reply/forward), and IMAP
(SMTP via lettre). Sent messages are inserted into the local DB
immediately so they appear in the Sent filter without waiting for sync.

- Tag-driven releases with auto-changelog and version pill

- Add cliff.toml for git-cliff changelog generation from conventional commits
  - Add scripts/release.sh: one-command release that bumps all version files
    (backend/Cargo.toml + 5 frontend package.json), refreshes Cargo.lock,
    regenerates CHANGELOG.md, then commits, tags, and pushes with prompts
  - Update Makefile: make release VERSION=x.y.z calls release.sh;
    make changelog regenerates CHANGELOG.md via git-cliff
  - Update release.yml: add version consistency verification step (tag must
    match backend/Cargo.toml and frontend/apps/web/package.json); replace
    custom generate-changelog.sh with orhun/git-cliff-action@v4
  - Add frontend version pill: vite.config.ts injects **APP_VERSION** from
    package.json at build time; Layout.tsx sidebar shows vX.Y.Z pill next
    to Emailibrium logo
  - Pin Docker base to node:24-alpine (LTS); downgrade @types/node to ^24;
    regenerate pnpm-lock.yaml
  - Update docs/releasing.md and docs/maintainer-guide.md with one-command
    release workflow, one-time setup requirements, and rollback procedure

- Move version pill to sidebar footer

Relocate the version pill from beside the brand title to the bottom-left
of the sidebar nav so the header shows only the logo. Hidden when the
sidebar is collapsed.

### Performance

- Optimize ingestion pipeline for 100K+ email onboarding

- Complete ingestion optimization suite (P1, P6, P7, P9)

P1 — Onboarding mode: rules-only classification during bulk sync
with async LLM backfill. Emails classified as "pending_backfill"
are processed in background after initial sync completes, using
buffer_unordered(backfill_concurrency) with configurable batch
size and throttling. Enables 100K inbox onboarding in under 10min.

    P6 — Batch LLM classification: classify N emails in one prompt
    via new classify_batch() trait method on all 4 providers. Batch
    prompt template from prompts.yaml with {{categories}}/{{count}}
    substitution. Per-line parse errors fall back to individual calls.
    New categorize_batch_with_fallback_config() in categorizer.

    P7 — Batch Redis cache: MGET/MSET replace N individual GET/SET
    calls per embedding batch. 2N round-trips reduced to 2 regardless
    of batch size. Graceful degradation on Redis failure.

    P9 — Per-collection RwLock: removed global lock on RuVectorStore.
    Each of 4 vector collections has its own independent RwLock.
    Concurrent writes to different collections no longer serialize.
    AtomicUsize for sidecar write counter.

- Optimize clustering pipeline from O(n²) to O(n log n) for 100k+ scale

- Replace brute-force all-pairs similarity graph with HNSW ANN search
  - Add sampled silhouette score (3k samples) to avoid O(n²) K selection
  - Use inverted indexes for sender/thread edge construction
  - Projected 100k-email clustering: ~10-19 hours → ~3-8 minutes (release)

### Refactors

- Eliminate hardcoded model catalog, source from YAML only

- Retrofit hardcoded config with YAML-driven configuration (wave 1)

Comprehensive plumbing of all 6 YAML config files into the Rust backend,
replacing ~50 hardcoded values across 11 files:

    - LLM params: temperature, top_p, repeat_penalty, classification params
      now sourced from tuning.yaml with per-model overrides from models-llm.yaml
      via GenerationParams resolution (all 4 providers: Ollama, Cloud, BuiltIn,
      OpenRouter)
    - Classification: domain rules, keyword rules from classification.yaml;
      prompts from prompts.yaml with {{categories}}/{{email_text}} substitution
    - Ingestion: embedding_batch_size, min_cluster_emails from tuning.yaml
    - Clustering: tfidf_max_terms, representative_emails from tuning.yaml
    - Error recovery: max_retries, retry_delay_ms with retry loop
    - Repetition detection: token_window, thresholds, phrase checks
    - Poll scheduler: intervals from app.yaml sync config
    - RAG pipeline: built from tuning.yaml via From<&RagTuning>
    - Hardware: OS overhead from app.yaml for model catalog sizing
    - New /api/v1/ai/config/app endpoint serves AppConfig to frontend

- Retrofit remaining config gaps with YAML-driven configuration (wave 2)

Memory management: idle model unloading with RAM-aware timeout selection
(idle_timeout_secs / low_ram_idle_timeout_secs), memory safety margin
applied to model fit checks, background monitoring task with configurable
interval and memory warning threshold.

    Model catalog: surface all metadata fields (family, rag_capable, notes,
    cost, chat_template, default_for_ram_mb) in API responses. Rewrote
    recommend_model() to use default_for_ram_mb for RAM-based auto-selection.
    Embedding catalog: added provider description, fastembed_variant,
    fastembed_quantized, is_default, ollama_tag.

    App config: paths from app.yaml override Figment compile-time defaults,
    security (rate_limit_capacity/refill, hsts_max_age) with env>yaml>default
    fallback chain, sync completion config accessible via PollSchedulerHandle,
    ingestion start timeout from network config.

### Security

- Remove email embeddings data and gitignore backend/data/

backend/data/vectors/email_text/documents.json contained 10.1 MB of
real email embedding vectors with email IDs — runtime data that should
never be checked in. Added backend/data/ to .gitignore.

    History rewrite follows to purge the blob entirely.

## [v0.1.0] - 2026-04-23

### Bug Fixes

- Wire all services, remove runtime stubs, harden infrastructure

Complete service wiring — all 10 modules now initialized in VectorService
and exposed via API endpoints. Removes all runtime stubs, mock data, and
TODO/FIXME comments from production code.

    Service wiring:
    - VectorService initializes all 10 services: embedding, store, categorizer,
      hybrid_search, cluster_engine, learning_engine, interaction_tracker,
      insight_engine, backup_service, quantization_engine, ingestion_pipeline
    - Backup restore on startup when enabled
    - 5 new API route files: clustering, learning, interactions, evaluation, backup
    - vectors.rs upgraded with hybrid search endpoint + interaction tracking
    - ingestion.rs wired to real IngestionPipeline (was stub)

    Runtime stub removal (backend):
    - OllamaEmbeddingModel: real reqwest HTTP client replacing "not yet integrated" stub
    - MockEmbeddingModel: only used when config.provider="mock" (no silent fallback)
    - Content extractors: honest ADR-006 references replacing TODO comments
    - Zero TODO/FIXME/stub remaining in non-test backend code

    Runtime stub removal (frontend):
    - RecentActivity: empty state replacing hardcoded mock events
    - ClusterVisualization: empty state replacing mock cluster data
    - OnboardingFlow: real OAuth callback replacing mockAccount creation
    - EmailClient: reclassify/move handlers wired to learning feedback API
    - SubscriptionsPanel: bulk unsubscribe wired to real API
    - TopicsPanel: cluster navigation wired
    - New learningApi.ts in @emailibrium/api package

    Toolchain and version alignment:
    - Rust 1.94.0 pinned via rust-toolchain.toml (MSRV in Cargo.toml)
    - Node.js 24+ across all configs, Dockerfiles, and CI workflows
    - pnpm 10.32+ via packageManager field
    - reqwest 0.12 added for Ollama HTTP client

    Infrastructure additions:
    - Dependabot config (5 ecosystems with dependency grouping)
    - Husky pre-commit hooks with lint-staged
    - GitHub Actions: enhanced CI (9 jobs), link checking, release workflow
    - Changelog generation script + CHANGELOG.md
    - Tailwind, PostCSS, ESLint flat configs
    - 3-tier Makefiles reorganized by task group (sindri/mimir pattern)
    - Docker lifecycle targets (15 make targets)
    - Release targets (make release VERSION=x.y.z)
    - Root package.json for husky/lint-staged
    - .lychee.toml for link checking config

- Resolve all frontend typecheck, lint, and formatting issues

- Add missing tsconfig.json for types, api, core, and ui packages
  (typecheck was failing with tsc usage dump)
  - Add DOM lib to api package tsconfig for Request, EventSource,
    localStorage types
  - Wrap emails array in useMemo in EmailClient to stabilize dependency
  - Wrap subscriptions array in useMemo in useInboxCleaner to stabilize
    two dependency arrays
  - Remove stale eslint-disable react/no-danger directive in MessageBubble
  - Export imapSchema in ImapConnect to fix "only used as type" warning
  - Remove stale eslint-disable react-hooks/rules-of-hooks directive in
    useDeferredValue (React 19 always has native useDeferredValue)
  - 5/5 packages typecheck clean, 0 lint errors, 0 lint warnings

- Add root devDependencies so pre-commit hook resolves lint-staged and prettier

The husky pre-commit hook ran `pnpm lint-staged` which invokes
`prettier`, but neither package was installed at the repo root,
causing ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL.

- Resolve clippy type_complexity and wire dead code into call paths

- Extract ConsentRow, AuditRow, EmailExportRow type aliases in privacy.rs
  to satisfy clippy::type_complexity (4 inline tuples → 3 named aliases)
  - Wire IMAP helpers (parse_fetch_response, envelope_to_message) into trait
    impl methods; remove unused http field from ImapProvider
  - Wire checkpoint saves into sync_scheduler.process_batch for crash recovery
  - Rename UndoEntry.batch_id → \_batch_id (stored but not read directly)
  - Apply rate_limit and log_scrubbing middleware to Axum router with
    ConnectInfo<SocketAddr> for IP extraction

- Use std::io::Error::other() per clippy::io_other_error

- Rename FTS5 column email_id to id to match emails table schema

The FTS5 external-content table requires column names matching the source
table. The emails table uses `id` (not `email_id`), causing migration 4
to fail with "no such column: T.email_id" on rebuild.

    Also fixes trigger SQL in search.rs test helpers.

- Clean shutdown on Ctrl+C — kill backend and frontend process group

The dev target backgrounded both servers with & but had no trap,
so Ctrl+C only killed make while child processes kept ports 3000
and 8080 occupied. Now uses trap 'kill 0' INT TERM EXIT + wait
to kill the entire process group on any signal.

- Renumber migrations to unique sequential versions (004-012)

SQLx derives migration version from the numeric prefix and enforces
uniqueness. Three pairs had duplicate version numbers: - 004: accounts + fts5_search → 004, 005 - 005: ingestion_checkpoints + per_user_learning → 006, 007 - 008: gdpr_consent + sync_queue → 010, 011

    Renumbered to 001-012 with no gaps or duplicates.

- SyncStatusIndicator infinite re-render loop

useSyncExternalStore requires getSnapshot to return a referentially
stable value when the underlying data hasn't changed. The previous
implementation called JSON.parse on every invocation, producing a
new array object each time ([] !== []), triggering infinite renders.

    Cache the raw localStorage string and only re-parse when it changes.

- Unwrap consent API response — backend returns { decisions: [] } not raw array

The GET /consent/gdpr endpoint returns GdprConsentListResponse with a
decisions field, but the frontend expected a bare GdprConsent[]. This
caused consents.find is not a function on the Settings page.

- Wire sidebar position setting into layout — Right now moves sidebar

The Appearance settings stored sidebarPosition but the Layout component
never read it. Now uses flex-row-reverse when position is 'right' and
flips the border from border-r to border-l.

- Add missing /chat route to frontend router

The sidebar linked to /chat but the route was never registered in
Router.tsx, showing "Not Found". Added lazy-loaded ChatInterface
route matching the pattern of all other feature routes.

- Vertically center send button with chat input box

Changed flex container from items-end to items-center so the send
button aligns with the middle of the textarea instead of sitting
below it.

- Onboarding health check uses correct endpoint /vectors/health

The onboarding page checked /api/v1/health which doesn't exist,
causing "Backend offline" even when the server is running. The actual
health endpoint is /api/v1/vectors/health (matching the API client).

- Load OAuth secrets from secrets/dev/ as env vars in make dev

The setup wizard writes credentials to secrets/dev/ files, but the
backend reads EMAILIBRIUM_GOOGLE_CLIENT_ID etc. from environment
variables. The dev target now exports all secret files as env vars
before starting the backend.

- Improve Gmail OAuth error handling and setup instructions

Check HTTP status from Gmail profile API before extracting emailAddress,
surfacing the actual Google error (e.g., "Gmail API not enabled") instead
of the opaque "Missing emailAddress in profile" message.

    Also fix incorrect redirect URIs in setup-secrets.sh and add missing
    steps for enabling the Gmail API and configuring Data Access scopes.

- Align FTS5 test schema with migration (email_id → id)

The test helper created the email_fts virtual table with column
"email_id" but the migration (005) and triggers use "id" to match
the source emails table. Also fixed the fts_search query to SELECT
id instead of email_id. Fixes all 5 search test failures.

- Implement email list filters (accountId, category, isRead)

The ListEmailsParams fields were deserialized but not used in the
SQL query, causing dead_code warnings. Now properly builds dynamic
WHERE clause with bind parameters for all filter fields.

- Reorder email routes so /labels and /thread match before /{id}

Static paths must be registered before the catch-all /{id} parameter
route, otherwise "labels" and "thread" are matched as email IDs,
returning 404 "Email not found".

- Clean up Gmail labels, MoveDialog UX, and from-address parsing

Gmail list*folders: - Filter out STARRED, UNREAD, CHAT, YELLOW_STAR, and all superstar
system labels that aren't valid move targets - Filter out CATEGORY*\* auto-labels - Title-case friendly names (Inbox, Sent, Trash, Spam, Drafts)

- Move provider badge before sender name and clean up avatar initials

- Provider badge (G/M/I) now appears before the sender name, not after
  - Avatar initials only use alphanumeric characters (A-Z, 0-9)
  - Special characters like quotes and brackets are stripped
  - Single-word names show one initial instead of two

- Sync starred status from Gmail STARRED label during email fetch

The email sync now checks if the message labels include "STARRED"
and sets is_starred accordingly in the local DB. Previously all
emails were inserted with is_starred=false regardless of Gmail state.

    Also ran a one-time fix on existing emails to set is_starred from
    the labels column (found 7 starred emails out of 2095).

- Normalize Outlook flagged messages to STARRED label

Outlook uses flag.flagStatus="flagged" instead of a STARRED label.
The parse_message now injects "STARRED" into the labels array when
the flag is set, so the sync code's is_starred detection works
consistently across Gmail (native STARRED label) and Outlook
(flagStatus). IMAP would use the \Flagged flag similarly.

- Remove redundant unread indicator dot from email list items

The blue dot unread indicator was removed as the visual treatment for
unread emails is handled elsewhere in the component styling.

- Remove redundant unread indicator dot from email list items

The blue dot unread indicator was removed as the visual treatment for
unread emails is handled elsewhere in the component styling.

- Insights data alignment and UX improvements

- Wire email list density and font size settings to email view

The appearance settings (Compact/Comfortable/Spacious density and Font
Size slider) were stored but never consumed by the email list components.
Now EmailList, GroupedEmailList, and EmailListItem read from the settings
store to apply density-based padding/row-heights and user-configured
font size.

- Make setup scripts compatible with bash 3.2 (macOS default)

Replace bash 4+ features (declare -A, ${var,,}, ${var^^}) with
portable alternatives (temp file, tr) so scripts work on stock macOS.

- Default LLM provider to built-in on first visit via persist migration

Users with older persisted settings had llmProvider set to 'none', which
overrode the new 'builtin' default. Add a version migration (v0→v1) so
the store upgrades existing localStorage data on first load.

- Add missing TopicCluster fields in tests and handle IMAP crate absence

Tests failed because TopicCluster gained `top_terms` and
`representative_email_ids` fields that were not provided in 9 test
initializers, and the IMAP integration test unwrapped an expected error.

- Standardize on Node.js 24 LTS across project

- Dockerfile: revert node:25-alpine back to node:24-alpine (fixes
  corepack removal in Node 25 that broke CI docker build)
  - @types/node: downgrade from ^25.5.0 to ^24.0.0
  - README.md: update prerequisite from Node.js 22.12+ to 24 (LTS)+
  - setup-guide.md: update to Node.js 24, replace corepack install
    with npm install -g pnpm@10

- Exclude .claude/skills from link checker

The skill-builder SKILL.md contains template placeholder links
(docs/API_REFERENCE.md, resources/examples/, related-skill-1, etc.)
that are intentional boilerplate, not broken documentation.

- Patch brace-expansion CVE via pnpm overrides

Override vulnerable transitive dependency brace-expansion: - 1.1.12 -> 1.1.13 (via minimatch@3) - 5.0.4 -> 5.0.5 (via minimatch@10)

- Add submodules: true to all Rust CI jobs

Backend depends on ruvector submodule. Without submodules: true,
actions/checkout@v6 doesn't fetch it, causing cargo to fail with
"No such file or directory" for ruvector-collections.

- Correct GGUF model repo_id and filename for HuggingFace downloads

- qwen3-1.7b: Qwen/Qwen3-1.7B-GGUF has no Q4_K_M, switch to
  unsloth/Qwen3-1.7B-GGUF which does
  - All Qwen3 models: filenames use uppercase (Qwen3-4B-Q4_K_M.gguf
    not qwen3-4b-q4_k_m.gguf)
  - Phi-4: filename is microsoft*Phi-4-mini-instruct-Q4_K_M.gguf
    (includes microsoft* prefix)

  All 11 builtin model URLs verified against HuggingFace API.

- Scope cargo fmt to emailibrium package only

cargo fmt --all includes the ruvector submodule which has its own
formatting conventions. Scope to --package emailibrium to only check
our code.

- Upgrade all GitHub Actions to latest 2026 versions, fix annotations

Actions upgraded: - actions/cache v4 -> v5 (Node.js 24) - actions/download-artifact v4 -> v7 (Node.js 24) - pnpm/action-setup v4 -> v5 (Node.js 24) - docker/setup-buildx-action v3 -> v4 - docker/login-action v3 -> v4 - rustsec/audit-check: add required token input

    Code fixes:
    - built-in-llm-manager.ts: remove unused initial assignment to modelPath
    - syncStore.test.ts: remove unused callCount variable

- Update default model from qwen2.5-0.5b to qwen3-1.7b

The old default model qwen2.5-0.5b-q4km was removed from the model
catalog (models-llm.yaml) during the 2026 leaderboard update but
references remained in config.yaml, config.rs, model_catalog.rs,
useSettings.ts, and AISettings.tsx.

    Updated all defaults to qwen3-1.7b-q4km (smallest model in current
    catalog, sourced from unsloth/Qwen3-1.7B-GGUF).

- Model download progress stuck on "Starting download..."

- Rate limiter too aggressive for local development

The default rate limit of 60 req/min (1/sec) per IP was causing
cascading 429s in development where the React frontend legitimately
makes 3+ concurrent API calls (emails, accounts, ingestion status,
model status).

    - Set RATE_LIMIT_PRESET=development in dev/dev-llm Makefile targets
    - Development mode now uses 200 req/min default for unknown endpoints
      (vs 60 in production), matching the session_status_limit
    - Production behavior unchanged (enable_user_limits=true → 60 default)

- Strip <think> blocks from Qwen 3 classification responses

Qwen 3 models emit <think>...</think> chain-of-thought reasoning
before answering. The classify() method now strips these blocks
before matching against categories. Also increased max_tokens from
50 to 200 to allow room for thinking + answer.

    Works with all models — stripping is a no-op for non-thinking models.

- Strip <think> from chat, inject date/time, clean up proptest warnings

- Inject current date into chat prompt loaded from YAML

The system prompt was being overridden by prompts.yaml via
with_system_prompt(), so the date injection in chat.rs was never
used. Now prepends "The current date and time is: ..." to the
YAML prompt at service construction in main.rs.

    Also updated prompts.yaml with rules 5-6:
    - Don't include internal reasoning/thinking
    - Use the current date for time-relative questions

- Plumb chat_max_tokens from YAML config instead of hardcoding 256

ChatService was hardcoding max_tokens=256 for chat responses, causing
truncated answers. Now reads from tuning.yaml (llm.chat_max_tokens)
with per-model override from models-llm.yaml (tuning.max_tokens).

    - Add configured_max_tokens() to GenerativeModel trait
    - Wire chat_max_tokens through ChatService constructor from YAML config
    - Builtin model resolves per-model tuning at construction time
    - GenerativeRouter delegates to active provider's configured value
    - Bump global default from 256 to 2048
    - Scale per-model limits by context window: 2K-8K based on capability
    - Update related docs

- Plumb RAG config from tuning.yaml instead of hardcoded defaults

RagPipeline now builds RagConfig from tuning.yaml via From<&RagTuning>,
ensuring top_k, min_relevance_score, max_context_tokens, include_body,
and max_body_chars are all driven by config. Fixed format_email_truncated
to respect include_body and max_body_chars settings.

- Include embedding.rs batch Redis changes from P7

The MGET/MSET changes in embed_batch() were part of the P7
optimization but were missed when staging the previous commit.
Replaces 2N individual Redis round-trips with 2 batch calls.

- Eliminate redundant YAML config reloads from disk on every request

model_catalog and switch-model handlers were calling load_yaml_config()
per-request (re-reading 6 YAML files from disk each time). Added
get_model_catalog_with_config() that accepts the pre-loaded YamlConfig
from AppState, avoiding redundant I/O on every API call.

- Update switch-model for builtin-llm feature, fix clippy warning

Updated switch-model handler inside #[cfg(feature = "builtin-llm")]
to use with_params_and_prompts() (with_prompts was removed during
config externalization). Added #[allow(clippy::too_many_arguments)]
to generate_sync() which takes 8 params for plumbed config values.

    Zero clippy warnings in both default and builtin-llm feature modes.

- Add exponential backoff retry for Gmail API rate limits (403)

Gmail's per-user quota (~250 queries/min) causes 403 errors during
bulk email fetching. Now retries rate-limited pages with exponential
backoff (5s, 15s, 45s) and gracefully degrades by processing whatever
pages were successfully fetched if retries are exhausted.

    - gmail.rs: detect 403 "quota"/"rate limit" as ProviderError::RateLimited
    - sync.rs: RetryConfig struct, fetch_page_with_retry() with exp backoff,
      inter-page throttle delay (fetch_page_delay_ms, default 200ms)
    - Graceful degradation: partial results returned instead of total failure
    - Config: sync.fetch_page_delay_ms in app.yaml, error_recovery params
      from tuning.yaml

- Skip trash/spam emails during ingestion embedding (Phase 3)

Added WHERE is_trash = 0 AND is_spam = 0 AND deleted_at IS NULL to
the pending-embedding query so trashed, spam, and soft-deleted emails
are not embedded or classified — saving compute on garbage.

- Increase rate limit capacity to prevent self-throttling during ingestion

Frontend makes rapid API calls during onboarding (SSE progress polling,
email counts, list refreshes) — easily 10+ req/sec. Previous settings
(capacity=60, refill=1/sec) exhausted the bucket in 6 seconds, blocking
the frontend with 429s from our own rate limiter.

    Bumped to capacity=500, refill=20/sec — sufficient for active frontend
    polling during bulk ingestion. Production deployments can override via
    RATE_LIMIT_CAPACITY and RATE_LIMIT_REFILL_PER_SEC env vars.

- Align frontend search API calls with backend vector routes

Frontend was calling POST /api/v1/search (404) instead of the correct
/api/v1/vectors/search/hybrid endpoint. Also fixes field name mismatch
(text→query), findSimilar HTTP method (GET→POST), classify path, and
adds camelCase serialization to backend response structs.

- Email filtering, sidebar counts, search results, and Insights Topics

- Fix spam/trash filters: add is_spam/is_trash to backend ListEmailsParams
  and exclude spam/trash from default inbox queries
  - Fix sidebar counts: show total emailCount (not unreadCount) for
    Categories, Labels, and Subscriptions; invalidate all sidebar queries
    on every mutation
  - Fix label filtering: handle Gmail $-prefixed labels with OR clause in SQL
  - Fix search "Unknown" sender: read from_addr metadata key instead of from
  - Add spam_count/trash_count to /emails/counts endpoint; exclude spam/trash
    from all count queries
  - Fix Insights Topics: replace broken subscription-heuristic grouping with
    dedicated /insights/topics endpoint using AI-assigned categories, real
    subjects, and proper counts
  - Add search result deep-linking: EmailClient reads ?id= param, fetches
    email directly, shows "Viewing search result" banner
  - Add "Show in inbox" scroll-to: virtualizer smooth-scrolls to selected email
  - Add topic card deep-linking: clicking a topic navigates to
    /email?group=cat-{name} with sidebar pre-selected

- Repair 3 failing ingestion tests

- Use db.run_migrations() in test_db() instead of only the initial schema,
  so is_spam/is_trash columns from migration 016 exist in the test DB
  - Use COALESCE(is_trash, 0) / COALESCE(is_spam, 0) in the pending emails
    query for NULL safety

- Adapt to rand 0.10 API renames

- rand::RngCore → rand::Rng (trait renamed in 0.10)
  - fill_bytes() now lives on the renamed Rng trait, no call-site changes needed

- Highlight and scroll to selected email in inbox list from all paths

- Fix CSS specificity bug where read/unread backgrounds overrode selection highlight
  - Add stronger visual indicator (indigo left border + bg) for selected email
  - Add progressive page loading to find and scroll to emails from search deep-links
  - Fix "Back to search" link navigating to Dashboard instead of Search view
  - Add .playwright-mcp/ to .gitignore

- Improve dark mode for sidebar logo and email list backgrounds

Invert sidebar logo SVGs in dark mode so the dark-blue brand colors
become readable against the dark background. Replace the invalid
Tailwind class `dark:bg-gray-850` with `dark:bg-gray-900/50` so read
emails no longer show a light highlight in dark mode.

- Unify category taxonomy, add centroid fallback, persist reclassification

- Align EmailCategory enum, YAML config, and frontend to 12 unified
  categories (added Travel to enum, Alerts/Promotions to YAML/config)
  - Handle empty built-in LLM responses gracefully (debug log instead of
    warn, defer to centroid fallback)
  - Use low-confidence centroid match as final fallback instead of always
    returning Uncategorized when LLM fails
  - Persist category change to DB on user reclassification so the UI
    reflects the change immediately on refetch

- Speed up make lint/format and resolve all lint errors

Constrain yamllint to use find+prune instead of scanning entire tree
(avoids walking node_modules/ and target/). Fix markdownlint fallback
message on lint errors. Add CARGO_INCREMENTAL=1 for faster clippy.
Exclude .agentic-qe/ from markdownlint, add language specifiers to
fenced code blocks, and fix React Hook exhaustive-deps warning.

- Model switch routing and chat template support

The generative router accumulated duplicate providers on model switch,
causing the original startup model (e.g. Qwen3-1.7B) to always win
routing over a newly activated model (e.g. Gemma 4). Fix by replacing
existing providers of the same type on register.

    Add per-model chat template formatting so Gemma models get their native
    <start_of_turn>/<end_of_turn> format instead of ChatML. The template is
    resolved from the chat_template field in models-llm.yaml.

    Also fix test failures: missing EmailMessage fields, sync tests needing
    tokio runtime, flaky pause-phase assertion, and IMAP tests requiring a
    live server.

- Use delta sync instead of local insert after send

Replace manual DB inserts in send/reply/forward handlers with a call
to sync_emails_from_provider(). This avoids duplicate rows when the
background sync also ingests the same sent message from the provider.

- Use individual API calls for grouped bulk actions

Bulk archive/delete in grouped email view called non-existent
/api/v1/actions/bulk-archive and bulk-delete endpoints, causing
silent failures. Now loops through individual working endpoints
(/emails/:id/archive and DELETE /emails/:id) matching the pattern
already used by mark-read and move actions.

- Update storybook test imports for v10 package consolidation

@storybook/test was absorbed into the storybook package in v10;
update all story file imports from '@storybook/test' to 'storybook/test'.

- Restore bash-3.2-compatible pre-commit hook

The previous commit (7d348ea) accidentally landed the interim v1 of
.husky/pre-commit — which used `mapfile` and failed on macOS's default
bash 3.2 with "mapfile: command not found". The working-copy fix
(while-read/$STAGED\_\* strings instead of arrays) was unstaged at
commit time because lint-staged flagged the file as partially staged
and hid the unstaged portion.

    This commits the actual bash-3.2-compatible version.

- Remove unused RED variable in setup-ai.sh

Resolves shellcheck SC2034 violation causing Validate Shell Scripts CI job to fail.

- Pass --tag to git-cliff so unreleased timestamp resolves

Without --tag, the unreleased section has a null timestamp and the
cliff template's `{{ timestamp | date(...) }}` filter errors out with:
Filter `date` received an incorrect type for arg `value`: got `Null`

- Run prettier on CHANGELOG before commit

Pre-commit lint-staged runs prettier --check on staged markdown, which
rejects git-cliff output. Format with prettier --write before staging
so the release commit passes pre-commit.

- Run markdownlint --fix on CHANGELOG before commit

Pre-commit also runs markdownlint-cli2 on staged markdown. Cliff output
can contain asterisk bullets from commit bodies which fail MD004.
Auto-fix after prettier to keep the release commit self-contained.

### Dependencies

- Bump frontend deps and fix format-check hanging on 22GB Rust artifacts

Consolidate 4 Dependabot PRs (#24-#27): - turbo 2.8.20 → 2.9.3 - @types/node 24.x → 25.5.0 - typescript-eslint 8.57.2 → 8.58.0 - @tanstack/react-query 5.95.2 → 5.96.0 - @tanstack/react-router 1.168.7 → 1.168.10

    Fix make format-check hanging indefinitely: prettier's fast-glob walker
    enters all directories before filtering via .prettierignore, causing it
    to traverse 22GB of Rust target/ dirs. Replace with find -prune | xargs
    which skips heavy dirs at the OS level (~3.8s vs infinite hang).

- Bulk apply 24 Dependabot PRs (#28-#53)

Backend (Rust): - rand 0.10.0 → 0.10.1 - llama-cpp-4 0.2.26 → 0.2.43 - fastembed 5.13.0 → 5.13.2 - schemars 0.8.22 → 1.2.1 - async-imap 0.10.4 → 0.11.2 (fix: updated read_response API) - async-native-tls 0.5.0 → 0.6.0 - redis 1.1.0 → 1.2.0 - mail-parser 0.9.4 → 0.11.2 - axum-test 19.1.1 → 20.0.0 - tokio 1.50.0 → 1.51.0

    Frontend (npm):
    - @tanstack/react-query + @tanstack/react-router (minor)
    - vite 8.0.3 → 8.0.7
    - turbo 2.9.3 → 2.9.5
    - jsdom 29.0.1 → 29.0.2
    - ky 1.14.3 → 2.0.0 (fix: prefixUrl→prefix, hook signature update)
    - react-hook-form 7.72.0 → 7.72.1
    - postcss 8.5.8 → 8.5.9
    - eslint group updates
    - storybook 10.3.3 → 10.3.5
    - vitest + msw updates

- Apply Dependabot PRs #55/#56 and fix security vulnerabilities

- Bump @tanstack/react-virtual ^3.13.23 → ^3.13.24 (PR #56)
  - Bump eslint-plugin-react-hooks ^7.0.1 → ^7.1.1 (PR #55)
  - PR #52 (rand 0.10.1) was already applied in Cargo.lock
  - Add pnpm override to force lodash >=4.18.1 (fixes CVE alerts #4/#5)
  - Fix MD032 lint errors in AGENTS.md and CLAUDE.md (blank line before lists)
  - Add AGENTS.md (GitNexus config, previously untracked)

- Bulk apply 12 Dependabot PRs + Rust 1.95 sweep (#69)

- deps: bulk apply 12 Dependabot PRs + Rust 1.95 sweep

  Frontend (pnpm monorepo):
  - react 19.2.4 -> 19.2.5 (#67)
  - react-hook-form 7.72.1 -> 7.73.1 (#66)
  - ky 2.0.1 -> 2.0.2 (#65)
  - lucide-react 1.7.0 -> 1.8.0 (#62)
  - nanoid 5.1.7 -> 5.1.9 (#59)
  - typescript 6.0.2 -> 6.0.3 (#64)
  - @types/node 25.5.0 -> 25.6.0 (#63)
  - tailwindcss 4.2.2 -> 4.2.4 (#61)
  - prettier 3.8.1 -> 3.8.3 (#60)
  - vitest 4.1.4 -> 4.1.5 (#58)

  Backend (cargo):
  - rand 0.10.0 -> 0.10.1 (#52)
  - Rust toolchain 1.94 -> 1.95 (Dockerfile #68, rust-toolchain.toml, Cargo.toml MSRV)

  Doc/config sweep for Rust 1.95:
  - README.md, docker-compose.yml, scripts/setup-prereqs.sh
  - docs/{setup,deployment,maintainer}-guide.md
  - docs/plan/{ci-potential-improvements,rust-builtin-llm-implementation}.md

  Historical/changelog references in docs/plan/{inception,implementation,march-2026-audit}.md
  intentionally left as-is (they document past state).

- Upgrade frontend peer deps and fix storybook/eslint conflicts

- Replace @storybook/test@8 with storybook@^10.3.5 (test utils moved into
  the storybook package in v10)
  - Allow eslint-plugin-react with eslint@10 via peerDependencyRules
  - Approve esbuild build scripts via onlyBuiltDependencies
  - Cargo.lock updated via cargo update

### Documentation

- Add OAuth setup guide for Google and Microsoft with citations

Comprehensive guide covering: - Google: credentials, consent screen (Internal vs External), Gmail API
scopes (non-sensitive/sensitive/restricted), Testing vs Production,
verification requirements, CASA security assessment - Microsoft: Entra app registration, Graph API permissions, tenant config,
delegated vs application permissions - Configuration reference mapping secrets/dev/ files to env vars - Troubleshooting: redirect_uri_mismatch, org_internal, missing env vars

    All sections cite official documentation:
    - developers.google.com/identity/protocols/oauth2
    - developers.google.com/workspace/gmail/api/auth/scopes
    - appdefensealliance.dev/casa
    - learn.microsoft.com/en-us/entra/identity-platform

- Add ADR-018 and DDD-008 for email operations domain

- Add test plan for group-by-sender feature

- Add March 2026 audit v2 — AQE quality, security, licensing review

Comprehensive project audit covering implementation quality, test coverage,
documentation, security posture (SAST + cargo-audit + npm audit), and
commercial licensing exposure across all 1,392 dependencies.

- Add UI overview guide with screenshot gallery

### Features

- Initial implementation of Emailibrium — vector-native email intelligence platform

A complete, working implementation spanning 7 development sprints, delivering
a Rust backend and React TypeScript frontend for privacy-first, semantic email
management.

    Backend (Rust — 36 source files, 387 passing tests):
    - Vector store facade with pluggable backends (ADR-003)
    - Embedding pipeline with fallback chain and Moka cache (ADR-002)
    - Hybrid search: FTS5 + HNSW vector search fused via Reciprocal Rank Fusion (ADR-001)
    - VectorCategorizer with EMA centroid classification and LLM fallback (ADR-004)
    - SONA 3-tier adaptive learning: instant feedback, session preferences, long-term consolidation
    - GraphSAGE-inspired clustering with K-means++, silhouette scoring, and stability guardrails (ADR-009)
    - Adaptive quantization: scalar (int8), product (PQ), and binary with auto-tier selection (ADR-007)
    - AES-256-GCM encryption at rest with Argon2id key derivation and zeroize (ADR-008)
    - Multi-asset content extraction: HTML, images, attachments, link classification, tracking detection
    - 6-stage ingestion pipeline with SSE progress streaming and pause/resume
    - Subscription detection with frequency analysis and actionability scoring
    - Ingest-tag-archive pipeline with configurable timing and safety mechanisms (ADR-010)
    - IR evaluation metrics: Recall@K, NDCG, MRR, Precision, macro-F1, confusion matrix, ARI, silhouette
    - SQLite vector backup with encrypted persistence
    - Criterion benchmarks: search scaling (1K-100K), quantization comparison, ingestion throughput
    - Security audit tests: encryption roundtrip, nonce randomness, embedding invertibility, CSP/CORS

    Frontend (React 19 / TypeScript — 154 source files, 0 type errors):
    - Monorepo: Turborepo + pnpm with 4 shared packages (types, api, core, ui)
    - 8 feature modules: command-center, email, inbox-cleaner, insights, rules, settings, onboarding, chat
    - Command palette (cmdk) with debounced semantic search and filter sidebar
    - Inbox Cleaner: 4-step wizard with subscription review, topic cleanup, and batch actions
    - Insights Explorer: 5-tab dashboard with Recharts (pie, line, bar), health score gauge
    - Email client: 3-panel layout with virtual scrolling, thread view, compose/reply
    - Rules Studio: AI suggestions, semantic conditions, rule builder, metrics dashboard
    - Settings: 5 tabs (general, accounts, AI/LLM, privacy, appearance)
    - 10 shared UI components: Button, Card, Badge, Input, Select, Toggle, Spinner, Avatar, EmptyState, Skeleton
    - PWA: service worker, install prompt, offline indicator
    - Accessibility: focus trapping, ARIA live regions, skip-to-content, keyboard shortcuts
    - Error handling: retry with exponential backoff, error boundaries, toast notifications
    - Secure storage: Web Crypto API (AES-GCM) + IndexedDB with non-extractable keys
    - 6 Playwright E2E spec files, 9 Storybook stories

    Architecture & Documentation (30+ documents):
    - Academic research evaluation with 30 citations (RESEARCH.md)
    - 10 Architecture Decision Records (ADR-001 through ADR-010)
    - 6 Domain-Driven Design bounded context documents
    - Primary implementation plan: 8 sprints, 47 tasks, risk register, success metrics
    - OpenAPI 3.0 specification for all 12 API endpoints
    - User guide, deployment guide, maintainer guide, configuration reference, releasing guide
    - Evaluation methodology: search quality, classification accuracy, clustering, performance, inbox zero protocol

- Vector-native email intelligence with tiered AI, semantic search, and guided inbox cleanup

- Rust backend (Axum 0.8) with 16 vector intelligence modules: embedding,
  HNSW search, categorization, clustering, ingestion, learning, insights,
  encryption, quantization, backup, metrics, consent, generative AI, reindexing
  - Tiered AI architecture: ONNX-first local embeddings (fastembed), Ollama and
    cloud providers as opt-in tiers — no data leaves the machine by default
  - Hybrid search engine combining FTS5 + HNSW + Reciprocal Rank Fusion with
    SONA adaptive re-ranking for sub-50ms semantic queries
  - SONA 3-tier adaptive learning system that improves classification and search
    from every user interaction, with A/B experiment control
  - GraphSAGE-inspired topic clustering via K-means++ with GNN-style neighbor
    aggregation for automatic email organization
  - AES-256-GCM encryption at rest with Argon2id KDF, zeroize memory, per-field
    encryption, and key rotation support
  - React 19 frontend with 8 feature modules: command center (Cmd+K palette),
    inbox cleaner (4-step wizard), email client (thread view, compose, reply),
    rules studio (AI-suggested semantic conditions), insights explorer (charts,
    health score), chat interface, onboarding, and settings
  - Multi-account onboarding supporting Gmail OAuth, Outlook, and IMAP with
    configurable archive strategies
  - 3 SQL migrations covering schema, AI consent tracking, and AI metadata
  - 5 backend evaluation test suites: classification accuracy, clustering quality,
    domain adaptation, search quality, and security audit
  - 6 Playwright E2E specs: navigation, email, inbox-cleaner, rules, search,
    onboarding
  - OpenAPI 3.0 specification for all 12 REST endpoints with full request/response
    schemas
  - 13 Architecture Decision Records and 7 Domain-Driven Design bounded context
    documents
  - Full infrastructure: Makefile, Docker Compose (dev + prod), 4 GitHub Actions
    workflows (CI, Docker publish, release, link-check), Dependabot
  - Comprehensive documentation: user guide, deployment guide, configuration
    reference, maintainer guide, releasing guide, and research papers with 30+
    academic citations
  - Security hardening: .gitignore excludes ML caches and session state, secrets
    management via env vars, CSP headers, no hardcoded credentials

- Wire dead-code infrastructure into production paths, fix lint

- Wire SONA Tier 2 session learning into SessionState with pub accessors
  (session_id, age), preference_vector (mean clicked − mean skipped), and
  rerank_boost (gamma × cosine similarity) — all real math, no stubs
  - Add quantize_vector/dequantize_vector dispatchers routing to Scalar,
    Binary, or raw fp32 based on QuantizationTier (ADR-007)
  - Add QuantizedVectorStore wrapper with store_with_quantization,
    get_dequantized, and auto-tier transition on insert
  - Wire EmbeddingStatus into ingestion via EmailEmbeddingRecord with
    Pending→Embedded/Failed state machine and mark_stale for re-embedding
  - Fix JobStatus lifecycle: jobs now create as Pending then transition to
    Running before background task spawns
  - Wire model manifest validation into EmbeddingPipeline::new — dimensions
    checked against get_manifest on init with warning on mismatch
  - Add EmbeddingPipeline::available_models and validated_dimensions public API
  - Add EvaluationReport and generate_evaluation_report aggregating ARI,
    silhouette, macro-F1, accuracy, and detection metrics (Sprint 7 ready)
  - Fix clippy: Iterator::last→next_back, manual %→is_multiple_of,
    type_complexity via type aliases
  - cargo fmt across all touched files
  - 212 tests passing, 0 non-dead-code clippy warnings

- Wire all dead-code infrastructure into API endpoints, eliminate all warnings

- Add GET /vectors/quantization endpoint exercising QuantizedVectorStore,
  quantize_vector/dequantize_vector, ScalarQuantizer, BinaryQuantizer,
  ProductQuantizer, PQCode, simple_kmeans, euclidean_distance_sq
  - Add GET /vectors/models endpoint using EmbeddingPipeline::available_models
    and validated_dimensions with model manifest lookup
  - Add GET /evaluation/report aggregating ARI, silhouette, macro-F1,
    accuracy, and subscription detection via generate_evaluation_report,
    ConfusionMatrix, adjusted_rand_index, detection_metrics, and
    reciprocal_rank_fusion
  - Add GET /learning/session exposing SONA Tier 2 SessionState with
    session_id, age, preference_vector, and rerank_boost
  - Add GET /ingestion/embedding-status exercising EmbeddingStatus and
    EmailEmbeddingRecord lifecycle (pending/embedded/failed/stale)
  - Make SessionState Clone-able, add LearningEngine::get_session()
  - Make euclidean_distance_sq and simple_kmeans pub for API access
  - Fix remaining clippy: is_multiple_of, repeat_n
  - 226 tests passing, 0 warnings, 0 clippy issues

- Redesign AI settings with provider-aware models, API keys, dark mode, and Outlook icon fix

- Redesign AI/LLM settings with ONNX-first defaults: embedding provider
  selector (Built-in ONNX / Ollama / OpenAI) with provider-filtered
  model dropdown showing dimensions and descriptions
  - Change default embedding from text-embedding-3-small (OpenAI) to
    all-MiniLM-L6-v2 (ONNX) matching the privacy-first backend default
  - Add "None (Rule-based)" as default LLM provider matching backend's
    Tier 0 generative config, with provider-specific model lists
  - Add API key inputs for OpenAI and Anthropic with masked password
    fields, show/hide toggle, and "Key saved" confirmation
  - Add Ollama base URL input with live model discovery via GET /api/tags
    showing model name, parameter count, and disk size
  - Add Ollama connection status indicator (connecting/connected/error)
  - Wire dark mode: add useThemeEffect in App.tsx that applies the dark
    class to <html> based on theme setting with OS matchMedia listener
  - Fix Outlook onboarding icon: replace garbled single-path SVG with
    proper multi-layer Outlook brand icon (envelope body, flap, O mark)
  - Add openaiApiKey, anthropicApiKey, ollamaBaseUrl to persisted settings

- Add OAuth configuration scaffolding for Gmail and Outlook (DDD-005)

- Add OAuthConfig, GmailOAuthConfig, OutlookOAuthConfig structs to
  backend config with env-var-based credential loading (never config files)
  - Gmail config: client ID/secret env vars, Gmail API scopes (modify,
    labels, userinfo.email), Google auth/token URLs
  - Outlook config: client ID/secret env vars, tenant ID (common default),
    Microsoft Graph scopes (Mail.ReadWrite, Mail.Send, offline_access,
    User.Read), dynamic auth/token URL builder per tenant
  - Add oauth section to config.yaml with all provider settings
  - Add 4 secrets templates: google_client_id, google_client_secret,
    microsoft_client_id, microsoft_client_secret
  - Mount OAuth secrets in docker-compose.yml backend service
  - Add "Email Provider OAuth Setup" section to deployment guide with
    step-by-step for Google Cloud Console, Microsoft Entra, and IMAP
  - Document all OAuth env vars (EMAILIBRIUM_GOOGLE_CLIENT_ID, etc.)
  - Update secrets/README.md with OAuth credential setup commands
  - 4 new config tests for OAuth defaults and URL construction

- Emailibrium — vector-native email intelligence platform with tiered AI, semantic search, and guided inbox cleanup

Major capabilities authored:

    - Axum REST API with 11 route groups (vectors, ingestion, insights, clustering, learning, interactions, evaluation, backup, AI, consent, auth)
    - Vector embedding engine with ONNX Runtime default, Ollama, and cloud provider tiers (ADR-011, ADR-012)
    - Hybrid search pipeline combining FTS5 full-text + HNSW vector search fused via Reciprocal Rank Fusion (ADR-001)
    - SONA adaptive learning with per-user personalization and EWC catastrophic-forgetting prevention
    - GNN-based email clustering using GraphSAGE on HNSW graphs (ADR-009)
    - Multi-asset content extraction (HTML, attachments, images, tracking pixels, link analysis)
    - Email provider integration with Gmail and Outlook OAuth scaffolding (DDD-005)
    - Ingest-tag-archive pipeline with SSE broadcast and checkpoint resumption (ADR-010)
    - Privacy architecture with embedding encryption, AI consent, and cloud API audit logging (ADR-008)
    - Model lifecycle management with integrity verification, registry, and CLI download (ADR-013)
    - Adaptive quantization for memory-constrained deployments (ADR-007)
    - Generative AI router for summarization, reply drafting, and inbox-zero recommendations
    - React SPA frontend with onboarding flow, AI settings, insights dashboard, and inbox cleaner
    - Vite 8 (Rolldown) build with Tailwind CSS v4 CSS-first configuration
    - SQLite with 9 migrations (schema, consent, metadata, accounts, FTS5, checkpoints, learning, audit, A/B tests)
    - Docker Compose production and dev stacks with security hardening (read-only, no-new-privileges, cap-drop ALL)
    - Redis caching layer with connection management
    - Event bus for domain event propagation across bounded contexts
    - 13 Architecture Decision Records and 7 Domain-Driven Design bounded context documents
    - CI pipeline: Rust format, Clippy, build, frontend lint/typecheck, link checking, Docker build, release workflows
    - Root Makefile with CI, Docker, release, and docs targets
    - Setup scripts for prerequisites, Docker, secrets, AI models, and validation
    - RuVector submodule integration as primary vector database (ADR-003)

- Complete all low-priority audit items (40-54) with vector fallbacks, remote wipe, HDBSCAN, and doc updates

Implements all 10 remaining low-priority items from the March 2026 audit: - ADR-005/DDD-000 doc updates, undocumented features in architecture.md - Makefile help fix, CHANGELOG population, model identifier reconciliation - QdrantVectorStore and SqliteVectorStore fallback backends (ADR-003) - RemoteWipeService with 5 API endpoints (ADR-008) - HDBSCAN alternative clustering algorithm (ADR-009) - Frontend tests in release CI gate, Docker Qdrant profile - Renamed INCEPTION.md/PRIMARY-IMPLEMENTATION-PLAN.md with link fixes

- Implement predecessor recommendations R-01 through R-10 with rule engine, offline sync, security middleware, and GDPR compliance

Backend (Rust — 3,824 new lines, 117 tests): - Rule engine with JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, and 7 REST endpoints (CRUD, validate, test) - IMAP email provider implementing EmailProvider trait with config validation and FETCH response parsing - Gmail incremental sync via history.list API with typed HistoryResponse and concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync via Graph delta query with typed DeltaResponse and pagination - Offline-first sync queue with FIFO dequeue, retry-until-max logic, and four conflict resolution strategies (LastWriterWins, LocalWins, RemoteWins, Manual) - Background sync scheduler with exponential backoff and configurable batch size - Processing checkpoints for crash recovery with save/resume/cleanup and 30-day retention - Token-bucket rate limiter per IP address with configurable burst size, automatic stale bucket cleanup, and 429 + Retry-After responses - HSTS header middleware and log scrubbing (redacts Bearer tokens, API keys, client secrets, passwords) - AI chat service with session management, sliding-window message history, TTL-based cleanup, and SSE streaming endpoints - Hot-reload configuration via mtime polling and Arc<RwLock<Arc<T>>> swap pattern - GDPR consent persistence (consent_decisions + privacy_audit_log tables), data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe with RFC 2369/8058 List-Unsubscribe header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer, and false-positive engagement guards - Property-based tests for content extraction, config parsing, search queries, and scalar quantization round-trips (feature-gated behind proptest) - Three new SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 657 new lines + 1,972 modified):
    - Rules Studio wired to backend: validate-before-save, test-against-sample-email with inline result display
    - Unsubscribe flow: preview dialog with per-sender method/impact, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming via POST-based ReadableStream with stop button, session tracking, and animated cursor indicator
    - GDPR consent settings tab with toggle switches per purpose, consent history table, data export (JSON/CSV), and two-step data erasure confirmation
    - Sync status indicator in Command Center header showing online/offline/pending states with cross-tab reactivity via useSyncExternalStore
    - New shared types (chat, consent, unsubscribe) and API client functions (chatApi, consentApi, unsubscribeApi)

    Documentation (942 new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR Compliance)
    - DDD-007 (Rules bounded context with aggregates, commands, events, domain services)
    - Updated architecture.md with rule engine, sync, security, and privacy sections
    - Predecessor comparison analysis with 12 recommendations (docs/plan/predecessor-recommendations.md)

- Implement predecessor recommendations R-01 through R-10 with rule engine, offline sync, security middleware, and GDPR compliance

Backend (Rust — 3,824 new lines, 117 tests): - Rule engine with JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, and 7 REST endpoints (CRUD, validate, test) - IMAP email provider implementing EmailProvider trait with config validation and FETCH response parsing - Gmail incremental sync via history.list API with typed HistoryResponse and concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync via Graph delta query with typed DeltaResponse and pagination - Offline-first sync queue with FIFO dequeue, retry-until-max logic, and four conflict resolution strategies (LastWriterWins, LocalWins, RemoteWins, Manual) - Background sync scheduler with exponential backoff and configurable batch size - Processing checkpoints for crash recovery with save/resume/cleanup and 30-day retention - Token-bucket rate limiter per IP address with configurable burst size, automatic stale bucket cleanup, and 429 + Retry-After responses - HSTS header middleware and log scrubbing (redacts Bearer tokens, API keys, client secrets, passwords) - AI chat service with session management, sliding-window message history, TTL-based cleanup, and SSE streaming endpoints - Hot-reload configuration via mtime polling and Arc<RwLock<Arc<T>>> swap pattern - GDPR consent persistence (consent_decisions + privacy_audit_log tables), data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe with RFC 2369/8058 List-Unsubscribe header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer, and false-positive engagement guards - Property-based tests for content extraction, config parsing, search queries, and scalar quantization round-trips (feature-gated behind proptest) - Three new SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 657 new lines + 1,972 modified):
    - Rules Studio wired to backend: validate-before-save, test-against-sample-email with inline result display
    - Unsubscribe flow: preview dialog with per-sender method/impact, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming via POST-based ReadableStream with stop button, session tracking, and animated cursor indicator
    - GDPR consent settings tab with toggle switches per purpose, consent history table, data export (JSON/CSV), and two-step data erasure confirmation
    - Sync status indicator in Command Center header showing online/offline/pending states with cross-tab reactivity via useSyncExternalStore
    - New shared types (chat, consent, unsubscribe) and API client functions (chatApi, consentApi, unsubscribeApi)

    Documentation (942 new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR Compliance)
    - DDD-007 (Rules bounded context with aggregates, commands, events, domain services)
    - Updated architecture.md with rule engine, sync, security, and privacy sections
    - Predecessor comparison analysis with 12 recommendations (docs/plan/predecessor-recommendations.md)

- Implement predecessor recommendations R-01 through R-10 with full-stack wiring

Backend (Rust — 3,800+ new lines, 117 tests): - Rule engine: JSON + natural-language parser, contradiction/loop validator, priority-ordered processor, 7 REST endpoints (CRUD, validate, test) - IMAP provider: EmailProvider trait impl with FETCH command builder and response parsing pipeline (TCP connection pending `imap` crate) - Gmail incremental sync: history.list API with typed HistoryResponse, concurrent batch_get_messages (buffer_unordered, max 10) - Outlook delta sync: Graph delta query with typed DeltaResponse and full pagination - Offline-first sync: FIFO queue with retry logic, four conflict resolution strategies (LastWriterWins/LocalWins/RemoteWins/Manual), background scheduler with exponential backoff - Processing checkpoints: crash recovery with save/resume/cleanup, wired into sync scheduler batch processing - Rate limiting: token-bucket per IP, wired as Axum middleware with ConnectInfo<SocketAddr>, 429 + Retry-After responses - Security headers: HSTS middleware, log scrubbing (redacts Bearer tokens, API keys, secrets) wired into router - AI chat: session management with sliding-window history, SSE streaming endpoints - Hot-reload config: mtime polling with Arc<RwLock<Arc<T>>> swap pattern - GDPR compliance: consent_decisions + privacy_audit_log tables, data export (Article 20), right to erasure (Article 17) - Bulk unsubscribe: RFC 2369/8058 header parsing, HTTP POST/GET/mailto execution, 5-minute undo buffer - Property-based tests: content extraction, config parsing, search queries, quantization (feature-gated) - Three SQLite migrations (rules, sync_queue + checkpoints, GDPR consent)

    Frontend (TypeScript — 650+ new lines, 1,900+ modified):
    - Rules Studio: validate-before-save, test-against-sample-email with inline results
    - Unsubscribe flow: preview dialog, batch execute, countdown undo toast (5-minute window)
    - Chat SSE streaming: POST-based ReadableStream with stop button, animated cursor
    - GDPR consent tab in Settings: toggle switches, consent history, data export/erase
    - Sync status indicator in Command Center: online/offline/pending with cross-tab reactivity
    - Shared types (chat, consent, unsubscribe) and API client functions

    Documentation (940+ new lines):
    - ADR-014 (Rule Engine), ADR-015 (Offline Sync), ADR-016 (Security Middleware), ADR-017 (GDPR)
    - DDD-007 (Rules bounded context)
    - Updated architecture.md and config.yaml
    - Predecessor comparison analysis (docs/plan/predecessor-recommendations.md)

- Wire dead code into live call paths — reduce warnings from 90 to 2

Rules engine wiring: - api/rules.rs create handler calls json_parser::parse_condition() and parse_natural_language() - validate handler calls validate_rules() for cross-rule loop detection - test handler calls process_email() for full pipeline evaluation - list handler uses RuleEngine struct (set_rules/rules methods)

    Middleware wiring:
    - Replaced 5 manual SetResponseHeaderLayer calls with single security_headers_middleware
    - log_scrubbing_middleware now calls scrub_query_params, scrub_headers, scrub_error_message
    - RateLimitConfig::from_env() with RateLimitPreset replaces manual config construction

    Vector services wiring:
    - CloudApiAuditLogger instantiated in VectorService, used by chat endpoint via AuditTimer
    - EvaluationEngine wired with 6 new /evaluation endpoints (A/B tests, IR metrics)
    - GenerativeRouter in VectorService with provider failover for ChatService
    - InferenceSessionManager tracks chat session lifecycle (start/complete/fail)
    - compute_mrr, compute_ndcg, compute_precision_at_k, compute_recall_at_k called from /ir-metrics

    Scaffolded modules (allow(dead_code) retained — 10 files):
    - ewc.rs, user_learning.rs, model_registry.rs, model_download.rs, model_integrity.rs
    - hdbscan.rs, ruvector_store.rs, sync_scheduler.rs, unsubscribe.rs, api/wipe.rs

- Wire provider enable/disable into AI API and consent system — zero warnings

- Add GET /ai/providers listing all registered providers with status
  - Add POST /ai/providers/:provider/disable and /enable for runtime control
  - Consent system auto-toggles cloud providers on GDPR consent changes
  - 3 new tests: list status, disable removes from selection, enable restores
  - Eliminates the last 2 dead_code warnings in the entire codebase

- Complete account settings, email sync, and ingestion pipeline

- Add emails API, sync progress logging, and dashboard UX improvements

- Global sync state via Zustand store for persistent sync UX

Move sync state (syncing, status, error, hasAccounts) from local
useState in CommandCenter into a global Zustand store (syncStore.ts).

    This means:
    - Auto-sync after OAuth uses the same store, so Dashboard shows
      progress immediately when it mounts
    - Navigating away from Dashboard and back preserves sync state —
      banner and animated Sync Now tile remain visible while sync runs
    - Sync cannot be double-triggered (store guards against it)
    - Stats auto-refetch when sync completes

- Add thread endpoint for email thread view

Add GET /api/v1/emails/thread/:thread_id that returns all emails
in a thread sorted by date, with participants and last activity.
Falls back to treating the thread_id as a message ID for single-
message threads (common with Gmail where thread_id == message_id).

- Add email action endpoints and widen inbox pane

- Add tooltips to email actions and move-to-folder dropdown

All hover action icons now have consistent title tooltips (Star,
Archive, Delete, Move to folder). The Move button opens a dropdown
with Archive, Spam, and Trash destinations. Closes on outside click.

- Implement provider-aware move-to-folder/label and star operations

This implements full cross-provider folder/label management:

- Auto-refresh expired OAuth access tokens

get_access_token now checks token_expires_at before returning.
When expired (or within 60s of expiry), it automatically refreshes
using the stored refresh token, persists the new tokens, and returns
the fresh access token. Supports both Gmail and Outlook providers.

    This prevents 401/502 errors that occurred after the 1-hour access
    token lifetime expired.

- Replace inline move dropdown with rich MoveDialog modal

The Move button now opens a proper centered modal dialog with: - Search/filter input for finding folders and labels - System folders section (INBOX, SENT, TRASH, SPAM, etc.) - Custom labels section with divider - "Create new label" input at the bottom - Keyboard support (Escape to close) - Backdrop click to dismiss - Email subject preview in header

    Replaces the clipped inline dropdown that was hard to use.

- Fix inbox count, add infinite scroll, and total emails tile

Email list: - Replace useQuery with useInfiniteQuery for paginated loading - Inbox badge now shows total email count from API (not page count) - Infinite scroll: automatically loads next page when scrolling
near the bottom of the virtualized list - Shows "Loading more..." spinner during page fetches

    Command Center:
    - Add "Emails" tile showing total ingested email count
    - Auto-refreshes every 10 seconds to reflect sync progress
    - Grid now 7 columns to accommodate the new tile

- Add distinct vector icon and emails tile to dashboard

- Disable nav items requiring LLM when none configured

Inbox Cleaner and Chat nav items are grayed out with an "LLM" badge
when llmProvider is "none" in settings. Hovering shows a tooltip
directing to Settings > AI / LLM. Items become clickable when an
LLM provider is configured.

    Also removes unused MailIcon from StatsCards.

- Collapsible sidebar with icons for all nav items

Sidebar now has: - Collapse/expand toggle button (chevron in header) - Lucide icons for each nav item (LayoutDashboard, Mail, Sparkles,
BarChart3, ListChecks, MessageSquare, Cog) - Collapsed mode: 64px wide, icons only with hover tooltips - Expanded mode: 208px wide, icons + labels - Smooth width transition animation - LLM-required items (Inbox Cleaner, Chat) grayed out with "LLM"
badge when no provider configured - All icons visible in both collapsed and expanded states

- Auto-mark-as-read on click with mark-unread option

- Add filter pills for All, Unread, Read, and Starred emails

Pill-based toggle buttons below the inbox header filter emails: - All: shows all emails (default) - Unread: shows only unread emails (isRead=false) - Read: shows only read emails (isRead=true) - Starred: shows only starred emails (isStarred=true)

- Add 2-level group-by-sender view with domain→sender→email hierarchy

Adds a "Grouped" pill to the email list that organizes emails into a
collapsible 2-level hierarchy: domains (A-Z) → senders (A-Z) → emails
(newest-first). Includes RFC 2822 address parsing, subdomain-to-root
normalization, virtual scrolling with variable row heights, infinite
scroll, and bulk hover actions (archive, delete, move, mark read/unread)
at both domain and sender levels. Removes redundant unread indicator dot.

- Implement email body HTML rendering with sandboxed iframe security

Extract body_html from Gmail (recursive MIME traversal) and Outlook
(contentType distinction) APIs, sanitize with ammonia email-specific
whitelist, and render in triple-layer sandboxed iframe (sandbox + CSP +
referrer policy). Removes vulnerable regex-based SanitizedHtml component,
fixes body truncation (200-char preview with subject fallback), and adds
ADR-019/020, DDD-009, research doc, and prioritized implementation plan.

- Add attachment management, fix body_html extraction, async sync

- Replace insights stubs with real database-driven analytics

- Email UI refinements - real counts, dynamic categories, search filter

Show actual email count with comma formatting instead of 999+ cap.
Replace hardcoded sidebar categories/topics/subscriptions with real
data from a new /api/v1/emails/categories endpoint. Add client-side
search filter bar with field selector (from/to/cc/subject/body).

- Add built-in local LLM tier (ADR-021) with quality gate fixes

Built-in Local LLM (Tier 0.5) — Backend - Add BuiltInLlmConfig to GenerativeConfig with model_id, context_size,
gpu_layers, idle_timeout, and cache_dir fields (vectors/config.rs) - Register BuiltIn variant in ProviderType enum with FromStr/Display
(vectors/model_registry.rs) - Change default generative provider from "none" to "builtin" - Add GGUF model-cached detection and AI diagnostic logging at startup
(main.rs) - Remove inline tracing from VectorService match arms; consolidate
provider logging in main.rs (vectors/mod.rs) - Add generative.builtin config block to config.yaml and
config.development.yaml

    Built-in Local LLM — Frontend AI Services (new)
    - built-in-llm-adapter.ts: node-llama-cpp wrapper with classify(),
      chat(), tokenCount(), idle auto-unload
    - built-in-llm-manager.ts: singleton lifecycle manager for model
      load/unload/status
    - generative-router.ts: priority-based provider router
      (builtin > ollama > cloud > rule-based)
    - hardware-detector.ts: GPU/CPU backend detection for Metal, CUDA, Vulkan
    - model-cache.ts: GGUF cache directory management with size reporting
    - model-downloader.ts: streaming model download with progress callbacks
    - model-manifest.ts: GGUF model registry (qwen2.5-0.5b-q4km default)
    - error-recovery.ts: retry logic with exponential backoff and
      circuit-breaker for LLM calls
    - electron-config.ts: Electron/Tauri IPC path resolution
    - useGenerativeRouter.ts: React hook exposing provider/classify/chat

    Built-in Local LLM — Frontend Test Suite (new, 6 files)
    - built-in-llm-adapter.test.ts: adapter load, classify, chat, dispose,
      error paths (469 lines)
    - generative-router.test.ts: priority routing, fallback, provider
      selection
    - hardware-detector.test.ts: backend detection mocking
    - model-management.test.ts: download, cache, manifest lifecycle
    - integration.test.ts: end-to-end adapter+router integration
    - built-in-llm-bench.ts: performance benchmark script (tok/sec,
      first-token latency)

    Built-in Local LLM — Frontend UI Integration
    - AISetup.tsx: add "Built-in LLM" tier card with icon, description, and
      model download prompt in onboarding wizard
    - AISettings.tsx: surface built-in LLM status, model info, and download
      controls in settings panel
    - ModelDownloadProgress.tsx: new component with progress bar, download
      size, and status indicators
    - useSettings.ts: add builtInLlm config fields to settings hook
    - ChatInterface.tsx: show "Powered by built-in AI (local)" badge when
      using builtin provider; add router.chat() integration comment
    - InboxCleaner.tsx: show "Powered by built-in AI" badge; add
      router.classify() integration comment
    - SetupComplete.tsx: display built-in LLM status in setup summary

    Tooling & Scripts
    - scripts/models.ts: new CLI for GGUF model management
      (list/download/delete/info)
    - scripts/setup-ai.sh: add Step 2 for built-in LLM with interactive
      download prompt; renumber Ollama to Step 3, Cloud to Step 4; update
      summary to show built-in LLM cache status
    - Makefile: add download-models and diagnose targets with help entries

    Build & Config
    - vite.config.ts: add node-llama-cpp to optimizeDeps.exclude
    - tailwind.config.ts: new Tailwind v4 config file
    - package.json: add node-llama-cpp dependency
    - pnpm-lock.yaml: lockfile refresh for new dependencies
    - .pnpm-approve-builds.json: new approved build list

    Documentation (new)
    - ADR-021-built-in-local-llm.md: architecture decision for Tier 0.5
    - ADR-021-addendum-rust-backend-llm.md: proposed Rust-side llama-cpp-2
      integration path
    - DDD-006-ai-providers-addendum-built-in-llm.md: domain design for
      built-in LLM bounded context
    - built-in-llm-implementation.md: original frontend implementation plan
    - rust-builtin-llm-implementation.md: Rust backend implementation plan
    - ci-potential-improvements.md: CI pipeline improvement notes

    Documentation (updated)
    - configuration-reference.md: document generative.builtin config block
    - deployment-guide.md: add GGUF model pre-download instructions
    - setup-guide.md: add built-in LLM section to AI provider setup

    Code Quality & Formatting
    - cargo fmt: fix match-arm formatting in vectors/mod.rs
    - clippy: map_or(false, ...) → is_some_and(...) in main.rs
    - prettier: auto-format 26 frontend files (imports, JSX, arrow fns)
    - eslint: fix react-hooks/exhaustive-deps in EmailClient, EmailList,
      GroupedEmailList (extract virtualizer deps); suppress no-console in
      bench script; remove stale eslint-disable directives
    - All quality gates green: cargo fmt, clippy, prettier, eslint, tsc,
      lychee

- Fix ingestion/categorization pipeline and add real-data sidebar navigation

Addresses 7 defects and 6 feature gaps identified during live testing with
2,100 synced Gmail emails where all categories showed "Uncategorized" and
sidebar navigation was non-functional.

    Critical Pipeline Fixes (DEFECT-1 through DEFECT-4):
    - Remove silent MockEmbeddingModel fallback in production; gate behind
      #[cfg(test)] so ONNX failures surface as errors instead of producing
      garbage vectors that poison search and classification
    - Wire categorize_with_fallback() into ingestion pipeline (was calling
      categorize() only, bypassing the entire LLM→rule-based fallback chain
      from ADR-012)
    - Load/seed category centroids on startup from DB; fresh installs get
      10 canonical centroids from embedded category descriptions so vector
      classification works from first sync instead of always returning
      "Uncategorized"
    - Add explicit "builtin" and "none" arms to generative provider match;
      previously fell through to wildcard, silently disabling LLM tier
    - Inject generative model into IngestionPipeline via set_generative()
      so the full centroid→LLM→rules fallback chain is live

    Ingestion & Progress Fixes (DEFECT-5 through DEFECT-7):
    - Remove premature Complete broadcast from outer ingestion handler;
      inner pipeline's own progress channel is now sole source of truth
    - Detect stale embedding status on startup: if vector store is empty
      but DB has emails marked 'embedded', reset to 'pending' for
      re-processing on next sync
    - Add clustering phase threshold check (>= 50 embedded emails) with
      TODO for ClusterEngine wiring

    Gmail Label Resolution:
    - Resolve Gmail label IDs (e.g. Label_356207...) to human-readable
      names by fetching label map during sync and passing through
      parse_message()
    - Add label_map parameter to parse_message() with Arc<HashMap> for
      concurrent access across buffered message fetches

    New Backend API Endpoints:
    - GET /emails/labels/all — cross-account label aggregation with counts,
      system label detection, and Title Case normalization
    - GET /emails/categories/enriched — categories with backend-assigned
      group (subscription vs category) and per-category unread counts
    - GET /emails/counts — accurate total/unread/per-category counts from
      dedicated SQL queries instead of client-side page counting
    - Add ?label=X filter parameter to GET /emails list endpoint with
      parameterized CSV-contains SQL

    Enhanced Rule-Based Classification (Gap 2):
    - Add 15+ sender domain rules: slack/discord→Notification,
      twitter/instagram/tiktok→Social, shopify→Shopping,
      mint/venmo→Finance, mailchimp/sendgrid→Marketing
    - Add sender prefix rules: noreply/no-reply→Notification,
      newsletter/digest→Newsletter
    - Add 8+ keyword rules: security alert→Alerts, your order→Shopping,
      weekly report→Work, you're invited→Personal
    - Add Gmail CATEGORY_* label mapper (category_from_gmail_label)

    Frontend Sidebar Overhaul:
    - Replace hardcoded SUBSCRIPTION_CATEGORIES and TOPIC_CATEGORIES sets
      with dynamic enriched categories from backend group field
    - Add Labels section to EmailSidebar with collapsible panel for
      provider-native labels (works without any AI)
    - Wire getAllLabels(), getEnrichedCategories(), getEmailCounts() APIs
      with React Query (30s stale, 30s refetch for counts)
    - Remove client-side count derivation from paginated email list;
      use server-side counts endpoint for accurate unread badges
    - Add 'label' icon type to SidebarGroup interface

    Command Center & Stats:
    - Reduce stats staleTime from 60s→10s and refetchInterval from
      120s→30s so vector counts appear promptly after embedding
    - Add formatIndexType() to display human-friendly names for vector
      index types (ruvector_hnsw→HNSW, etc.)
    - Add title tooltip and truncation to stat card values

    Serialization & Types:
    - Add #[serde(rename_all = "camelCase")] to HealthStatus and
      VectorStats for consistent frontend/backend field naming
    - Export AggregatedLabel, EnrichedCategory, EmailCounts types from
      @emailibrium/api package
    - Add test-vectors feature flag to Cargo.toml for dev-dependency
      access to MockEmbeddingModel in integration tests

- Add background email polling, incremental sync, and Command Center enhancements

Introduces a background poll scheduler that automatically checks each
connected account for new mail on a configurable interval, using
provider-native incremental sync (Gmail history.list, Outlook delta
queries) to fetch only new/changed messages instead of re-listing the
entire mailbox.

    Background Poll Scheduler (new module):
    - Add poll_scheduler.rs: Tokio background task that ticks every 15s,
      checking which accounts are due for a sync based on their individual
      sync_frequency setting
    - Per-account tracking: last poll time, backoff state, in-progress flag
      to prevent overlapping syncs for the same account
    - Exponential backoff on failures: 30s → 60s → 120s → ... → 10min cap,
      with automatic reset on success
    - Callback-based architecture: sync closure created in main.rs bridges
      the lib-crate scheduler to the binary-crate ingestion code
    - API endpoints: GET /ingestion/poll-status for monitoring, POST
      /ingestion/poll-toggle to enable/disable at runtime
    - Spawned at server startup in main.rs, handle stored in AppState

    Incremental Sync (two-path sync_emails_from_provider):
    - Full sync (onboarding): No history_id in sync_state — paginates all
      messages, then captures Gmail's historyId via profile endpoint for
      future incremental syncs
    - Incremental sync (polling): Has history_id — calls Gmail history.list
      or Outlook delta query to detect only added/updated/deleted messages,
      fetches full details for just those IDs
    - Graceful fallback: if delta fails (expired history_id, Gmail 404),
      clears the marker and falls through to full sync automatically
    - Handles remote deletions: messages deleted on the provider are removed
      from local DB during incremental sync
    - Extracted upsert_email() and fetch_provider_history_id() helpers to
      deduplicate code between full and incremental paths

    Sidebar Filter Fixes:
    - Fix category/subscription filter case mismatch: sidebar IDs used
      toLowerCase() but DB stores Title Case — removed lowercasing so
      filter values match exactly (e.g. "Alerts" not "alerts")
    - Add COLLATE NOCASE to backend category WHERE clause as safety net
    - Add label field to GetEmailsParams TypeScript interface for type
      safety (was passing through via cast but not in the type)

    Inbox Count Corrections:
    - Inbox pill now shows unread count (consistent with category/label
      pills) instead of total count
    - Removed subtitle feature that was added then removed per user feedback

    Command Center Enhancements:
    - Add "Unread" stat tile between Emails and Total Vectors, powered by
      the /emails/counts endpoint instead of polling getEmails({limit:1})
    - Wire Topic Clusters panel to GET /api/v1/clustering/clusters via new
      clusterApi.ts client; add camelCase serde to ClusterListResponse and
      ClusterSummary backend response types
    - Replace static "Recent Activity" placeholder with real "Category
      Breakdown" panel showing enriched categories with bar charts, email
      counts, unread counts, and group badges
    - Add "Last Updated" timestamp in header subtitle using TanStack Query's
      dataUpdatedAt, formatted as "Updated Mar 26, 2:45 PM"
    - Expand stats grid from 7 to 8 columns for the new Unread tile

    Settings & Configuration:
    - Fix sync_frequency values: dropdown was storing minutes (1, 5, 15, 60)
      but backend interprets as seconds — changed to 60, 120, 300, 900, 3600
    - Add "2 min" option to sync frequency dropdown
    - Fix backend validation: replace allowlist [1, 5, 15, 60] with range
      check 60..=86400 seconds to accept the corrected values
    - Add migration 015_sync_frequency_seconds.sql to convert existing
      accounts from minutes to seconds (values < 60 multiplied by 60)

    Code Quality (clippy + formatting):
    - Fix manual_range_contains: use !(60..=86400).contains(&f)
    - Fix needless_borrows_for_generic_args: remove & from .bind() call
    - Fix borrowed_box: &Box<dyn EmailProvider> → &dyn EmailProvider with
      &*provider at call sites
    - Run cargo fmt and prettier on all changed files

- Add RAG pipeline, externalize config to YAML, and enhance built-in LLM

Introduces email-aware RAG chat, moves hardcoded model catalogs and tuning
parameters to external YAML config files, and adds llama.cpp built-in LLM
integration with hardware-aware model selection.

    RAG Pipeline (ADR-022, DDD-010):
    - Add rag.rs with hybrid search retrieval and token-budgeted context injection
    - Integrate RAG context into chat and chat/stream endpoints
    - Auto-generate session IDs when not provided by the client
    - Rewrite SSE protocol to emit {type:"token"/"done"/"error"} events

    Config Externalization:
    - Add config/ directory with 6 YAML files: app, classification, models-llm,
      models-embedding, prompts, tuning
    - Add yaml_config.rs loader with typed structs and serde defaults
    - Add new API endpoints: /model-catalog, /embedding-catalog, /system-info,
      /config/prompts, /config/classification, /config/tuning
    - Remove all hardcoded model arrays from frontend (AISettings, model-manifest)
    - Frontend now fetches model/embedding catalogs dynamically from backend API
    - Add provider validation summary at startup

    Built-in LLM Enhancements:
    - Add generative_builtin.rs with llama-cpp-2 Rust bindings (gated behind
      builtin-llm Cargo feature)
    - Add model_catalog.rs for hardware-aware model recommendation
    - Add --download-model <id> CLI flag for targeted model downloads
    - Add OpenRouter as a generative provider option
    - Update default model to qwen3-1.7b-q4km

    Backend Infrastructure:
    - Add backend/Makefile with build, test, lint, download, and run targets
    - Update root Makefile with new backend integration targets
    - Update Cargo.toml with new dependencies (uuid, llama-cpp-2, etc.)
    - Always run ingestion pipeline on poll (handles incomplete prior runs)
    - Always create ChatService (frontend handles local chat when no backend provider)
    - Load chat session TTL and history limits from YAML tuning config

    Frontend Enhancements:
    - Refactor AISettings to load models from API instead of hardcoded arrays
    - Update ClusterVisualization, StatsCards, and CommandCenter components
    - Update ModelDownloadProgress for API-driven catalog
    - Add branding assets (SVG logo/icon/text) to public/
    - Update Layout, useChat hook, AccountSettings
    - Add ingestion API exports and vector types

- Merge 20 dependabot PRs, fix breaking changes, add 248 tests

Dependency upgrades (20 PRs merged): - CI: actions/checkout v6, setup-node v6, upload-artifact v7,
docker/build-push-action v7, markdownlint-cli2-action v23 - Backend: sha2 0.11, rand 0.9, redis 1.1, hf-hub 0.5,
criterion 0.8, async_zip 0.0.18, axum-test 19.1 - Frontend: TypeScript 6, vitest 4, zod 4, recharts 3,
storybook 10, jsdom 29, eslint 10 - Docker: node 24-alpine -> 25-alpine

    Breaking change fixes:
    - sha2 0.11: use byte slice iteration for hex formatting
    - rand 0.9: rename thread_rng() to rng()
    - TypeScript 6: add ignoreDeprecations, node types, vite-env.d.ts
    - Recharts 3: fix Tooltip formatter type, nullish coalescing
    - Storybook 10: add React.ComponentType to decorator params
    - Vitest 4: migrate all mocks to vi.hoisted() pattern

    New tests (248 total):
    - Backend middleware: rate limiting (15), log scrubbing (15),
      security headers (15)
    - Backend API integration: 31 HTTP tests with in-memory SQLite
    - Backend RAG pipeline (12) + poll scheduler (14)
    - Frontend security: secureStorage AES-GCM (9), error-recovery (13),
      retry utility (10)
    - Frontend hooks: useEmails (16), useConsent (10), useSettings (11),
      useChat (10), syncStore (11)
    - Frontend API client + 5 modules (56)

- Show active model name in chat header for all providers

The chat header now displays the active model name instead of the
generic "Powered by built-in AI (local)": - builtin: shows model ID (e.g., "qwen3-1.7b-q4km") - local: shows "Ollama" - openai: shows "OpenAI" - anthropic: shows "Anthropic" - none: shows "Rule-based"

- Soft-delete, spam/trash management, filter toggle, and UI actions

- Auto-purge expired trash/spam emails (Phase 9)

Hourly background task deletes emails that have been in trash or spam
longer than configured retention period (default 30 days each).

    - Added EmailConfig struct with trash/spam retention_days,
      skip_trash/spam_embedding, default_folder_filter
    - Added email section to AppConfig
    - Spawns tokio task at startup, runs hourly
    - Logs purge counts when emails are removed
    - Config in app.yaml: email.trash_retention_days, spam_retention_days

- Add background label repair and grouped thread links

Add periodic background task to re-resolve unresolved Gmail label IDs
(e.g. Label_356207529) to human-readable names. Refactor ThreadView
link extraction to deduplicate, classify, and group links by type
(actions, documents, social, unsubscribe, tracking).

- Add Sent mail filter pill and fix sidebar count accuracy

Add folder-based filtering across the full stack so users can view sent
mail via a new "Sent" pill in the Email Tools filter bar. Fix sidebar
counts that were showing total email counts as if they were unread —
now correctly displays both total (gray pill) and unread (indigo pill
with envelope icon) counts for categories, subscriptions, and labels.

- Topic clusters dashboard with full sync, re-embed, and incremental reclustering

- Add Topic Clusters visualization with word clouds and representative emails
  - Add clustering status to AI Readiness panel (embedding + clustering progress)
  - Persist clusters to SQLite so they survive server restarts (migration 017)
  - Add configurable min_k/max_k for silhouette K-selection (tuning.yaml)
  - Add LLM warmup inference after model load to prevent cold-start empty responses
  - Fix stale-emails bug: fetch_pending_emails now includes 'stale' status
  - Add selective re-embed modes (all/failed/stale) with auto-trigger ingestion
  - Add clear_all to VectorStoreBackend trait (all 5 backends)
  - Add email_id dedup in batch_insert to prevent duplicate vectors on re-embed
  - Clear clusters from SQLite + memory during full re-embed
  - Add Full Sync mode to Sync Now (clears vectors/clusters + rebuilds everything)
  - Add incremental reclustering: auto-recluster when 50+ new emails accumulate
  - Externalize all timeouts and polling intervals to config/app.yaml
  - Add useAppConfig hook for frontend config consumption via GET /ai/config/app
  - Data-driven polling: faster refresh (5s) when embedding is in progress

- Add pipeline concurrency locks, List-Unsubscribe header support, and clustering performance tuning

Pipeline Concurrency Control: - Add per-account pipeline locking (sync_lock.rs) preventing concurrent sync/ingestion runs - Return HTTP 409 with structured PipelineBusyResponse when pipeline is already active - Add /api/v1/ingestion/lock-status endpoint for frontend pre-flight checks - Integrate lock acquisition/release in both manual ingestion and poll scheduler - Frontend: PipelineBusyError class, syncStore surfaces conflict messages with phase details - InboxCleaner pre-flight lock check with user-facing conflict alert banner

    RFC 2369/8058 List-Unsubscribe Headers:
    - Add list_unsubscribe and list_unsubscribe_post columns to emails table (migration 018)
    - Extract headers from Gmail (payload.headers), Outlook (internetMessageHeaders), and IMAP (FETCH fields)
    - Shared UnsubscribeHeaders utility in email/types.rs with per-provider adapters
    - Persist headers during upsert with COALESCE to preserve existing values
    - Surface headers in SubscriptionInsight for smarter unsubscribe method selection
    - Exclude user's own email addresses from subscription detection (sent mail false positives)

    Unsubscribe API Realignment:
    - Switch from subscriptionIds to UnsubscribeTarget with sender + header fields
    - Add camelCase serde for SubscriptionTarget, UnsubscribeResult, BatchResult, UnsubscribePreview
    - Frontend types: UnsubscribeTarget, updated UnsubscribeResult/Preview to match backend schema
    - SubscriptionsPanel: dual Unsubscribe/Keep buttons per row, forward headers to preview/batch APIs

    Clustering & Ingestion Performance (ADR-021):
    - Pipeline channel buffer: 2 → 32 (aligns with Tokio internal block size)
    - Silhouette sampling: 3000 → 500 (configurable via tuning.yaml)
    - KMeans probe iterations: 50 → 15, final iterations: 100 → 30
    - Precompute global TF-IDF document-frequency table once instead of per-cluster O(K×N×V)
    - All tuning parameters externalized in config/tuning.yaml

    Dashboard Real-Time Pipeline Awareness:
    - StatsCards polls /ingestion/progress for live phase + count display
    - ClusterVisualization uses pipeline phase for accurate empty-state messaging
    - CommandCenter invalidates dashboard queries on sync completion
    - syncStore: phase-aware progress (syncing → embedding → categorizing → clustering → complete)

    Rules Engine Foundation:
    - Add engine.rs (rule evaluation), parser.rs (YAML parsing), validator.rs (JSON Schema validation)
    - Wire list_unsubscribe fields into rule test email context

- Real-time pipeline observability, adaptive polling, and dashboard UX improvements

Backend — backfill progress tracking: - Add BackfillProgress struct and shared state to IngestionPipeline/Handle - New GET /api/v1/ingestion/backfill-progress endpoint for polling LLM backfill state - Track total, categorized, and failed counts during background backfill - Populate total count from pending_backfill query before starting batches - Broadcast accurate total in SSE progress events (was previously 0)

    Frontend — persistent pipeline status banner (CommandCenter):
    - Replace simple sync banner with phase-aware pipeline banner driven by ingestion progress API
    - Show per-phase labels (Fetching, Embedding, Categorizing, Clustering, AI categorization, etc.)
    - Display account name, percentage, counts, ETA, and inline progress bar for embedding phase
    - Green checkmark + auto-dismiss after 10s on completion
    - Separate error banner with dismiss button

    Frontend — adaptive polling and cache tuning:
    - Add 4 new cache config keys: ingestionActiveRefetchIntervalMs, ingestionActiveStaleTimeMs,
      statsRefetchIntervalMs, statsActiveRefetchIntervalMs
    - useStats accepts isActive flag to switch between idle and active polling intervals
    - StatsCards: poll email counts and vector stats faster during active ingestion; invalidate
      queries reactively when ingestion processed/embedded counts change
    - StatsCards: poll backfill progress and invalidate categories-enriched-cc as categorization advances
    - Email counts query uses faster stale/refetch times when pipeline is active
    - Embedding query polls at active rate during pipeline run

    Frontend — AI Readiness and cluster visualization:
    - Show AI Readiness card during active pipeline even when no emails are embedded yet
    - Add backfilling phase to "past embedding" detection (treat as 100% embedded)
    - Add categorization sub-section to AI Readiness driven by backfill progress
    - ClusterVisualization: suppress stale clusters during pre-clustering phases (syncing, embedding,
      categorizing, analyzing) and show progress message instead
    - SyncStatusIndicator: show "Processing" dot during active pipeline or sync; restructure into
      clear priority branches (pipeline → offline → pending → synced)

    Frontend — config and plumbing:
    - Add snakeToCamel transform in useAppConfig to handle backend YAML snake_case keys
    - Deep-merge remote config with defaults so every key has a guaranteed fallback
    - Export getBackfillProgress and BackfillProgressResponse from @emailibrium/api
    - Add backfilling phase label to syncStore phase map
    - Remove delayed "Sync complete!" Zustand message; pipeline banner handles completion now
    - Fix QuickActions "Add Account" href from /settings to /onboarding

- Add email sync progress banner, full IMAP provider, and Outlook $count support

Backend — Email sync progress visibility: - Add `last_progress` cache to `IngestionBroadcast` so the polling endpoint
(`/api/v1/ingestion/progress`) returns sync-phase progress before the
pipeline creates a job — previously the endpoint returned `active: false`
during the entire syncing phase, making the banner invisible - Broadcast per-page progress inside the full-sync pagination loop with
running count of fetched emails and provider's estimated total - Update `ingestion_progress_json` to fall back to broadcast cache when
the pipeline has no active job, covering the syncing phase gap - Clear broadcast cache on pipeline lock release to prevent stale data

    Backend — Outlook provider enhancements:
    - Add `$count=true` and `ConsistencyLevel: eventual` header on first page
      of Graph API message list requests to get total message count
    - Extract `@odata.count` into `result_size_estimate` for determinate
      progress bar during Outlook email sync (was previously `None`)

    Backend — Full IMAP provider implementation:
    - Add `async-imap` (0.10, runtime-tokio), `async-native-tls` (0.5,
      runtime-tokio), and `mail-parser` (0.9) dependencies
    - Complete rewrite of IMAP provider with real async TLS connections,
      replacing the previous stub that returned errors for all operations
    - Implement all 16 EmailProvider trait methods with full parity to
      Gmail/Outlook: authenticate, list_messages, get_message, archive,
      label/remove_labels, create/delete_label, list_labels, list_folders,
      move_message, unarchive, mark_read, star_message
    - UID-based descending pagination with EXISTS count for progress bar
    - RFC822 body parsing via mail-parser for robust MIME/header extraction
    - Per-operation connection lifecycle (connect → operate → logout)
    - 22 unit tests covering config validation and trait method gating

    Frontend — Command Center dashboard:
    - Add syncing-phase progress bar to the pipeline banner — determinate
      with percentage when provider gives total estimate (Gmail/Outlook),
      indeterminate pulsing bar otherwise (fallback)
    - Show "Fetching emails (X% — N / ~total)" status text during sync
    - Fix banner not appearing at all during email ingestion phase

- Improve search with FTS5 query sanitizer, from_name indexing, and sender name matching

Add FTS5 query sanitization to strip stop words and join terms with OR for
natural-language queries. Index from_name in FTS5 so sender display names are
searchable. Improve LIKE fallback to search per-keyword independently. Match
sender filters against from_name in addition to from_addr. Simplify sync
progress banner to indeterminate bar since provider estimates are unreliable.
Speed up hook-handler for pre-bash/post-bash by skipping stdin timeout.

- Add MCP server, tool-calling orchestrator, and enhanced RAG pipeline

Implement ADR-028 (MCP + tool-calling chat) and ADR-029 (enhanced RAG):

    - MCP Streamable HTTP server at /api/v1/mcp with rate limiting and audit logging
    - Chat orchestrator with human-in-the-loop tool confirmation flow
    - Tool-calling provider abstraction for cloud LLMs (OpenAI, Anthropic)
    - Reranker, extractive snippet extraction, and query intent parser
    - Thread grouping for conversation-aware search results
    - Weighted Reciprocal Rank Fusion with per-retriever weights
    - Frontend: ToolCallIndicator, ConfirmationDialog, SSE event types
    - Migrations: FTS5 BM25 weights, thread_key column, MCP tool audit table
    - Consolidate configs/ into config/environments/

- Disable nav items until an email account is onboarded

Only Command Center and Settings remain active before onboarding.
Other nav items are grayed out with a tooltip prompting the user
to connect an account first.

- Add tool_calling capability to model catalog

Surface a `tool_calling` boolean across backend structs, the YAML model
catalog, and the API response so the frontend can show which models
support native function/tool calling.

    - Add `tool_calling` field to ModelInfo, LlmModelEntry, and API mapping
    - Tag every model entry in models-llm.yaml with tool_calling capability
    - Add Gemma 4 (E4B, 26B MoE, 31B), Nemotron 3 Nano (4B, 30B MoE) models
    - Refine tuning params (top_p, repeat_penalty) and fix context sizes
    - Show Tools badge in ChatInterface when active model supports tools
    - Show tool-calling indicator in AISettings model selector
    - Resolve toolCalling state in useGenerativeRouter per provider type

- Persist settings, improve RAG sender matching, upgrade llama-cpp

- Add app_settings table and GET/PUT /settings endpoints so user
  preferences (e.g. selected LLM model) survive server restarts
  - Frontend bidirectional settings sync with debounced auto-push
  - Support multi-word sender names in query parser ("Josh Bob",
    "Mind Valley") with trailing punctuation stripping
  - Inject extracted sender names into RAG search text and add
    space-collapsed variants for fuzzy from_name matching
  - Lower context_sufficiency_threshold from 0.01 to 0.005
  - Upgrade llama-cpp-2 to llama-cpp-4 (0.1→0.2), fix UTF-8 safe
    phrase repetition detection, strip leaked chat template markers
  - Add daily rotating file logs via tracing-appender
  - Promote RAG debug logging to info for better observability

- Add send, reply, and forward email endpoints

Implement the missing backend API routes that the frontend was already
calling (POST /emails/send, /emails/:id/reply, /emails/:id/forward).
Add send_message, reply_to_message, and forward_message to the
EmailProvider trait with implementations for Gmail (RFC 2822 via
messages.send), Outlook (Graph sendMail/reply/forward), and IMAP
(SMTP via lettre). Sent messages are inserted into the local DB
immediately so they appear in the Sent filter without waiting for sync.

- Tag-driven releases with auto-changelog and version pill

- Add cliff.toml for git-cliff changelog generation from conventional commits
  - Add scripts/release.sh: one-command release that bumps all version files
    (backend/Cargo.toml + 5 frontend package.json), refreshes Cargo.lock,
    regenerates CHANGELOG.md, then commits, tags, and pushes with prompts
  - Update Makefile: make release VERSION=x.y.z calls release.sh;
    make changelog regenerates CHANGELOG.md via git-cliff
  - Update release.yml: add version consistency verification step (tag must
    match backend/Cargo.toml and frontend/apps/web/package.json); replace
    custom generate-changelog.sh with orhun/git-cliff-action@v4
  - Add frontend version pill: vite.config.ts injects **APP_VERSION** from
    package.json at build time; Layout.tsx sidebar shows vX.Y.Z pill next
    to Emailibrium logo
  - Pin Docker base to node:24-alpine (LTS); downgrade @types/node to ^24;
    regenerate pnpm-lock.yaml
  - Update docs/releasing.md and docs/maintainer-guide.md with one-command
    release workflow, one-time setup requirements, and rollback procedure

- Move version pill to sidebar footer

Relocate the version pill from beside the brand title to the bottom-left
of the sidebar nav so the header shows only the logo. Hidden when the
sidebar is collapsed.

### Performance

- Optimize ingestion pipeline for 100K+ email onboarding

- Complete ingestion optimization suite (P1, P6, P7, P9)

P1 — Onboarding mode: rules-only classification during bulk sync
with async LLM backfill. Emails classified as "pending_backfill"
are processed in background after initial sync completes, using
buffer_unordered(backfill_concurrency) with configurable batch
size and throttling. Enables 100K inbox onboarding in under 10min.

    P6 — Batch LLM classification: classify N emails in one prompt
    via new classify_batch() trait method on all 4 providers. Batch
    prompt template from prompts.yaml with {{categories}}/{{count}}
    substitution. Per-line parse errors fall back to individual calls.
    New categorize_batch_with_fallback_config() in categorizer.

    P7 — Batch Redis cache: MGET/MSET replace N individual GET/SET
    calls per embedding batch. 2N round-trips reduced to 2 regardless
    of batch size. Graceful degradation on Redis failure.

    P9 — Per-collection RwLock: removed global lock on RuVectorStore.
    Each of 4 vector collections has its own independent RwLock.
    Concurrent writes to different collections no longer serialize.
    AtomicUsize for sidecar write counter.

- Optimize clustering pipeline from O(n²) to O(n log n) for 100k+ scale

- Replace brute-force all-pairs similarity graph with HNSW ANN search
  - Add sampled silhouette score (3k samples) to avoid O(n²) K selection
  - Use inverted indexes for sender/thread edge construction
  - Projected 100k-email clustering: ~10-19 hours → ~3-8 minutes (release)

### Refactors

- Eliminate hardcoded model catalog, source from YAML only

- Retrofit hardcoded config with YAML-driven configuration (wave 1)

Comprehensive plumbing of all 6 YAML config files into the Rust backend,
replacing ~50 hardcoded values across 11 files:

    - LLM params: temperature, top_p, repeat_penalty, classification params
      now sourced from tuning.yaml with per-model overrides from models-llm.yaml
      via GenerationParams resolution (all 4 providers: Ollama, Cloud, BuiltIn,
      OpenRouter)
    - Classification: domain rules, keyword rules from classification.yaml;
      prompts from prompts.yaml with {{categories}}/{{email_text}} substitution
    - Ingestion: embedding_batch_size, min_cluster_emails from tuning.yaml
    - Clustering: tfidf_max_terms, representative_emails from tuning.yaml
    - Error recovery: max_retries, retry_delay_ms with retry loop
    - Repetition detection: token_window, thresholds, phrase checks
    - Poll scheduler: intervals from app.yaml sync config
    - RAG pipeline: built from tuning.yaml via From<&RagTuning>
    - Hardware: OS overhead from app.yaml for model catalog sizing
    - New /api/v1/ai/config/app endpoint serves AppConfig to frontend

- Retrofit remaining config gaps with YAML-driven configuration (wave 2)

Memory management: idle model unloading with RAM-aware timeout selection
(idle_timeout_secs / low_ram_idle_timeout_secs), memory safety margin
applied to model fit checks, background monitoring task with configurable
interval and memory warning threshold.

    Model catalog: surface all metadata fields (family, rag_capable, notes,
    cost, chat_template, default_for_ram_mb) in API responses. Rewrote
    recommend_model() to use default_for_ram_mb for RAM-based auto-selection.
    Embedding catalog: added provider description, fastembed_variant,
    fastembed_quantized, is_default, ollama_tag.

    App config: paths from app.yaml override Figment compile-time defaults,
    security (rate_limit_capacity/refill, hsts_max_age) with env>yaml>default
    fallback chain, sync completion config accessible via PollSchedulerHandle,
    ingestion start timeout from network config.

### Security

- Remove email embeddings data and gitignore backend/data/

backend/data/vectors/email_text/documents.json contained 10.1 MB of
real email embedding vectors with email IDs — runtime data that should
never be checked in. Added backend/data/ to .gitignore.

    History rewrite follows to purge the blob entirely.
