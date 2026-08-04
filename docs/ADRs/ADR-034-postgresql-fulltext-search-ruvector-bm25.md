# ADR-034: PostgreSQL Full-Text Search via ruvector-postgres's BM25, Not tsvector

- **Status:** Accepted (design decision only — implementation lands in the postgres-support pipeline's phase 5, not this ADR)
- **Date:** 2026-08-03
- **Deciders:** Chris Phillipson
- **Context:** ADR-033 makes PostgreSQL a real second database backend. Three of the 29 migrations (`005_fts5_search.sql`, `019_fts5_add_from_name.sql`, `020_fts5_bm25_weights.sql`) build SQLite's FTS5 full-text index — virtual tables, sync triggers, and a hand-tuned 6-column BM25 weight vector (subject×10, from_name×5, from_addr×3, body_text×1, labels×2 — ADR-029). **SQLite FTS5 has no PostgreSQL equivalent**; this ADR decides what full-text search on the PostgreSQL backend uses instead.

## 1. Problem Statement

Postgres's native full-text search (`tsvector`/`tsquery` + GIN index + `ts_rank`/`ts_rank_cd`) is not a drop-in replacement for FTS5 — it uses a materially different, and by most accounts weaker, relevance-ranking algorithm than BM25 (the algorithm FTS5 uses). Reproducing this pipeline's own DoD goal — "producing a schema equivalent to the SQLite one" — for full-text search specifically means finding a way to get real BM25 ranking on Postgres too, not just _a_ keyword search.

## 2. Decision

**Use `ruvector-postgres`'s `ruvector_bm25_score()` SQL function for PostgreSQL full-text ranking, not `tsvector`/`ts_rank`.** Considered and rejected: ParadeDB's `pg_search` (also true BM25, via Tantivy) and plain `tsvector`+GIN+`ts_rank_cd`.

### 2.1 Why `ruvector-postgres` over the alternatives

|                             | `tsvector`+`ts_rank_cd`         | ParadeDB `pg_search`                               | `ruvector-postgres`                                                                                                                                              |
| --------------------------- | ------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Ranking algorithm           | Native Postgres, not BM25       | True BM25 (Tantivy)                                | True BM25 (`ruvector_bm25_score`)                                                                                                                                |
| License                     | PostgreSQL License (permissive) | **AGPL-3.0** (community); enterprise features paid | **MIT**                                                                                                                                                          |
| New third-party dependency? | No                              | Yes                                                | No — already vendored (`ruvector/crates/ruvector-postgres/`), same MIT-licensed ecosystem this project already depends on for its primary vector store (ADR-003) |
| Extension install           | None (core Postgres)            | pgrx extension, custom image                       | pgrx extension, custom image                                                                                                                                     |

`ruvector-postgres` wins on the two criteria that matter most here: it's the only true-BM25 option that introduces **zero new licensing surface** (MIT, same as the rest of RuVector this project already ships) and **zero new third-party codebase** to vet, track for CVEs, or reconcile with this project's existing ADR-003 commitment to RuVector as the vector store. ParadeDB's `pg_search` is a credible, actively-maintained alternative and would be the right call if `ruvector-postgres` turned out not to fit — but it adds a genuinely new dependency with a copyleft license this project doesn't otherwise carry, for a capability RuVector already provides in-house.

Plain `tsvector` was rejected on ranking-quality grounds alone: it's zero-dependency and would have been the pragmatic choice if BM25 parity weren't a goal, but it does not reproduce FTS5's ranking behavior, and this pipeline's phase 1 goal is explicitly schema _equivalence_, not just _a_ working keyword search.

### 2.2 What `ruvector_bm25_score()` actually is, and what that implies

It is a **scoring primitive**, not a managed index: `ruvector_bm25_score(terms, doc_freqs, doc_len, avg_len, total)` computes the BM25 formula given term/document frequencies the caller supplies. It does not automatically maintain an inverted index the way FTS5's `content='emails'` external-content table + `AFTER INSERT/UPDATE/DELETE` triggers do today. Using it for real requires the application (or a companion Postgres-side structure) to maintain term frequencies, document frequencies, and average document length as emails are ingested, updated, and deleted — comparable maintenance burden to what FTS5's triggers already handle, just not automatic.

### 2.3 Where this lands, and why not now

This ADR is a **design decision**, not an implementation. The actual adoption — installing `ruvector-postgres` (a custom Postgres Docker image is required; Ruvnet publishes prebuilt images, easing but not eliminating this), building the term/document-frequency maintenance path, and wiring query-time BM25 scoring into the search pipeline — is deliberately its own phase (**phase 5**), not folded into phase 1 or phase 3, for two reasons:

- It depends on phase 3's compose-profile pattern existing first (the vanilla `postgres:16-alpine` service becomes profile-gated in phase 3; phase 5 then swaps that same gated service to a `ruvector-postgres` image rather than touching the service definition twice across unrelated phases), but is substantial enough — extension install, index-maintenance design, query-path wiring — to be its own independently reviewable, independently shippable unit rather than a rider on phase 3's CI/infra work.
- Phase 1's own goal — get the _relational_ schema (the other 26 migrations) running cleanly on Postgres — doesn't depend on full-text search working yet. Blocking phase 1 on designing an index-maintenance strategy for BM25 would conflate two genuinely separable concerns.

**Interim state (phases 1–4, until phase 5 lands):** a PostgreSQL-backed deployment has no full-text keyword search. Semantic/vector search via RuVector's core HNSW engine is unaffected — it doesn't depend on which SQL backend stores relational data (per ADR-003, RuVector's vector store is independent of `EMAILIBRIUM_DATABASE_URL`). This is a known, explicit, temporary gap, not a silent one — see ADR-033 §2.3 for the same pattern applied to the connection-layer bridge.

## 3. Consequences

**Positive**

- No new third-party dependency or license to reconcile with this project's existing MIT-licensed RuVector commitment.
- True BM25 ranking on Postgres, not a lesser approximation — genuine schema/capability equivalence with SQLite FTS5, not just "search that works."

**Negative / costs**

- `ruvector_bm25_score()` is lower-level than FTS5's automatic index — phase 5 (or a follow-up) must design and build term/document-frequency maintenance, which is real, non-trivial work, not a config flag.
- Requires a custom Postgres Docker image rather than vanilla `postgres:16-alpine` — a real, if manageable (prebuilt images exist), operational cost for anyone deploying the Postgres backend.
- Full-text search is unavailable on the Postgres backend for two full phases (1–2) of this pipeline. Must be documented clearly wherever Postgres deployment is described (phase 4 docs) so this isn't discovered as a surprise.

## 4. Alternatives Considered

- **ParadeDB `pg_search`** — true BM25, actively maintained, would work. Rejected on license grounds (AGPL-3.0 community edition vs. this project's all-MIT dependency graph) and because it introduces a wholly new codebase to track when RuVector already provides equivalent capability.
- **Plain `tsvector`/GIN/`ts_rank_cd`** — zero new dependencies, zero extra infrastructure (works with vanilla Postgres). Rejected because it doesn't reproduce BM25 ranking — a materially different (and by Postgres's own extension ecosystem's admission, weaker) relevance algorithm than what this pipeline's phase 1 goal (schema equivalence) implies. Remains the fallback if `ruvector-postgres` adoption in phase 5 proves impractical.
- **Defer full-text parity indefinitely (no Postgres keyword search, ever)** — rejected; the goal is a _real_ second backend, and keyword search is part of this app's existing hybrid search architecture (ADR-001), not an optional extra.

## 5. References

- `ADR-033-postgresql-backend-support.md` — the connection-layer decision this ADR builds on.
- `ADR-001` — Hybrid Search Architecture (the vector+keyword search this decision restores parity for).
- `ADR-029` — the FTS5 BM25 column-weight tuning this ADR cannot exactly reproduce on Postgres.
- `ruvector/crates/ruvector-postgres/` — the vendored pgrx extension (`ruvector_bm25_score`, `docs/SQL_FUNCTIONS_REFERENCE.md`).
- `.autopilot/pipeline.yml` (`feature_id: postgres-support`) — phase 1 (this ADR's immediate scope exclusion) and phase 5 (where the adoption work lands).
- [ParadeDB: Why We Picked AGPL](https://www.paradedb.com/blog/agpl) — the licensing rationale this ADR weighed against.
