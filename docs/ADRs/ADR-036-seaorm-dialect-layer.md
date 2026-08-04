# ADR-036: SeaORM as the Dialect Layer — Delete the Hand-Rolled Backend Dispatch

- **Status:** Accepted
- **Date:** 2026-08-03
- **Deciders:** Chris Phillipson
- **Supersedes:** ADR-035's _implementation strategy_ (runtime placeholder translation + per-backend match dispatch). ADR-035 **remains authoritative as the dialect-divergence catalog** — its §2.3–§2.7 findings are the requirements this ADR's solution was validated against.
- **Related:** ADR-033 (dual-backend decision, URL-scheme dispatch — unchanged), ADR-034 (FTS via ruvector-postgres BM25 — unchanged, now implemented through this ADR's escape-hatch pattern).

## 1. Context

Phase 2 of the postgres-support pipeline set out to migrate ~22 call sites onto the
`Database` enum with a runtime placeholder translator (ADR-035). Fifteen files in,
the empirical picture had changed materially:

- The true surface was **51 files**, not 22.
- Four _additional_ divergence classes were discovered mid-flight, each producing
  real bugs or silent behavior drift caught only by live-Postgres testing:
  INT4-vs-i64 decode widths (§ADR-035), `datetime('now')` writes into TEXT-typed
  timestamp columns (§2.5), plain-`TIMESTAMP` columns requiring `NaiveDateTime`
  decode (§2.6), and `avg(integer)` returning `NUMERIC` on Postgres (§2.7).
- Every converted call site carried a two-arm `match Database::Sqlite/Postgres`
  around its terminal execute/fetch, plus `adapt()`/`audited_sql()` plumbing —
  hand-rolled custom code growing linearly with the codebase, forever.

The user's directive: **complete minimization of custom dialect code**, with carte
blanche to reimagine the architecture if it simplifies backend swapping.

## 2. Decision

Adopt **SeaORM 2.0** as the dialect layer. Concretely:

1. **`sea_orm::DatabaseConnection` replaces the `Database` enum as the handle every
   repository/service holds.** It is itself a backend-dispatching wrapper over a
   sqlx pool — the same design we hand-rolled, maintained upstream. The only
   backend `match` that survives is one arm at the composition root wrapping the
   pool `Database::connect()` creates (URL-scheme dispatch per ADR-033 unchanged).
2. **Entities are the single source of truth for Rust-side types.** `seq: i32`,
   ids as `Vec<u8>`, timestamps by actual column type — declared once; SeaORM's
   value layer owns encode/decode per backend. ADR-035's decode-width bug class
   becomes unrepresentable.
3. **Query construction goes through SeaORM/sea-query** — placeholders, quoting,
   `OnConflict` upserts, dynamic filters, pagination, aggregates: one code path.
4. **SQL-side JSON mutation is eliminated, not translated**: `json_set`/`jsonb_set`
   call sites become read-modify-write inside a transaction (spike-verified).
   **Known tradeoff:** the single atomic UPDATE those functions rode in had no
   lost-update window; the read-modify-write does (plain SELECT, no row lock,
   READ COMMITTED on PostgreSQL). It is sound under the codebase's actual write
   topology — each `(plan_id, seq)` row has exactly one writer (one apply worker
   per account; a row's account owns its seq) — and that invariant is documented
   at the ported call sites. Any future call site with concurrent same-row
   writers must take a row lock first (`lock_exclusive()`, i.e. `SELECT … FOR
UPDATE` on PostgreSQL; SQLite serializes writers anyway).
5. **A narrow raw-SQL escape hatch remains**, via
   `Statement::from_sql_and_values(conn.get_database_backend(), ...)`, for the
   irreducible cases only: aggregate casts (`AVG(...)::float8`, §2.7) and the
   per-backend FTS implementations (FTS5 vs `ruvector_bm25_score()`, ADR-034).
   Escape hatches live behind the same repository traits as everything else.

### Out of scope here, queued as a follow-up pipeline (`db-schema-modernization`)

Schema debt that _causes_ dialect divergence (TEXT-typed timestamp columns,
JSON-arrays-as-TEXT, status embedded in `payload_json`) and adoption of
`sea-orm-migration` for future migrations. SeaORM maps today's schema as-is;
normalizing it is separable, data-migration-sensitive work.

## 3. Spike evidence (2026-08-03)

A throwaway spike (`backend/examples/spike_seaorm.rs`, deleted when the real port
lands) ported the hardest operations from `cleanup/repository/plan_repo.rs` and ran
the identical scenario against both an on-disk SQLite database and a live
`postgres:16-alpine` container, **through the pools our existing
`Database::connect()` created** — proving legacy sqlx code and SeaORM can share one
pool during incremental adoption. All 11 checks passed on both backends:

| #   | Check                                                                                      | Result |
| --- | ------------------------------------------------------------------------------------------ | ------ |
| 1   | Single `sqlx v0.9.0` in the dependency tree (sea-orm 2.0.1 unifies with ours)              | PASS   |
| 2   | Wrap existing sqlx pools (`SqlxSqliteConnector`/`SqlxPostgresConnector::from_sqlx_*_pool`) | PASS   |
| 3   | `Vec<u8>` (16-byte UUID) primary keys, including composite `(Vec<u8>, i32)`                | PASS   |
| 4   | One-code-path `ON CONFLICT` upsert                                                         | PASS   |
| 5   | One-code-path transactions                                                                 | PASS   |
| 6   | Optional/dynamic filters + seq-cursor pagination                                           | PASS   |
| 7   | `MAX(seq)` aggregate decoding to `i32`                                                     | PASS   |
| 8   | `json_set`/`jsonb_set` divergence eliminated via read-modify-write                         | PASS   |
| 9   | `update_many` + `rows_affected`                                                            | PASS   |
| 10  | Per-backend raw-SQL escape hatch (`AVG` cast; the FTS pattern)                             | PASS   |
| 11  | Repository fns generic over `ConnectionTrait` (work with connection _or_ transaction)      | PASS   |

Total integration friction encountered: two one-line API renames
(`query_one(&stmt)` → `query_one_raw(stmt)` for raw statements in 2.0).

## 4. Alternatives considered

- **Finish the hand-rolled approach (ADR-035) for the remaining ~35 files** —
  rejected. It writes the dual-backend layer by hand across ~50 files, and any
  later ORM adoption rewrites them all a second time. Adopting now converts the 15
  finished files once more but the 35 remaining files only once.
- **sea-query alone (builder, no ORM)** — rejected as insufficient: it solves the
  SQL-_text_ half (placeholders, upserts, DDL) but not the decode half — and the
  decode half produced every real bug found in phase 2.
- **`sqlx::Any`** — already rejected in ADR-035 §2.2; the typed-decode erasure
  makes the decode problem worse.
- **Diesel (+diesel-async)** — rejected: multi-backend code requires generics over
  `Backend` at every boundary (more ceremony than the match-arms it replaces), and
  its migrations are raw SQL — the dual-directory problem stays.
- **welds** — closest in spirit, but a much smaller community/ecosystem than
  SeaQL's; not worth the risk delta for a privacy-critical local-first store.

## 5. Consequences

**Positive**

- The per-call-site custom dialect code (match-arms, `adapt()`, `audited_sql()`
  wrapping, hand-written per-backend SQL pairs) is deleted rather than completed.
  `Database` shrinks to: URL dispatch, migrations, and one composition-root wrap.
- ADR-035's divergence classes — upserts (a genuinely-different-SQL-text case
  its §2.3 translation algorithm cannot cover), §2.5/§2.6 (timestamp handling at
  the decode layer), and the width class become library-owned instead of
  convention-owned ("check the migration DDL before choosing a decode type" is no
  longer a human rule).
- The FTS phase (ADR-034) gets a clean, already-proven pattern (spike check #10).

**Negative / costs**

- The 15 hand-converted files are rewritten once more (their bug fixes — correct
  widths, correct timestamp semantics — carry directly into entity definitions;
  ADR-035's catalog was effectively the requirements doc for this adoption).
- A significant new dependency (sea-orm + sea-query + macros). Mitigations: it
  builds on the sqlx we already ship (verified single-version), is 100%
  `forbid(unsafe_code)`, and is trimmed to the features we use.
- `SqlSafeStr`-style audit ergonomics change: SeaORM builds SQL from typed ASTs,
  so injection surface shifts from "audit every dynamic string" to "audit the few
  raw escape hatches" — a smaller, enumerable set.

## 6. References

- `backend/examples/spike_seaorm.rs` — the spike (temporary; this ADR records its results).
- ADR-033, ADR-034, ADR-035 — prior decisions this builds on.
- SeaORM 2.0 migration guide; sea-query repository. (Verified 2026-08-03: sea-orm
  2.0.1 depends on sqlx 0.9, matching `backend/Cargo.toml`.)
