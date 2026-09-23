# qe-court record — postgres-support, phase 3 (full SeaORM port; hand-rolled dispatch deleted)

- **Delivery:** `git diff 27f7533..HEAD -- backend/` on branch `autopilot/postgres-support/phase-3`
  (83 files at filing time; every remaining DB call site on SeaORM's single code path,
  `Database::adapt()` / `audited_sql()` / `sqlite_placeholders_to_postgres()` / `Database::pool()`
  deleted; contract: behavior-preserving bug-for-bug on SQLite). PR #259.
- **Convened:** 2026-08-05, per `pipeline.court: auto` + phase 3 ∈ `risk_phases`.
- **Config:** `.claude/skills/qe-court/config.json` (2-vendor panel; `writerIsNeverJuror`,
  `blindFiling`, `minDistinctVendors: 2`, `overturnDepth: 2`).

## Panel

| Seat                        | Route                                  | Status                                    | Charges |
| --------------------------- | -------------------------------------- | ----------------------------------------- | ------- |
| prosecutor.brutal-honesty   | claude-code (agent)                    | filed                                     | 17      |
| prosecutor.mutation         | claude-code (agent, isolated worktree) | filed (12 mutations applied empirically)  | 8       |
| prosecutor.security-scanner | codex                                  | filed (lean re-run after full-diff stall) | 2       |
| prosecutor.codex-review     | codex                                  | filed (lean re-run after full-diff stall) | 1       |
| prosecutor.devils-advocate  | codex                                  | **skipped** — stalled >30 min; see note   | —       |
| prosecutor.sherlock         | codex                                  | **skipped** — stalled >30 min; see note   | —       |
| Tier-3 floor: scoping-tests | claude-code (agent)                    | filed                                     | 6       |
| Tier-3 floor: compliance    | claude-code (agent)                    | filed                                     | 5       |
| defense                     | claude-code                            | this record's dispositions                | —       |
| jury                        | codex                                  | see Verdict                               | —       |

**Skipped-seat note (per config `_acceptedRisk` mitigation contract):** all four codex seats
stalled on the full 83-file diff (>30 min silent, the known failure class from phase 2 — this time
with diff-reading-only prompts, so the stall is diff-size, not cargo). Per config,
security-scanner is never omitted and was **re-run lean** (explicit file list) to completion;
codex-review was likewise re-run lean so both vendors are represented among _filed_ prosecutors
(`minDistinctVendors: 2` holds; all grading remains codex). devils-advocate's completeness probe
is covered by the two Tier-3 floor filings (a full owner-scoping coverage census and a
project-rule/ADR compliance audit with a checked-clean list); sherlock's forensic
claim-vs-reality probe is covered by floor-compliance's claim verification (byte-identical claims
checked against DDL, entity-vs-migration type audit, JSONL well-formedness) **and** by
brutal-honesty, which independently ran exactly that probe (charges 1, 2, 6, 9, 14 are all
claim-vs-reality forensics with `git log -S` evidence).

## Charges and dispositions (39 filed)

Remediation commits: `9e03a6d` (scoping + compliance batch), `68c3c09` (mutation kill-pins),
`e9be763` (brutal-honesty batch + codex-review fix). Every disposition below was re-verified by
the full suite (1300 tests, 0 failed) + clippy clean at `e9be763`.

### floor-compliance (1 MAJOR, 4 MINOR)

1. **[MAJOR] Three stale `db_error` doc comments claim `VectorError` has no `DbErr` variant.**
   REMEDIATED (`9e03a6d`): comments corrected to state the real, deliberate rationale
   (operation-name prefix retained; variant part of the module's observable error shape).
2. **[MINOR] Raw-SQL hatch inventory outgrew ADR-036 §5.** REMEDIATED (`9e03a6d`): §5 amended to
   enumerate the five accepted classes (portable aggregate text, per-backend FTS, per-backend
   auto-increment DDL, catalog introspection, the ADR-003 store wholesale).
3. **[MINOR] `wipe_audit_log` has no migration/entity.** PRE-EXISTING; RECORDED
   `pl-wipe-audit-log-no-migration` (db-schema-modernization candidate).
4. **[MINOR] Test constructor ships in release builds.** REMEDIATED (`e9be763`):
   `#[cfg(any(test, feature = "test-vectors"))]` — the crate's own `embedding.rs` idiom.
5. **[MINOR] Large-file growth.** ACCEPTED: growth is test-dominated (production/test split
   audited in the filing); splitting api/emails.rs + api/ingestion.rs production halves noted for
   the optimization pass.

### floor-scoping (3 MAJOR, 3 MINOR)

1. **[MAJOR] Every destructive owner-scoped DELETE untested for scoping.** REMEDIATED
   (`9e03a6d`): `wipe_user_data` two-owner bystander assertions; `empty_trash` condition extracted
   (`trashed_condition`) and pinned by a real two-account delete. `disconnect_account` DEFERRED
   with `pl-api-integration-false-coverage` — its handler needs `AppState`, which is not
   lib-exported (same root cause as charge 2).
2. **[MAJOR] `backend/tests/api_integration.rs` is false coverage (replica handlers diverged from
   production).** RECORDED `pl-api-integration-false-coverage`: root fix (lib-export
   AppState/routes) is architectural, out of a bug-for-bug port's scope; deferred with record.
3. **[MAJOR] vectors/ingestion.rs five owner filters unpinned, helper id scheme blocks a second
   account.** REMEDIATED (`9e03a6d`, `68c3c09`): ids account-prefixed; two-account ingestion pin;
   deleted/trash/spam exclusion pin; checkpoint per-account + conflict-progression pins.
4. **[MINOR] Provider-coupled sync paths unpinned.** RECORDED `pl-sync-provider-paths-unpinned`
   (needs a provider fake; `mark_sync_idle` — the no-provider path — is pinned).
5. **[MINOR] user_learning composite-key upsert unpinned across users.** REMEDIATED (`9e03a6d`):
   two-user same-category isolation test.
6. **[MINOR] api/attachments.rs ported with no DB tests.** ACCEPTED: its filters are
   `email_id`-scoped (not owner-scoped — outside the two-owner contract); noted for the
   optimization pass.

### mutation (12 applied, 8 survived → 8 charges)

All eight survivors REMEDIATED (`68c3c09`) with kill-pins that assert the exact behavior each
mutation broke: update_email_state two-row pin (M11), purge extracted as
`purge_expired()`/`purge_cutoff()` with polarity+cutoff pin (M12), ingestion soft-delete/trash/
spam exclusion pin (M5), hybrid `filter_email_ids` date-direction + is_read pins (M1/M2),
recurring-senders DESC-order pin (M4), checkpoint conflict-update progression pin (M6),
user_learning UpdatedAt-advance pin (M8). One residual gap ACCEPTED-with-record inside M6's pin
comment: the `status` column's failed→running conflict transition specifically needs an
embedding-failure fake (the progression pin covers the update-column set generally).

### brutal-honesty (8 MAJOR, 9 MINOR)

1. **[MAJOR] `received_at` on-disk format silently changes; test prose claims the opposite.**
   Prose REMEDIATED (`e9be763`) — the comment now states the port introduces the second shape and
   names the live-upgrade ordering caveat; RECORDED `pl-received-at-mixed-shape`. The format
   itself is the ADR-036 entity-typing consequence; retirement (TIMESTAMPTZ + backfill) is
   db-schema-modernization's, by plan.
2. **[MAJOR] `test_sqlite_database` justification false; crate already had the fix idiom.**
   REMEDIATED (`e9be763`): `cfg(any(test, feature = "test-vectors"))`, honest doc.
3. **[MAJOR] `now_text()` copy-pasted four times.** REMEDIATED (`e9be763`): one `db::now_text()`
   owns the format contract; four copies deleted. (The two "inline" sites format _other_
   instants — a retention cutoff and a test seed — not now().)
4. **[MAJOR] 21 hand-rolled migration replayers.** REMEDIATED (`e9be763`):
   `db::apply_sqlite_migrations()` is the one copy; 25 call sites converted (incl. the concat!,
   single-file, and path-match variants).
5. **[MAJOR] ensure_table question answered two opposite ways.** REMEDIATED (`e9be763`): every
   migration-owned `ensure_*` deleted (audit/privacy no-ops, `user_learning`/evaluation DDL
   replicas) and their composition-root calls dropped; tests replay migrations 007/009; the one
   keeper is `remote_wipe`'s genuinely migration-less table. The two hand-written _minimal test
   schemas_ (`offline_queue`/`checkpoint`) ACCEPTED as test fixtures, not schema copies.
6. **[MAJOR] Four comments falsified by this very range.** REMEDIATED: row 1 in `9e03a6d`; rows
   2–4 in `e9be763` (`ensure_*` comments deleted with the fns; AppState.orm doc rewritten to the
   real end-state + `pl-database-enum-in-signatures`; the "coexists" comment resolved by deleting
   the dead variant it described).
7. **[MAJOR] Escape hatch ~35× wider than the ADR; ADR untouched.** REMEDIATED (`9e03a6d`): §5
   amended (five classes). The 24 `Expr::cust("COUNT(*)")` sites render SQL identical to the
   `Func::count` helper — consolidation ACCEPTED for the optimization pass
   (`pl-database-enum-in-signatures` note). No injection found by any seat; every interpolating
   `format!` is test-only (security-scanner grep, independently).
8. **[MAJOR] Live-PG evidence is a hand-run script.** PARTIALLY ACCEPTED: the accidentally
   committed example was removed (`68c3c09`); the 15-check run's content and results are recorded
   in `8cb700a` and this record; CI-enforced PostgreSQL (EMAILIBRIUM_TEST_PG_URL job) is phase 4's
   DoD **by plan** — the phase-3 DoD required a live verification, which was performed and caught
   two real PG decode bugs (fixed in `8cb700a`).
9. **[MINOR] `count_star` triplicated; one doc factually wrong about PG.** Doc REMEDIATED
   (`e9be763`); consolidation ACCEPTED for the optimization pass.
10. **[MINOR] Two dead `sqlx::Error` variants.** REMEDIATED (`e9be763`): both deleted.
11. **[MINOR] Privacy leak-test pins a type nothing produces.** REMEDIATED (`e9be763`): pins
    `sea_orm::DbErr`; doc updated.
12. **[MINOR] i64→i32 narrowing solved three ways.** ACCEPTED (optimization pass; recorded).
13. **[MINOR] `ci_eq` Unicode fold diverges from SQLite's ASCII `lower()`.** REMEDIATED
    (`e9be763`): `to_ascii_lowercase()` restores exact-case non-ASCII matches on SQLite; the
    per-backend non-ASCII folding difference (PG lower() is Unicode-aware) documented as
    irreducible without ICU.
14. **[MINOR] `ensure_sync_state` doc misstates SQLite's conflict algorithm.** REMEDIATED
    (`e9be763`): FK exclusion corrected; the check-then-act race caveat added.
15. **[MINOR] Half-finished handle refactor (`Arc<Database>` ctors).** ACCEPTED + RECORDED
    `pl-database-enum-in-signatures` (deliberate phase scoping; mechanical churn deferred).
16. **[MINOR] Read-rate aggregate duplicated in-file.** REMEDIATED (`e9be763`):
    `read_rate_expr()`.
17. **[MINOR] Tests disable FK enforcement the port relies on.** ACCEPTED: deliberate parent-less
    seeding in minimal fixtures; the FK-dependent invariant is exercised by migrated-DB tests
    (mcp_integration runs the full migration set with FKs on).

### security-scanner, lean (2 MAJOR)

1. **[MAJOR] `wipe_user_data` aborts on phantom `search_interactions.user_id`; erasure partial.**
   PRE-EXISTING, ported bug-for-bug per the phase contract; RECORDED
   `pl-wipe-interactions-phantom-column` (filed before the seat ran; the seat's
   backups-deleted-first sequencing detail is in the record). Fix is a schema/behavior decision
   (phase 7/8 or db-schema-modernization).
2. **[MAJOR] Failed scheduled wipes discarded when any wipe in the batch succeeds.**
   PRE-EXISTING (byte-identical to `27f7533`, verified); in-memory schedule logic untouched by
   the port; RECORDED `pl-scheduled-wipe-failure-discard` (phase 7/8 behavior change).
   All other probes: **0 injection findings** — every raw statement binds its values; GDPR
   export/erase predicates match the pre-port tree.

### codex-review, lean (1 MINOR)

1. **[MINOR] `before` filter includes exact-midnight rows (legacy excluded the whole boundary
   day).** REMEDIATED (`e9be763`): strict `lt` + comment. All seven other probe areas returned
   clean (PG-validity of aggregates/decodes, checkpoint conflict columns, dequeue claim
   semantics, entity-vs-PG-DDL spot check, sync-state binds).

## Tally

39 charges filed → **26 remediated** (verified by suite+clippy at `e9be763`), **7
recorded-and-deferred** with parking-lot entries (all pre-existing bugs or explicitly
out-of-contract architectural work), **6 accepted with rationale** (test fixtures, stylistic
consolidation, phase-4-owned CI work). **0 unaddressed.** Parking-lot records added this court:
`pl-wipe-audit-log-no-migration`, `pl-api-integration-false-coverage`,
`pl-sync-provider-paths-unpinned`, `pl-scheduled-wipe-failure-discard`,
`pl-received-at-mixed-shape`, `pl-database-enum-in-signatures` (+
`pl-wipe-interactions-phantom-column` filed during the port itself).

## Verdict

**SHIP** (codex jury, 2026-08-05). The jury spot-checked the five least-convincing dispositions
against the tree and **held all five**, zero overturns (so neither overturn round was needed):

1. floor-scoping-1 — HOLD: `trashed_condition` scoping + two-account bystander assertions
   verified at api/emails.rs:913/:1646; `wipe_user_data` bystander survival at
   remote_wipe.rs:560.
2. brutal-honesty-4 — HOLD: single replay implementation at db/mod.rs:105; no competing replay
   loops remain (oauth's whole-script `execute_unprepared` calls are not hand-rolled replayers).
3. security-scanner-1 — HOLD: verified `27f7533` already carried the phantom
   `search_interactions.user_id` delete and both dialects' migrations still lack the column; the
   defect is real, remains prioritized, and the "pre-existing and recorded" disposition is
   accurate.
4. security-scanner-2 — HOLD: schedule-retention logic byte-identical to `27f7533`; pre-existing,
   correctly recorded.
5. codex-review-1 — HOLD: `e9be763` changed the boundary comparison to strict `lt`, restoring the
   legacy whole-boundary-day exclusion.

Jury rationale (verbatim core): "Both security defects are real and should remain prioritized,
especially the partial GDPR wipe, but commit history establishes that they predate this
behavior-preserving port and the court record names their exact failure modes. … Given the
stipulated 1,300-test/clippy result, green PR CI, and successful 15-check live-PostgreSQL
verification after the two decode fixes, no phase-3 release blocker remains."
