# MCP Tool Surface

Emailibrium embeds a [Model Context Protocol](https://modelcontextprotocol.io) server so an
MCP client (or the in-process chat assistant) can call email operations as tools instead of
REST endpoints. This document describes the surface as it actually exists today, generated
from the single source of truth: `backend/src/tools/mod.rs::declarations()`.

## Transports

Both transports serve the exact same registry -- same tools, same schemas, same rate limits.

| Transport                 | How                                                     | Source                                  |
| ------------------------- | ------------------------------------------------------- | --------------------------------------- |
| Streamable HTTP (default) | Mounted at `/api/v1/mcp` alongside the REST API         | `backend/src/main.rs`                   |
| stdio                     | `--mcp-stdio` CLI flag, or `EMAILIBRIUM_MCP_MODE=stdio` | `backend/src/main.rs::resolve_mcp_mode` |

In stdio mode the process is a pure MCP server over stdin/stdout: it never binds an HTTP port
or builds the REST router. `--mcp-stdio` takes precedence over the environment variable.

## What's actually implemented

**15 tools, all read-only.** There is no write/action tool live in this build -- `send_email`,
`delete_email`, and `create_rule` are recorded in `config/tools.yaml` with `deferred: true`
(their intended rate limit and confirmation policy is pre-declared) but have no handler and are
not in the registry. Calling them is not possible; they exist in config only so a future
implementation doesn't have to invent policy for them from scratch. See ADR-028 and
`docs/plan/mcp-maturation.md` for the maturation roadmap.

Every enabled tool goes through one dispatch path (`ToolRegistry::dispatch`) shared by both the
MCP transport and the in-process chat orchestrator, so rate limiting and audit logging apply
identically regardless of caller.

### The original seven

| Tool                 | Description                                                                 | Rate limit/min |
| -------------------- | --------------------------------------------------------------------------- | -------------- |
| `search_emails`      | Search emails by query text; returns sender, subject, date, relevance score | 30             |
| `get_email`          | Full email content (headers, body, metadata) by ID                          | 20 (default)   |
| `list_recent_emails` | Most recent emails across all connected accounts                            | 20 (default)   |
| `count_emails`       | Count emails, filterable by sender, category, date range                    | 20 (default)   |
| `get_email_thread`   | All emails in the same conversation thread, ordered by date                 | 20 (default)   |
| `get_insights`       | Analytics: counts by category, top senders, daily volume (7 days)           | 20 (default)   |
| `list_rules`         | All email rules with conditions, actions, and status                        | 20 (default)   |

### The eight A3 tools

| Tool                   | Description                                                                                                  | Rate limit/min |
| ---------------------- | ------------------------------------------------------------------------------------------------------------ | -------------- |
| `list_accounts`        | Connected accounts with provider, status, sync counters (includes disconnected/errored)                      | 20             |
| `get_sync_status`      | Ingestion pipeline progress + per-account sync state (independent of each other)                             | 30             |
| `list_subscriptions`   | Detected newsletter/mailing-list senders with frequency, read rate, suggested action                         | **5**          |
| `preview_cleanup_plan` | Build a cleanup plan in memory and summarize it                                                              | **5**          |
| `find_similar_emails`  | Emails semantically nearest to a given email, by embedding similarity                                        | 30             |
| `list_clusters`        | Discovered topic clusters with top terms and representative emails                                           | 20             |
| `get_learning_metrics` | Relevance-learning counters: feedback volume, click ranks, centroid drift (process-local, resets on restart) | 30             |
| `list_attachments`     | Attachment metadata (filenames, types, sizes) for one email -- never file contents                           | 30             |

`list_subscriptions` and `preview_cleanup_plan` are rate-limited to 5/min deliberately: both scan
full `body_text` per sender, which is far more expensive than the other read-only tools
(`config/tools.yaml` comment, `backend/src/tools/mod.rs`).

### `preview_cleanup_plan` is strictly ephemeral -- unlike the REST endpoint of a similar name

This MCP tool's description says it plainly: "Strictly dry-run: nothing is saved and no mailbox
is modified. The returned plan is ephemeral and cannot be applied"
(`backend/src/tools/mod.rs::declarations()`, where the tool's description string is defined; the
handler itself is `backend/src/tools/readonly/cleanup_preview.rs`). That is accurate for this
code path.

It is easy to conflate with `POST /cleanup/plan` (the REST endpoint documented in
`docs/api/openapi.yaml`, tag `cleanup`) -- but that handler
(`backend/src/cleanup/api/plan.rs::create_plan`) genuinely persists the plan it builds, with a
30-minute `valid_until` TTL, so it can be reviewed and applied later via `POST
/cleanup/apply/{plan_id}`. The two are separate implementations behind similarly-named
surfaces: the MCP tool is read-only by design (an LLM should not be able to create durable
state as a side effect of "just asking a question"), the REST endpoint is the real
plan-then-apply workflow the cleanup wizard UI drives.

## Request parameters

Full JSON Schemas are generated from each tool's request struct via `schemars`
(`backend/src/tools/mod.rs::schema_for`) and published verbatim over both transports --
`get_insights`, `list_rules`, `list_accounts`, and `get_learning_metrics` take no arguments and
publish an explicit empty-object schema (`{"type":"object","properties":{}}`). Every optional
field has a server-side default, so an empty `{}` argument object is always a valid call for
tools that have any optional-only fields. Field names below are exactly as declared in
`backend/src/tools/readonly/params.rs` -- these structs have no `rename_all`, so the wire
format is the Rust field name unchanged (snake_case).

| Tool                   | Parameters                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `search_emails`        | `query: string` (required), `limit: u32` (default 20)                                                                                                                                                                                                                                                                                                                                                                                         |
| `get_email`            | `email_id: string` (required)                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `list_recent_emails`   | `limit: u32?` (default 20, max 100)                                                                                                                                                                                                                                                                                                                                                                                                           |
| `count_emails`         | `from_filter?`, `category?`, `after?` (ISO 8601), `before?` (ISO 8601)                                                                                                                                                                                                                                                                                                                                                                        |
| `get_email_thread`     | `email_id: string` (required)                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `get_insights`         | _(none)_                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `list_rules`           | _(none)_                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `list_accounts`        | _(none)_                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `get_sync_status`      | `account_id: string?` -- omit for all accounts                                                                                                                                                                                                                                                                                                                                                                                                |
| `list_subscriptions`   | `limit?` (default 50, max 200), `category?` (newsletter/marketing/notification/receipt/social/unknown), `min_email_count?`                                                                                                                                                                                                                                                                                                                    |
| `preview_cleanup_plan` | `user_id: string` (required, `[A-Za-z0-9_-]+`), `account_ids: string[]` (empty = every account), `unsubscribe_senders: {sender, account_id}[]`, `cluster_actions: {cluster_id, account_id, action}[]` (action: archive/deleteSoft/deletePermanent/label), `rule_selections: {rule_id, account_id}[]` (evaluate-only, never applied), `archive_strategy?` (olderThan30d/olderThan90d/olderThan1y/custom), `sample_limit?` (default 10, max 50) |
| `find_similar_emails`  | `email_id: string` (required), `limit?` (default 10, max 50), `min_score?` (0.0-1.0, default 0.5)                                                                                                                                                                                                                                                                                                                                             |
| `list_clusters`        | `limit?` (default 20, max 100), `include_representatives?` (default true)                                                                                                                                                                                                                                                                                                                                                                     |
| `get_learning_metrics` | _(none)_                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `list_attachments`     | `email_id: string` (required), `include_inline?` (default false)                                                                                                                                                                                                                                                                                                                                                                              |

## Errors

Every handler returns one of six failure kinds (`backend/src/tools/registry.rs::ToolError`),
surfaced with a stable machine-readable `kind()` label: `not_found`, `invalid`, `database`,
`not_configured` (the backing service isn't wired in this deployment), `denied` (disabled by
`config/tools.yaml` policy), `rate_limited` (over the per-minute budget -- distinct from
`denied` because a client should retry a rate limit but never retry a denial).

## Keeping this accurate

This file is generated by hand from `backend/src/tools/mod.rs::declarations()` and
`config/tools.yaml`. If a tool is added, removed, renamed, or its policy changes, this file
drifts the same way `docs/api/openapi.yaml` would for a REST route -- there is no automated
check yet. `backend/src/tools/mod.rs`'s own test suite (`every_tool_is_declared_exactly_once`,
`the_shipped_config_names_only_tools_that_exist`, etc.) guards the registry's internal
consistency but does not check this document.
