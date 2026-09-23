# qe-court record — postgres-support, phase 2 (SeaORM foundation + exemplar ports)

- **Delivery:** `git diff 05036c9..HEAD -- backend/` on branch `autopilot/postgres-support/phase-2`
  (SeaORM 2.0 dialect layer per ADR-036: entities, `Database::sea_orm()`, plan/job repo re-ports,
  spike graduation, plus the 11 inherited pre-re-plan hand-conversion wip commits).
- **Convened:** 2026-08-03, per `pipeline.court: auto` + phase 2 ∈ `risk_phases`.
- **Config:** `.claude/skills/qe-court/config.json` (2-vendor panel: Claude writer/defense-side seats,
  Codex/GPT grading seats; `writerIsNeverJuror`, `blindFiling`, `minDistinctVendors: 2`, `overturnDepth: 2`).

## Panel

| Seat                        | Route                | Status                                   | Charges |
| --------------------------- | -------------------- | ---------------------------------------- | ------- |
| prosecutor.brutal-honesty   | claude-code (sonnet) | filed                                    | 4       |
| prosecutor.codex-review     | codex                | filed                                    | 13      |
| prosecutor.mutation         | claude-code          | filed (13 mutations applied empirically) | 19      |
| prosecutor.security-scanner | codex                | filed (lean re-run, see note)            | 0       |
| prosecutor.devils-advocate  | codex                | **skipped** — stalled >35 min; see note  | —       |
| prosecutor.sherlock         | codex                | **skipped** — stalled >35 min; see note  | —       |
| defense                     | claude-code          | this record's dispositions               | —       |
| jury                        | codex                | see Verdict                              | —       |

**Skipped-seat note (per config `_acceptedRisk` mitigation contract):** the devils-advocate and
sherlock seats were launched with prompts requiring `cargo` runs under a `read-only` sandbox — an
operator (convener) error, not a codex derailment; both ground on failing commands and were stopped.
The security-scanner seat hit the same stall and was **re-run lean** (diff-reading only) to
completion — that seat is never omitted per config. Coverage for the two skipped seats' probe areas
came from the Tier-3 floor instead: a dedicated adversarial reviewer (9 findings, incl. forensic
claim-checking — sherlock's probe) and five parallel /code-review agents (CLAUDE.md compliance,
shallow bug scan, git-history regression check, prior-PR feedback, code-comment contracts —
devils-advocate's completeness probe). Both vendors are represented among _filed_ prosecutors
(claude: brutal-honesty, mutation; codex: codex-review, security-scanner), satisfying
`minDistinctVendors: 2`, and all grading remains codex (`writerIsNeverJuror` holds).

## Charges and dispositions (kill round)

### brutal-honesty (4 filed)

1. **[MAJOR] Read-modify-write payload sync lost the old single-UPDATE atomicity, undocumented.**
   REMEDIATED (`3132594`): single-writer-per-row invariant + row-lock escalation path documented at
   both status updaters (plan_repo.rs) and in ADR-036 §2.4.
2. **[MINOR] DoD literal `grep:absent: json_set|jsonb_set` failed on doc comments.** REMEDIATED
   (`3132594`): comments paraphrased; the literal check now returns 0 hits.
3. **[MINOR] `cargo tree | sort -u` prints 2 lines for one sqlx version.** ACCEPTED AS EVIDENCE
   NUANCE: the second line is cargo tree's `(*)` dedup marker for the _same_ `sqlx v0.9.0`;
   normalized output is exactly one version. No dual sqlx stack exists (the DoD's stated intent).
4. **[MINOR] Unchecked `as i32` seq cast in insert_operation.** REMEDIATED (`3132594`): checked
   `i32::try_from` that errors like PostgreSQL itself would.

### codex-review (13 filed)

All 13 investigated. None is a defect _introduced by this phase_; the phase's contract was
behavior-equivalence with the hand-rolled code (pinning tests), and each pre-existing latent issue
is now durably recorded rather than silently carried:

- C3, C4 → already recorded pre-filing (`pl-sample-ids-unwritten`, `pl-pred-status-case`).
- C1 → `pl-load-1000-cap` (clamp identical at phase-1 marker `05036c9`).
- C2 → `pl-action-filter-json`.
- C5 → `pl-applied-at-payload-unsynced`.
- C6 → `pl-plan-id-only-authz`.
- C8, C9, C10, C13 → consolidated `pl-legacy-enum-file-classes` (files phase 3 re-ports; content/jobs
  is not yet wired per Cargo.toml).
- C7 (session-TZ timestamp shift) → PARTIALLY REFUTED empirically (cr-bugscan verified inserts against a
  live container; the _crash_ claim is false and binds succeed) but the non-UTC-session write-shift risk is
  real in principle → `pl-timestamp-write-tz` with the precise fix for phase 3.
- C11, C12 (forgiving decode defaults) → pre-existing design preserved verbatim from the old code;
  noted, not actioned (changing decode strictness is a behavior change out of phase scope).
- Plus 6 CLEAN findings confirming the port's core claims.

### mutation (19 filed; 13 mutations empirically applied, 12 SURVIVED at filing)

Root cause: one test-design gap — no two-owner scoping assertions. REMEDIATED (`7068245`): the
suite gained tenant/plan/job scoping tests (kills charges 1–8), an expire_due status-guard fixture
(9), timestamp/hash round-trip asserts (10, 16), column-level asserts for applied_at/status/
email_id/source_kind (11, 17, 18), risk_max medium/high round trip (12), and a list_by_user
ordering assert (14). Charges 13 (sample_operations coverage — column never written; see
`pl-sample-ids-unwritten`), 15 (clamp bounds), 19 (lt/lte boundary) recorded as accepted residual
minor gaps. Empirical re-verification of the MAJOR mutations against the hardened suite: see
Addendum A below.

### security-scanner (0 filed)

SQL injection, secrets, untrusted-content binding, and error/log hygiene probes all CLEAN.

### Tier-3 floor (adversarial reviewer, 9 findings + five /code-review agents)

- tier3 #1 (unchecked seq cast) — already fixed pre-report (`3132594`).
- tier3 #2 (`load` 1000-cap), #6 (PartiallyApplied spelling), #7 (write-side TZ), #8 (206 residual
  `.pool()` sites; partial conversions in ingestion.rs) — parking-lotted (see above; #8 →
  `pl-partial-pool-conversions`). The suggested pre-phase-3 postgres:// startup guard was
  considered and declined: the enum/DatabaseConnection coexistence is the pipeline's declared
  interim state, phase 3 (next) deletes `pool()`, and phase 4 only then wires CI/deploy surfaces.
- tier3 #3 (list_operations error-swallow removal) — deliberate bug fix from wip commit `2bab3f5`,
  documented in its message; confirmed intentional by the git-history review agent.
- tier3 #4 (RMW race) — as brutal-honesty C1, remediated. #5 (payload key-order normalization) —
  documented at `payload_with_status` (`7068245`).
- tier3 #9 (PG tests green-by-default) — `pl-phase4-test-pg-env` records the phase-4 CI handoff
  (export `EMAILIBRIUM_TEST_PG_URL` in the postgres job).
- /code-review: claudemd — 4 legacy-enum files crossed the 500-line guideline via per-backend
  duplication (the exact duplication ADR-036 exists to delete; resolved by phase 3, tracked);
  bugscan — NO ISSUES (incl. an empirical live-PG check); history — NO ISSUES, all wip-commit bug
  fixes verified preserved, upsert-coverage gap closed (`7068245`); priorprs — rules.rs missed
  conversions (→ `pl-partial-pool-conversions`); comments — ADR-035 §2.3 citation misattributions
  fixed in ADR-036/plan_repo/cleanup-audit (`7068245`), entity-doc + spike-label precision fixed.

## Verdict

- **Jury (codex, blind to writer):** SHIP, 0 surviving charges — spot-checked remediation commits,
  mutation re-verification, entity/DDL match, single-sqlx claim, and parking-lot provenance
  (sampled latent defects confirmed pre-existing at `05036c9`).
- **Overturn round 1 (codex, effort high): OVERTURN — 3 grounds.** Per protocol this remands the
  SHIP into the fix loop. Grounds and dispositions:
  1. _Single-writer invariant not enforced_ — CONFIRMED and REMEDIATED (`0378b0d`): `begin_apply`'s
     status gate reads the caller's snapshot (pre-existing TOCTOU allowing duplicate apply
     workers, now recorded as `pl-concurrent-apply-guard`); the plan_repo/ADR-036 documentation
     was corrected from "safe because one writer" to the accurate "intended topology, not
     machine-enforced, with the pre-existing race named". The race itself predates the phase
     (duplicate mailbox side effects under the old code too) and its fix (atomic Ready→Applying
     CAS + tests) is deliberately routed through the parking-lot, not folded into this phase.
  2. _sea-orm default features not trimmed_ — CONFIRMED and REMEDIATED (`0378b0d`):
     `default-features = false` with the explicit list; `sqlite-use-returning-for-3_35` kept
     deliberately (the verified RETURNING insert path) with a comment. Resolved features now
     exclude with-json/with-rust_decimal/with-time/stream. Full suite (1205), live-PG tests, and
     clippy re-verified green after the trim.
  3. _cargo-tree DoD line literally unmet_ — ACKNOWLEDGED AS A PLANNER AUTHORING BUG, not
     reinterpreted away: the command as written cannot ever print one line while sea-orm exists,
     because sqlx necessarily appears at ≥2 tree positions and cargo appends a `(*)` dedup marker
     to repeats — the very sharing the check exists to prove. Verbatim output: `sqlx v0.9.0` /
     `sqlx v0.9.0 (*)` (one version, two occurrences). The canonical duplicate-version check
     (`cargo tree -d -e no-build | grep sqlx`) prints nothing. Recorded here and in the ledger
     rather than editing the committed DoD text (gate-tampering) or asserting a literal pass.
- **Overturn round 2 (codex, effort high, on the remediated delivery): OVERTURN → WITHDRAWN on
  adjudication.** Its single ground — `sea-orm-arrow`/`arrow` entries in Cargo.lock violating the
  "no arrow" deliverable — rested on lockfile semantics: Cargo pins optional dependencies
  feature-agnostically whether or not any feature activates them. Feature-resolved evidence put
  back to the same reviewer thread: `cargo tree --prefix none | grep -ci arrow` → 0 (all edge
  kinds), and sea-orm's resolved feature set contains no `arrow`. The reviewer withdrew: "I
  conflated feature-agnostic lockfile pins with the resolved build graph. Both cargo-tree probes
  return zero Arrow edges; the no-Arrow feature-selection requirement is satisfied."

**FINAL VERDICT: SHIP** — jury SHIP with 0 surviving charges; overturn round 1's grounds
remediated in the fix loop (`0378b0d`); overturn round 2 withdrawn on factual adjudication.
The human judge reviews this record on the phase PR (pr_ci mode).

## Addendum A — mutation re-verification (post-hardening, commit `7068245`)

Five representative mutations — one per surviving-mutation class — re-applied one at a time
against the hardened suite (with mtime isolation after an initial stale-incremental-build
false-attribution run was detected and discarded), each reverted after its run; final tree clean,
baseline 24/24 green:

| Mutation (class)                                                     | Result     | Killed by                                                                                               |
| -------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------- |
| `load`: dropped `user_id` filter (read-scope drop / tenant boundary) | **KILLED** | `load_is_scoped_to_the_owning_user`                                                                     |
| `save`: dropped `plan_id` on ops `delete_many` (delete-scope drop)   | **KILLED** | `save_upserts_in_place_and_touches_only_its_own_plan`, `replace_account_rows_swaps_only_target_account` |
| job `update_state`: dropped `job_id` filter (update-scope drop)      | **KILLED** | `update_state_transitions_state_counts_and_finished_at`                                                 |
| `expire_due`: dropped ready/draft guard (predicate weakening)        | **KILLED** | `expire_due_expires_only_overdue_ready_or_draft`                                                        |
| `load`: swapped `created_at`/`valid_until` (field transposition)     | **KILLED** | `save_load_round_trip`                                                                                  |

The remaining MAJOR mutations (etag delete scope, list_operations plan scope,
replace_account_rows plan scope, cancel scope, list_by_user tenant filter, applied_at column
write) each map to a dedicated assertion added in `7068245` for exactly that mutation; accepted
residual minors: clamp bounds, lt/lte boundary, sample_operations (column never written — see
`pl-sample-ids-unwritten`).
