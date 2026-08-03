# ADR-035: One Query String Per Call Site, Translated at Runtime — Not Duplicated Per Backend

- **Status:** Accepted
- **Date:** 2026-08-03
- **Deciders:** Chris Phillipson
- **Context:** ADR-033's phase 2 — migrating every call site off the temporary `Database::pool()` bridge onto the `Database` enum directly — turned out to be a materially bigger problem than "swap the pool type." SQLite and PostgreSQL do not share bind-parameter syntax: SQLite accepts bare `?` and explicit `?N`; PostgreSQL requires `$N`. sqlx's runtime `query()`/`query_as()` API does not translate between them — the placeholder style is baked into the SQL text itself, and the wrong style is a runtime error, not a compile error. A repo-wide audit found **~250 bind-parameter occurrences across roughly 84 query strings**, and separately **41 inline `datetime('now')` calls across 13 files** (a SQLite-specific function with no PostgreSQL equivalent — same problem class phase 1 already solved for migration files, just showing up again in application-code query strings), across the true call-site surface: **51 files** touch `SqlitePool` directly (either typed on a struct field or via the `.pool()` bridge) — more than double the pipeline's original 22-file estimate, since several structs hold their own `pool: SqlitePool` field populated by a `.clone()` from `main.rs` rather than calling `.pool()` themselves.

## 1. Problem Statement

Every one of those ~84 query strings needs to produce PostgreSQL-compatible placeholder syntax, and every `datetime('now')` call needs to become `now()`, when running against a `Postgres`-backed `Database`, without regressing the SQLite path. Three ways to get there, each with a real cost:

1. **Hand-duplicate every query per backend** — write two full query strings (and often two `.bind()` chains) at each call site.
2. **A runtime translation helper** — author each query once, SQLite-style, and rewrite it to Postgres style automatically when needed.
3. **Reopen ADR-033 and adopt `sqlx::Any`** — sqlx's driver-erasing pool type performs placeholder translation for you (though not the `datetime('now')` function-call problem, which is orthogonal to which pool type you use).

## 2. Decision

**A runtime translation helper, `Database::adapt(&self, sql: &str) -> Cow<'_, str>`, in `backend/src/db/mod.rs`.** Every call site keeps exactly the query string it has today (SQLite-style `?`/`?N` placeholders, `datetime('now')` where needed); `adapt()` returns it unchanged for `Database::Sqlite` and, for `Database::Postgres`, rewrites `datetime('now')` → `now()` first, then placeholders to `$1, $2, …`.

### 2.1 Why not hand-duplicate every query

~84 query strings, several with 10+ bind parameters (e.g. `cloud_api_audit_log`'s 10-column `INSERT`), would become ~168 strings to keep in sync by hand across every future schema change — the exact "two things to keep in sync" failure mode ADR-033 already avoided at the migration-directory level by using matching filenames. A shared translator turns an O(query count) maintenance burden into an O(1) one, verified by one focused test suite instead of eyeballing 84 pairs for drift.

### 2.2 Why not reopen ADR-033 and adopt `sqlx::Any`

Revisited given this new evidence, not dismissed reflexively. `AnyPool`'s automatic placeholder translation is a genuine, relevant point in its favor that ADR-033 didn't weigh (it rejected `Any` purely on typed row/column grounds). But the conclusion doesn't change: `Any` erases enough driver-specific type information that the existing `query_as::<_, T>()` typed-decode call sites — including the ones already identified as needing real `String → DateTime<Utc>` decode fixes for the `TIMESTAMPTZ` columns from phase 1 — would need *more* rework under `Any`, not less, because `Any`'s row type has weaker compile-time type-checking than a concrete `Sqlite`/`Postgres` row. Solving the placeholder problem while making the typed-decode problem harder is not a net win. The translator solves the one real problem `Any` would have solved, without taking on the one real problem it would have made worse.

### 2.3 The translation algorithm

For `Database::Postgres`, `adapt()` first does a plain substring replace of `datetime('now')` → `now()` (safe as a straight substitution here — every occurrence in this codebase is a function call in the query's SQL structure, never literal data inside a bound value or string), then runs a single-pass scanner over the result that tracks whether it is inside a single-quoted string literal (honoring `''` as an escaped quote, the SQL standard) or a double-quoted identifier, and only rewrites `?` outside both:

- `?N` (already explicitly numbered, e.g. `?1`, `?2`) → `$N` — a direct, order-preserving translation; SQLite's and Postgres's numbered-parameter semantics are identical (the Nth bound value), just spelled differently.
- Bare `?` → `$<k>` where `k` is a per-call sequential counter (the order the bare `?` occurrences appear in the string) — matching how SQLite itself treats consecutive bare `?`s as sequential parameters.
- `Database::Sqlite` → returns the input unchanged (`Cow::Borrowed`), so the SQLite path has zero allocation and zero behavior change from before this phase.

Mixing bare and numbered placeholders in the *same* query string is not attempted by any call site in this codebase today and is not a case this translator is designed to make sensible — each query picks one style, matching existing convention.

### 2.4 The call-site pattern this enables

A function that used to take `pool: &SqlitePool` takes `db: &Database` instead. Its body computes the adapted SQL once (`let sql = db.adapt("...");`) and then dispatches the bind/execute chain per backend:

```rust
pub async fn update_email_state(db: &Database, email_id: &str, /* … */) -> Result<u64, sqlx::Error> {
    let sql = db.adapt("UPDATE emails SET is_trash = ?1, is_spam = ?2, folder = ?3, deleted_at = ?4 WHERE id = ?5");
    let result = match db {
        Database::Sqlite(pool) => sqlx::query(&sql).bind(is_trash).bind(is_spam).bind(folder).bind(deleted_at).bind(email_id).execute(pool).await?,
        Database::Postgres(pool) => sqlx::query(&sql).bind(is_trash).bind(is_spam).bind(folder).bind(deleted_at).bind(email_id).execute(pool).await?,
    };
    Ok(result.rows_affected())
}
```

The `.bind()` chain is written once per match arm (sqlx's builder type is concretely `Sqlite`- or `Postgres`-flavored once bound to a pool, so it cannot be shared across both in a single expression without a generic function) — but the *query text* and the *business logic* are authored exactly once. This is the same "duplicate only the unavoidable, share everything else" shape ADR-033's migration-directory split already uses.

### 2.5 The `datetime('now')` substitution is unsafe when the target column is TEXT, not TIMESTAMPTZ

Discovered mid-phase, after `content/jobs.rs` had already shipped through `adapt()`'s substitution: several tables (`background_jobs`, `processing_checkpoints`, `connected_accounts`, `sync_state`) deliberately keep their `created_at`/`updated_at`/`completed_at` columns as **TEXT** in the PostgreSQL migrations too — not `TIMESTAMPTZ` — specifically because "downstream Rust code parses these columns as plain strings in that exact format" (verbatim from the migration comments; see e.g. `migrations/postgres/004_accounts.sql`, `006_ingestion_checkpoints.sql`). Their column `DEFAULT` uses `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')`, producing the same `'YYYY-MM-DD HH:MM:SS'` shape SQLite's `datetime('now')` produces — not bare `now()`.

`adapt()`'s blind `datetime('now')` → `now()` substitution doesn't know a given occurrence targets one of these TEXT columns. Confirmed via `psql`: assigning `now()` (a `timestamptz`) into a TEXT column does **not** error — Postgres has a general assignment-cast to `text` from any type via its output function — but the resulting string is shaped like `2026-08-03 22:31:37.104356+00` (fractional seconds, `+00` offset), not `2026-08-03 22:31:37`. This is a **silent format drift**, not a crash: `content/jobs.rs`'s `updated_at`/`completed_at` are plain `String` fields with no downstream parsing, so nothing broke loudly, but the two backends now write differently-shaped strings for the same logical value — exactly the cross-backend behavioral parity this whole phase exists to guarantee. (For a column that *is* parsed back into a `DateTime`, e.g. `email/oauth.rs`'s `connected_accounts.created_at`/`updated_at`, the drift is worse: the parse falls through both the RFC3339 attempt and the `%Y-%m-%d %H:%M:%S` fallback and the row is silently dropped by a `filter_map`.)

**Fix:** any call site whose target column is TEXT-with-a-`datetime('now')`-shaped-default must **not** rely on `adapt()`'s substitution for that literal — hand-write the Postgres arm's SQL with `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')` in place of `datetime('now')`, matching §2.4's two-arm pattern (this is a "genuinely different SQL text" case in the same bucket as `INSERT OR REPLACE`/`json_set` from `plan_repo.rs`, not a case `adapt()` can cover mechanically). `adapt()`'s substitution remains correct and usable only when the target is a genuine `TIMESTAMPTZ` column (none of the call sites converted so far bind a bare `datetime('now')` SQL literal against one — those columns are instead always populated by binding an explicit `DateTime<Utc>` Rust value, e.g. `rules.created_at`).

**Action taken:** `content/jobs.rs`'s five affected call sites (`dequeue`, `mark_completed`, `mark_failed`, `cancel`, `resume_abandoned`) were fixed to hand-write the Postgres arm. Every subsequent call site touching a TEXT-shaped timestamp column follows this pattern from the start rather than reaching for `adapt()`.

## 3. Consequences

**Positive**

- One query string per call site, matching what's there today — the phase-2 migration is additive (adapt + branch-on-execute), not a rewrite of ~84 query strings by hand.
- `adapt()`'s correctness is verified once, by a dedicated unit test suite (quoted-string-literal edge cases, mixed numbered/bare within different queries, no-op on the SQLite path), rather than trusted implicitly at 84 call sites.
- Keeps ADR-033's typed-decode call sites viable — doesn't reopen the row-type-erasure cost `Any` would have imposed.

**Negative / costs**

- A hand-rolled SQL scanner, however small and tested, is still hand-rolled parsing logic — a genuinely malformed edge case (e.g. a query string with unbalanced quotes) would misbehave. Mitigated by the fact that every query string in this codebase is a static literal authored by the codebase itself, never user input — the translator's input space is bounded and enumerable, not adversarial.
- Every migrated call site still has the two-arm `match` for the terminal execute/fetch call — not zero duplication, just far less than duplicating full query text.
- The `datetime('now')` substitution is a mechanical text replace with no awareness of the target column's actual type — §2.5 documents the TEXT-vs-TIMESTAMPTZ footgun this creates and requires each call site to be checked against its migration, not assumed safe.

## 4. Alternatives Considered

- **Hand-duplicate every query per backend** — rejected; ~84 query strings become ~168 to keep in sync, for a benefit (no shared translation logic) that doesn't outweigh the ongoing maintenance cost.
- **`sqlx::Any`** — reconsidered given the placeholder-translation benefit, still rejected; makes the already-identified typed-decode gap (phase 1's `TIMESTAMPTZ` columns) worse, not better.
- **A query-builder crate (e.g. `sea-query`)** — would solve both the placeholder and (partially) the typed-decode problem, but is a materially larger dependency and rewrite than this phase's scope justifies; revisit only if the hand-rolled translator proves insufficient in practice.

## 5. References

- `backend/src/db/mod.rs` — `Database::adapt()` and its test suite.
- `docs/ADRs/ADR-033-postgresql-backend-support.md` §2.1 — the enum-vs-trait-object and runtime-vs-compile-time-checked-query decisions this ADR builds on.
- `.autopilot/pipeline.yml` (`feature_id: postgres-support`, phase 2) — the call-site migration this ADR unblocks.
