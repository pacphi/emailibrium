# ADR-031: Multi-Tenancy Groundwork

- **Status:** Proposed
- **Date:** 2026-06-08
- **Deciders:** Chris Phillipson
- **Context:** Emailibrium today is a single-operator application: one person runs it on their own hardware and connects several of their own mailboxes (Gmail/Outlook via OAuth, others via IMAP). The near-term distribution model is open source — others run their own instance on their own hardware. A **hosted, multi-user** offering is a plausible future path that would require real per-user isolation, authentication, and per-tenant key management. This ADR records the design for that path **without implementing it yet**, so the eventual migration is additive and de-risked rather than a rewrite.

---

## 1. Problem Statement

There is no notion of a user/tenant in the codebase:

- **No `users` table**, no authentication middleware, no identity extractor. All `/api/v1` routes are unauthenticated.
- `connected_accounts.email_address` is **globally `UNIQUE`** — two users could never connect the same address.
- Account/data queries are scoped by `account_id` only — roughly **180+ query sites** across ~9 account-scoped tables (`connected_accounts`, `emails`, `sync_state`, `ingestion_checkpoints`, `sync_queue`, `processing_checkpoints`, `attachments`, `cleanup_plan_*`, `cleanup_audit_log`) carry no owner filter.
- Several tables are **globally shared** with no scoping at all: `topic_clusters`, `rules`, `category_centroids`. In a multi-user world these would leak one user's data/behavior into another's.
- Token encryption uses a **single global master key** (`EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD`) with a fixed salt.

Doing full multi-tenancy now is multi-week work that would change behavior for the only current user (one operator). The decision is therefore: **lay a thin, forward-compatible seam now (design only), implement when hosting is real.**

> Precedent: the cleanup subsystem (ADR-024/025) and per-user learning (migration 007) already use a `user_id` column. Multi-tenancy should converge on that pattern rather than invent a new one.

---

## 2. Decision

Adopt an **owner-scoped** model centered on a `users` table and an `owner_id` foreign key, rolled out in phases. The local/self-hosted deployment runs as a **single bootstrapped default user**, so single-operator behavior is unchanged and zero-config; the hosted path later adds real authentication and more users on the same schema.

### 2.1 Identity: bootstrapped default user

Introduce a `users` table:

```sql
CREATE TABLE users (
    id          TEXT PRIMARY KEY NOT NULL,   -- UUID
    email       TEXT UNIQUE,                 -- login identity (hosted); NULL/local sentinel for self-hosted
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

- On startup, **ensure exactly one default user row exists** for self-hosted mode. Its id is stable: read `EMAILIBRIUM_OWNER_ID` if set, else generate a UUID once and persist it (e.g. in `app_settings`). All existing accounts/data backfill to this id.
- Hosted mode simply creates additional `users` rows + real credentials; nothing else about the schema changes.

### 2.2 Ownership: `owner_id` on account-rooted tables

- Add `owner_id TEXT NOT NULL REFERENCES users(id)` to `connected_accounts` (and, in later phases, to the other account-rooted tables OR rely on a join through `connected_accounts.owner_id`).
- **Replace** the global `UNIQUE(email_address)` with `UNIQUE(owner_id, email_address)` — two users may connect the same address; one user still can't connect it twice.
- Prefer **scoping data tables by joining through `connected_accounts.owner_id`** (since `emails`, `sync_state`, etc. already carry `account_id`) rather than duplicating `owner_id` onto all 9 tables. This keeps the migration small and the isolation key single-sourced. Duplicate `owner_id` only where a hot query can't afford the join.

### 2.3 Global tables (the real isolation hazard)

`topic_clusters`, `rules`, `category_centroids` are singletons today. Options, in preference order:

1. **Scope by `owner_id`** (add the column + filter). Correct and simplest to reason about; each user gets their own clusters/rules/centroids. Recommended.
2. Keep vectors global but gate **access** through owner-scoped account membership (only viable if vectors are never themselves sensitive — they encode email content, so they are; rejected for hosted).

Decision: in the hosted phase these tables get `owner_id` and all reads/writes filter on it. For self-hosted (single user) they continue to work unchanged because every row shares the one owner.

### 2.4 Authentication seam

- Add a `CurrentUser` Axum extractor. In self-hosted mode it resolves to the bootstrapped default user with **no auth required** (env-gated, e.g. `EMAILIBRIUM_AUTH=disabled`). In hosted mode it validates a session/JWT and resolves the real user.
- Handlers take `CurrentUser` and pass `owner_id` into the data layer. Until hosted mode exists, the extractor is a no-op that returns the default user — so adding it is non-breaking.
- Auth middleware (session/JWT validation, rate-limit-per-user) is added only in the hosted phase; the existing global rate-limit and security-header middleware stay.

### 2.5 Encryption / key management

- Today: one Argon2id-derived key from a global master password + fixed salt.
- Hosted: derive a **per-user key** (e.g. salt = `TOKEN_KEY_SALT || owner_id`, or a per-tenant key in a KMS with versioning), so a compromise/rotation is scoped to one tenant. The existing `derive_key(password, salt)` already accepts a custom salt, so this is an additive change in `OAuthManager`.
- Self-hosted single-user keeps the current global key (the user owns the box anyway).

---

## 3. Phased Rollout

| Phase                 | Scope                                                                                                                                                                                                                  | Trigger                                       |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| **0 (this ADR)**      | Design only. No schema/code change.                                                                                                                                                                                    | now                                           |
| **1 — Identity seam** | `users` table + bootstrapped default user + `CurrentUser` no-op extractor + `owner_id` on `connected_accounts` (backfilled to default user) + `UNIQUE(owner_id, email_address)`. Non-breaking for the single operator. | First step toward hosting, or when convenient |
| **2 — Query scoping** | Thread `owner_id` through account create/list/disconnect and the ~180 data queries (join through `connected_accounts`).                                                                                                | Hosting committed                             |
| **3 — Global tables** | `owner_id` on `topic_clusters`, `rules`, `category_centroids` + filters.                                                                                                                                               | Hosting committed                             |
| **4 — Auth + keys**   | Real auth middleware + JWT/session, per-user key derivation, per-user rate limits, re-encrypt token cache.                                                                                                             | Before any real multi-user traffic            |

Each phase is independently shippable and leaves the single-operator deployment working.

## 4. Consequences

- **Positive:** the seam (a `users` table + `owner_id` + a no-op `CurrentUser`) is small and non-breaking, yet every later phase becomes additive. Converges on the `user_id` pattern the cleanup subsystem already uses.
- **Negative / risk:** Phase 2 touches many query sites — must be done with the join-through-`connected_accounts` strategy and good test coverage to avoid cross-tenant leakage. The `UNIQUE` constraint change requires a table rebuild on SQLite (no `DROP CONSTRAINT`).
- **Security:** the SSRF guard, generic error mapping, and IMAP fail-closed behavior already added are deployment-model-agnostic and remain valid under multi-tenancy. Loopback is currently allowed in the SSRF guard for ProtonMail Bridge — **hosting must disable loopback** (it would let one tenant reach the host's local services). Track this as an explicit Phase 4 gate.

## 5. Out of Scope

Implementation of any phase. This ADR only fixes the target design so future work is incremental. The IMAP transport-security work (SSL/STARTTLS/plaintext, ADR-noted in the same change set) is independent of this and already shipped.
