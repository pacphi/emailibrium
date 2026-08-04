# ADR-033: PostgreSQL as a Second Database Backend, Selected by URL Scheme

- **Status:** Accepted
- **Date:** 2026-08-03
- **Deciders:** Chris Phillipson
- **Context:** `docker-compose.yml` has run a `postgres:16-alpine` service since early on, and `.github/workflows/smoke.yml` writes a `postgres://` URL into the `database_url` secret — both implying PostgreSQL support already existed. It didn't: `backend/src/db/mod.rs` opened an unconditional `SqlitePoolOptions` connection regardless of the URL passed in, and an entrypoint secret-name bug meant the smoke test's `postgres://` URL never even reached the app — it silently exercised SQLite the whole time. `docs/deployment-guide.md`'s Database Strategy section was corrected (under the docs-accuracy-audit pipeline) to say PostgreSQL is "planned, not implemented," which read as a regression to the user, who believed it worked. This ADR is the decision record for making that belief true.

---

## 1. Problem Statement

Emailibrium needs a real, working, CI-verified second database backend (PostgreSQL) alongside the existing default (SQLite) — not a docker-compose service nobody talks to. Two additional constraints shape the decision:

- **Both backends stay first-class.** SQLite remains the default for local/native use (single-user, zero-ops); PostgreSQL is for multi-user/production deployments. Neither is being deprecated.
- **Choosing one must be convenient in every deploy/run mode** — native (`just dev` / `just dev-llm`), `just docker-up-dev`, and `just docker-up` — not just technically possible if you already know the right environment variable. This requirement came from the user directly while this pipeline was being promoted, and reshaped phases 3 and 4 of the implementation plan (see `.autopilot/pipeline.yml`, `feature_id: postgres-support`).

## 2. Decision

**Dispatch on the connection URL's scheme. `sqlite:...` selects SQLite; `postgres://...` or `postgresql://...` selects PostgreSQL. No separate feature flag or config key exists for this choice, and none should be added.**

`backend/config.yaml`'s single `database_url` key (overridable via `EMAILIBRIUM_DATABASE_URL` per the existing `Env::prefixed("EMAILIBRIUM_")` convention) is the one place this is configured, in every mode:

```yaml
database_url: 'sqlite:emailibrium.db?mode=rwc' # sqlite:<path>?mode=rwc | postgres://user:pass@host:5432/db
```

### 2.1 `Database` becomes an enum, not a bigger struct

```rust
pub enum Database {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}
```

`Database::connect(url)` inspects the scheme and opens the matching pool. This phase (phase 0 of the pipeline) introduces the enum and the dispatch; it does **not** yet migrate every call site in the crate onto it — see §2.3.

**Why an enum instead of a trait object (`Box<dyn SomeDbTrait>`):** sqlx's `Sqlite` and `Postgres` driver types don't share a query-building trait that erases the concrete pool type without losing sqlx's compile-time-checked query support (which this crate doesn't use today — see §2.2 — but shouldn't be foreclosed). An enum keeps both pool types concrete, is `Clone` for free (both `SqlitePool` and `PgPool` are cheap `Arc`-backed clones), and matches the two-backends-only, not-N-backends shape of this problem. A trait object would buy indirection this crate doesn't need.

### 2.2 Runtime queries stay runtime queries — no `sqlx::query!`/`query_as!` macros

The crate has zero uses of sqlx's compile-time-checked macros today (`grep -rn 'sqlx::query!\|sqlx::query_as!' backend/src` = 0) — every query is the runtime `query`/`query_as` + `.bind(...)` form. This ADR keeps it that way rather than adopting the checked macros now:

- The checked macros need a `DATABASE_URL` pointed at a live database _at compile time_, per-driver — introducing them now would mean two separate compile-time database requirements (SQLite and PostgreSQL) for one crate, which sqlx does not support cleanly for a dual-backend build.
- Runtime queries are what let phase 2 make a single call site portable across both backends (branching on `&self` where SQL dialect differs), which compile-time-checked queries — tied to one schema, one driver — would make significantly harder.

### 2.3 What stays SQLite-only for now (and why that's fine)

This phase is plumbing, not the full migration. Two things are true simultaneously and both are intentional:

- **The SQLite path is provably unchanged.** Existing tests pass unmodified; `Database::connect` picks the SQLite arm for every URL it always picked SQLite for before.
- **Most call sites in the crate don't yet accept a `Postgres`-backed `Database`.** ~250 call sites across 27 files read a `SqlitePool` out of a `Database` value via a bridging accessor, `Database::pool(&self) -> &SqlitePool`, which panics if called on a `Postgres` variant. This is a deliberate, temporary bridge: it lets phase 0 introduce the enum without forcing an atomic, all-at-once rewrite of every SQL call site in the same change (a wide-blast-radius edit better done, and reviewed, on its own — see phase 2 of the pipeline). This panic is not reachable on the one path that exists today, though: `main.rs` calls `run_migrations()` immediately after `connect()`, and — per the migration-portability gap below — that fails first, with a normal `sqlx::Error`, for any real `postgres://` URL. The panic becomes reachable once phase 1 makes migrations Postgres-portable but phase 2 hasn't yet migrated a given call site; it is a known, accepted gap for that window, not a silent one.
- **Migrations are not yet Postgres-portable, and the porting surface is larger than just `AUTOINCREMENT`/`PRAGMA`.** 6 of the 29 files in `backend/migrations/` use those two SQLite-only constructs (no direct PostgreSQL equivalent). But `grep -l BLOB backend/migrations/*.sql` finds 6 files needing `BYTEA` instead, and `grep -l "datetime('now')" backend/migrations/*.sql` finds 5 more needing `now()` — with some overlap across all three categories. Phase 1 owns the actual audit and count; this ADR only establishes that the surface exists and is non-trivial. `Database::run_migrations()` dispatches by backend already, but running it against a real PostgreSQL connection fails until phase 1 lands. The SQLite path is unaffected.

### 2.4 Convenience across every deploy/run mode (added mid-plan, see §Context)

Because URL-scheme dispatch is the _only_ selection mechanism, "which mode am I in" and "which database am I using" are orthogonal by construction — the same `EMAILIBRIUM_DATABASE_URL` value means the same thing whether it's set in a shell before `cargo run` (native), baked into `config/environments/config.*.yaml` (docker), or delivered via the `database_url` Docker secret. What phase 3 adds is _operational_ convenience on top of this: gating `docker-compose.yml`'s `postgres` service behind a compose profile (mirroring the existing `qdrant` service's opt-in pattern) so a SQLite-only docker deployment isn't forced to run a Postgres container it never talks to, and surfacing the toggle in `justfile`'s help text and `docs/deployment-guide.md` per mode. None of that changes this ADR's core mechanism — it just makes the mechanism visible and low-friction wherever the app runs.

## 3. Consequences

**Positive**

- One config variable, one selection mechanism, consistent across native/dev/prod — no mode-specific flag to keep in sync.
- Runtime queries preserve the flexibility phase 2 needs to make individual call sites dialect-aware without a parallel compile-time-checked query system per backend.
- The bridging accessor (`Database::pool()`) makes the phase-0/phase-2 split mechanically explicit in the type system: it's not just documentation, a `Postgres`-backed `Database` visibly panics at any call site phase 2 hasn't migrated yet, rather than silently misbehaving.

**Negative / costs**

- **A `postgres://` URL is not yet usable end-to-end.** Phase 0 alone lets you _connect_; phases 1–3 are required before the app actually works against PostgreSQL. Documented above and in the pipeline's phase dependency graph — not a surprise to whoever runs phase 1 next.
- **~250 mechanical call-site edits landed in this phase** (a field access, `db.pool`, becoming a method call, `db.pool()`) purely to keep the crate compiling against the new enum. These are syntactic only — no call site's behavior changed — but they touch 27 files, which is a wider diff than "phase 0 is plumbing" might suggest at a glance. The alternative (a struct with a public `pool: SqlitePool` field wrapping an internal enum) was considered and rejected: it would either duplicate state (a redundant field alongside the real enum) or require the exact same accessor-based edit anyway, without the type-level safety of an enum making the SQLite-only assumption explicit.

## 4. Alternatives Considered

- **A separate `EMAILIBRIUM_STORE_BACKEND`-style enum flag for the database** (mirroring the vector store's existing `store.backend: ruvector | memory | qdrant | sqlite` key). Rejected: it would let the URL and the flag disagree (e.g. a `postgres://` URL with a flag still saying `sqlite`), and it adds a second thing to configure per mode — directly against the "convenient in every mode" requirement. The URL already encodes the scheme; a flag would be redundant at best, contradictory at worst.
- **`sqlx::Any`, the driver-erasing pool type.** Rejected: `AnyPool` erases enough driver-specific type information that several existing query patterns (typed `fetch_one`/`fetch_all` with driver-specific row types) would need rework beyond what phase 0's scope justifies, and it doesn't remove the need to branch on dialect differences in SQL text — it only defers where that branching happens.
- **A full atomic migration of every call site in phase 0.** Rejected: ~250+ call sites across 27 files, several with real dialect-sensitive SQL (not just mechanical), is a wide-blast-radius change better reviewed as its own phase (phase 2) than folded into "the abstraction exists" plumbing.

## 5. References

- `backend/src/db/mod.rs` — the `Database` enum, `connect()`, `run_migrations()`, `pool()`.
- `backend/config.yaml` — the `database_url` key and its inline scheme documentation.
- `.autopilot/pipeline.yml` (`feature_id: postgres-support`) — the full phase plan (0: this ADR + abstraction; 1: migrations; 2: call-site migration; 3: CI + docker-compose profile + justfile docs; 4: deployment docs).
- `docs/deployment-guide.md` — operator-facing setup (updated in phase 4).
- `docs/ADRs/ADR-032-make-to-just-task-runner.md` — the `just` task runner this ADR's phase 3 extends with the docker-compose profile toggle.
