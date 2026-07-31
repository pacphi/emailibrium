# qe-court record — ci-build-optimization phase 4

**Delivery:** restructure Rust CI to build test binaries once into a `cargo-nextest`
archive consumed by `rust-test` (OPTIMIZATION_SPEC F-5).
**Date:** 2026-07-31 · **Phase:** 4 (in `risk_phases`) · **Court mode:** `auto`

---

## ⚠️ Panel could NOT be seated as configured — read this before trusting the verdict

`.claude/skills/qe-court/config.json` routes 4 of 8 roles to **Cognitum**
(`prosecutor.devils-advocate`, `prosecutor.sherlock`, `prosecutor.security-scanner`,
and — critically — **`jury`**). Cognitum is **not configured in this environment**:
no `COGNITUM*` env var, and `aqe llm-router config` returns nothing.

The skill also mandates calling `validateCourtConfig()` from
`src/skills/qe-court/referee.ts` before seating a panel. **That file is not present**
in this project's skill install (`.claude/skills/qe-court/` contains only
`config.json`, `evals`, `schemas`, `scripts`, `SKILL.md`), so the machine-checked
invariant validation could not be run at all.

Per the skill's own instruction — *"do not proceed with a degraded panel and do not
silently re-route around it"* — this is recorded as a **partial court**, not a full
one. What actually ran is stated plainly below. **This verdict carries less weight
than a fully-seated court** and should be read as one strong cross-vendor lens plus
the Tier-3 floor, not as the ADR-124 protocol.

| Role | Configured | Actually run |
|---|---|---|
| Defense | claude-code | — (not run) |
| Prosecutor · devils-advocate | cognitum-mid | ❌ vendor unavailable |
| Prosecutor · brutal-honesty | claude-code | ✅ ran as the Tier-3 reviewer subagent |
| Prosecutor · sherlock | cognitum-high | ❌ vendor unavailable |
| Prosecutor · security-scanner | cognitum-mid | ❌ vendor unavailable |
| Prosecutor · mutation | ollama | ❌ not run |
| Prosecutor · codex-review | codex | ✅ ran (`codex exec`, GPT — true cross-vendor) |
| Jury | cognitum-high | ❌ **vendor unavailable — no independent jury seated** |
| Deeper reviewer / overturn | codex | ❌ overturn round not run |

**Anti-collusion status:** 2 distinct vendors did file charges (Claude + GPT-via-codex),
satisfying `minDistinctVendors: 2`. But `writerIsNeverJuror` could not be *enforced*
because no jury was seated — the author (Claude) adjudicated the charges. That is the
weakness in this record. The human judge is the real backstop.

---

## Charges filed

### Prosecutor: brutal-honesty / Tier-3 reviewer (Claude) — 2 blockers, 1 false claim

| # | Charge | Status |
|---|---|---|
| 1 | **Arch split.** `rust-build` used `HEAVY_RUNNER` while `rust-test` hardcoded `ubuntu-latest`. A nextest archive is host-bound (pins host target triple, ships host libstd). Setting `HEAVY_RUNNER` to `ubuntu-24.04-arm` — its documented purpose — would emit aarch64 binaries `rust-test` cannot exec. Impossible before the split, when build+run shared a runner. | **REPRODUCED → FIXED** (both jobs now share `runs-on`) |
| 2 | **Bench gated the artifact.** `cargo bench --no-run` ran before `upload-artifact`, so a bench-only break aborted the job pre-upload and skipped `rust-test`. | **REPRODUCED → FIXED** |
| 3 | **False claim in my own comment.** I wrote that benches were "nearly free" in `rust-build` because they share its target dir. Verified false: `cargo bench --no-run` builds the *optimized* `bench` profile into `target/release`, a separate full dep-graph build. | **CONFIRMED → comment removed, bench moved to its own job** |

### Prosecutor: codex-review (GPT — cross-vendor) — 3 charges

| # | Charge | Status |
|---|---|---|
| 1 | **Test-signal suppression.** Moving `upload-artifact` earlier fixes artifact *availability* but not job *conclusion*. A failing doctest step after the upload still fails `rust-build`; `rust-test` `needs:` it, so GitHub skips `rust-test` and all ~1177 tests go dark. **The upload-first comment was false.** | **REPRODUCED → FIXED.** `rust-build` now *ends* at upload; doctests moved to independent `rust-extra-checks`. |
| 2 | **Version-skew / supply-chain race.** Both jobs installed `cargo-nextest` unpinned via mutable `@v2`. A release landing between the jobs could mismatch archive producer and consumer; nextest's archiving docs require matching versions. | **VALID → FIXED.** Both pinned to `cargo-nextest@0.9.140`. |
| 3 | **Merge-gate regression.** Benches moved out of the `Rust Tests` context; if branch protection required only that context, a bench failure would no longer block merge. | **NOT CURRENTLY EXPLOITABLE.** Verified via `gh api`: neither `main` nor `develop` has branch protection, so there are no required contexts to regress. Recorded for whenever protection is added — the new job names (`rust-build`, `rust-extra-checks`) must be included. |

**Notable:** charge 1 from the cross-vendor prosecutor is exactly the kind of finding
the court exists for — the Claude-side reviewer found the *adjacent* bug (ordering)
and I applied a fix that looked sufficient but wasn't. A second vendor caught that the
fix was incomplete. Single-lens review would have shipped this.

---

## Kill round

Not run (no blind refuter seated). All charges above were instead **verified directly
by reproduction or by reading the authoritative source** before being accepted:
- Charge 3 (merge-gate) was *downgraded* by direct evidence (`gh api` → 404 "Branch not protected").
- The bench-profile claim was confirmed by observing `cargo bench --no-run` build the optimized profile.

## Verdict

**REMAND → charges fixed → re-rendered as SHIP (partial court).**

Every reproduced charge was fixed in-phase; none was waived. The verdict is marked
*partial* because no independent jury was seated and no overturn round ran, so the
asymmetric "SHIP must survive escalation" guarantee — the mechanic that makes a court
harder to fool than a review — **did not apply here**.

## Evidence backing the delivery

- Archive builds: 15 binaries, 106 files, **125 MB**.
- Run-from-archive in a *different directory*: **1177 tests run, 1177 passed, 7 skipped**.
- `cargo test --workspace`: **1177 passed, 7 ignored** — exact parity, no lost tests.
- Doctests: 2 present, both `ignored`, **0 executed** either way → nextest's lack of
  doctest support costs nothing today, and `rust-extra-checks` now guards future ones.
- `--workspace-remap .` and `--profile ci` both verified working locally.

## Correction to an earlier record

The phase-0 PR and ledger reported **"39/39 tests passing."** That was **wrong** — it
summed only the tail of `cargo test` output and missed `lib.rs` (1004 unit tests) and
`main.rs` (65). The real figure was always **1177**. Corrected here for the record.

## For the human judge

**Strongest case FOR shipping:** exact 1177-test parity between the old and new paths,
verified by execution rather than inference; every charge from two vendors was
reproduced and fixed rather than argued away; the wall-clock win is structural
(`rust-build` runs concurrently with clippy instead of `rust-test` waiting for clippy
and only then compiling).

**Strongest case AGAINST:** no independent jury and no overturn round — the author
adjudicated charges against their own work. Two of the three most serious findings
were caught only *after* an initial fix looked adequate, which is evidence the
remaining unreviewed surface may hold more. The 125 MB artifact round-trip is new
per-run cost that partially offsets the scheduling win, and has not been measured in
CI. And the real wall-clock benefit is still **unproven in CI** — the rust-cache has
been cold since phase 1 (`pl-rust-cache-cold-after-env-change`), so no trustworthy
warm baseline exists to compare against yet.
