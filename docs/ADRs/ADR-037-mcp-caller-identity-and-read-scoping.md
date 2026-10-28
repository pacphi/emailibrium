# ADR-037: MCP Caller Identity Is Derived, Never Client-Supplied

- **Status:** Accepted
- **Date:** 2026-10-27
- **Deciders:** Chris Phillipson
- **Related:** ADR-028 (MCP tool calling & chat — the registry and dispatch path this ADR scopes), ADR-030 (cleanup dry-run/apply/undo — `cleanup_plans` is the one table this ADR's scoping actually touches), ADR-031 (multi-tenancy groundwork — the `user_id` columns this ADR starts wiring to verified identity), ADR-036 (SeaORM dialect layer — unaffected, this ADR adds a filter, not a new dialect concern).
- **Supersedes:** The `.autopilot`-queued `mcp-identity-authz` pipeline (parked 2026-08-06, retired along with the rest of `.autopilot/` on 2026-10-27; its phase 0 findings were re-verified against current `develop` before this ADR was written — see `docs/plan/postgres-autopilot-delta-2026-10.md` §4b).

## 1. Context

`backend/src/middleware/local_auth.rs` (shipped 2026-09-22, independently of this ADR)
answers one question: does this request carry the engine's credential? It does not
answer, and was never meant to answer, a second question: *who is this request
acting on behalf of?* Before this ADR, that second question had no real answer —
it had a self-reported one.

Verified on `develop` (commit `37efb42`) before writing any code:

- `backend/src/tools/readonly/params.rs:106` declared `pub user_id: String` on
  `PreviewCleanupPlanRequest`, published in the MCP tool's input schema. The
  caller supplied it directly; nothing checked it against the credential.
- `backend/src/tools/context.rs`'s `ToolContext` carried `db`, `vectors`, `oauth`,
  `poll_scheduler`, `sync_progress`, `pipeline_locks` — no identity field of any
  kind.
- `backend/src/cleanup/repository/plan_repo.rs`'s `CleanupPlanRepository::load`
  and `list_by_user` already filtered by `user_id`; `cancel`, `list_operations`
  and `sample_operations` did not — knowing a plan's UUID was sufficient to
  cancel it or read its operations, regardless of who created it
  (`pl-plan-id-only-authz`).
- The REST layer (`backend/src/cleanup/api/plan.rs`) had the identical shape:
  `get_plan` and `create_plan` already took `user_id` from a query parameter
  (`UserQuery`); `cancel_plan` received the same query struct and **ignored
  it** (`Query(_q)`); `list_operations` and `sample_operations` didn't accept
  it at all.
- `local_auth.rs`'s own doc comment: *"This is one local principal, not
  multi-tenant authorization."* There is exactly one real caller in production.
  The `user_id` columns already in `cleanup_plans`/`cleanup_audit_log`
  (migrations 024/025) and `per_user_learning`/`cloud_api_audit` (007/008) are
  ADR-031's multi-tenancy groundwork — schema ready, never wired to a verified
  identity.
- `backend/src/api/accounts.rs` (the connected mailboxes) does not reference
  `user_id` at all. This ADR does not change which mailboxes a caller sees.

The threat this closes is not "two real users mixing up mail" — there is one
real user today. It is: **anything that can authenticate at all could, before
this change, additionally claim to be an arbitrary `user_id` and act on that
claim's cleanup plans.** Milestone 2 of the mail-agent plan
(`docs/plan/mail-agent-2026-10.md`) adds more MCP tools on top of this exact
dispatch path; this needed to be fixed before that, not after.

## 2. Decision

1. **A `Principal` type (`backend/src/principal.rs`) is the only thing a tool
   handler or cleanup-plan query may use for "whose data is this."** It is
   constructed from the engine's own fixed identity
   (`Principal::local_operator()`), never from a request field. `ToolContext`
   carries one as a required field, populated at construction — the same place
   `db`/`vectors`/etc. are wired, not per-request, because this engine has
   exactly one caller identity to carry.
2. **`user_id` is removed from `PreviewCleanupPlanRequest`'s published schema.**
   `#[serde(deny_unknown_fields)]` makes a request that still sends it fail
   deserialization — rejected as invalid params, not silently ignored. Silent
   ignoring would leave a caller believing it still chooses whose plan is
   built.
3. **`CleanupPlanRepository::cancel`, `list_operations` and
   `sample_operations` now take `user_id: &str` and enforce it**, via a shared
   `assert_owned` check that returns `NotFound` — identical to "doesn't
   exist" — for a plan that exists but belongs to someone else, so the error
   doesn't itself leak which plan ids are claimed. `load`'s internal
   pagination call into `list_operations` now threads the same `user_id`
   through; the redundant re-check costs one extra filtered lookup per page
   and is not worth special-casing away.
4. **The REST layer gets the identical, pre-existing treatment its siblings
   already had**, not a redesign: `cancel_plan` now actually uses the
   `UserQuery` it was already receiving and ignoring; `list_operations` and
   `sample_operations` gain a `user_id` field on their own query structs
   (`ListOpsQuery`, `SampleQuery`), matching the established per-route
   convention already used by `RefreshQuery`. This ADR does **not** change
   where the REST layer's `user_id` comes from (still a query parameter,
   still caller-supplied) — see §4, Non-goals.
5. **Of the 15 published MCP read tools, exactly one needed a code change.**
   See §3 for the full enumeration — the per-tool decision this ADR is
   required to record.

## 3. Tool-by-tool scoping decision (required enumeration)

| Tool | Backing data | Owner column? | Decision |
| --- | --- | --- | --- |
| `preview_cleanup_plan` | in-memory plan build, tagged by owner for the response | `cleanup_plans.user_id` (on the eventual persisted plan; this tool never persists) | **Scoped.** Owner is now `ctx.principal`, never `req.user_id`. |
| `search_emails`, `get_email`, `list_recent_emails`, `count_emails`, `get_email_thread`, `list_attachments` | `emails` / `attachments` tables, scoped by `account_id` | None | **Not user-scoped, by design.** Mail belongs to a connected account, not a `user_id` — this engine has one principal and every connected account is theirs. Nothing to filter. |
| `get_insights`, `list_clusters` | clustering/analytics tables, scoped by `account_id` | None | **Not user-scoped, by design** — same reasoning. |
| `list_rules` | `rules` table | None | **Not user-scoped, by design** — rules apply per-account, not per-user. |
| `list_accounts`, `get_sync_status` | `connected_accounts` | None (confirmed: this table doesn't reference `user_id` at all) | **Not user-scoped, by design.** |
| `list_subscriptions` | subscription/sender aggregates, scoped by `account_id` | None | **Not user-scoped, by design.** |
| `find_similar_emails` | vector search over `emails` | None | **Not user-scoped, by design.** |
| `get_learning_metrics` | process-local in-memory counters (SONA learning engine), reset on restart | None — never persisted at all | **Not user-scoped: there is no row to own.** The metrics aren't written to any table, let alone a `user_id`-tagged one. |

No tool was left unscoped by omission — the fourteen above have no owner
column because the underlying data genuinely isn't user-scoped in a
single-principal engine (it's scoped by connected account, which is a
different axis this ADR doesn't touch), not because scoping them was skipped.

**Addendum (milestone 2):** `doctor`, `get_snapshot`, `brief`, `review`,
`identity_map` (`tools/staff`) were added after this ADR landed. All five
read `emails`/`connected_accounts`, the same account-scoped, no-`user_id`-
column tables as the row above — **not user-scoped, by design**, same
reasoning, no new row needed here.

## 4. Non-goals

- **No login, registration, or multi-user UI.** There is one principal. This
  ADR makes that principal *derived* instead of *asserted*; it does not make
  the engine multi-tenant.
- **No change to the REST layer's `user_id` source.** `get_plan`,
  `create_plan`, `cancel_plan`, `list_operations`, `sample_operations` all
  still take `user_id` from a query parameter at the REST boundary — exactly
  as `get_plan`/`create_plan` already did before this ADR. That boundary is
  just as caller-trusted as the MCP tool's was, and sharing the single
  `Principal::local_operator()` mechanism there too is a reasonable future
  step, but it's a separate decision with its own blast radius (every
  `cleanup/api/plan.rs` route, not three repository methods) — not folded into
  this ADR.
- **No rate-limiting or audit-trail change.** Keying `ToolRateLimiter` and the
  audit trail per-caller instead of globally (`pl-tool-rate-limit-not-per-caller`)
  is the retired `mcp-identity-authz` pipeline's phase 1, deliberately not
  built here.
- **No capability/scope model.** Letting a credential be granted one tool's
  write access without granting all of them is phase 2 of the same retired
  pipeline, deliberately not built here — there are no write tools yet.

## 5. Known follow-on, not fixed here

The retired `mcp-identity-authz` pipeline noted that its authorization tests
ideally run against the real dispatch path (`AppState`'s router) rather than
the replica handlers in `backend/tests/api_integration.rs`, because
authorization bugs are exactly the kind a replica test can miss. This ADR's
new tests (`cancel_rejects_a_plan_belonging_to_another_principal`,
`list_operations_and_sample_operations_reject_another_principal`) exercise the
real `SeaOrmCleanupPlanRepo` directly, which is the real implementation, not a
replica — so this gap does not apply to the tests added here. It remains
relevant for any future test that goes through `api_integration.rs` instead.
