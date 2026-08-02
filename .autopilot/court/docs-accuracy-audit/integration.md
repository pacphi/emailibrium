# qe-court record — docs-accuracy-audit, integration (develop → main)

**Convened**: 2026-08-02, per `.claude/skills/qe-court/config.json` (ADR-124 pattern), at
the integration seat (`.github` STEP E.2: "convene on... the base→trunk integration PR —
the human judge's evidence")
**Verdict**: **REMAND → fixed → re-verified**

## Scope

Full `develop`...`main` diff: all 7 phases (0–6) of `docs-accuracy-audit` plus the
cross-phase optimization commit — 40 files, +7076/-520 lines. Each phase already passed
its own Tier-1 + Tier-3 (or, for the risk phase 6, a degraded Tier-3 — see
`.autopilot/court/docs-accuracy-audit/phase-6.md`) gate individually before merging into
`develop`. This pass looks specifically for what's only visible at the full-integration
level: cross-phase contradictions, commit-history hygiene, and anything that slipped past
per-phase review because no single phase's diff showed the whole picture.

## Panel

Codex connectivity recovered since phase 6's total (4/4) outage — a plain "pong"
connectivity check and one substantive review prompt both completed this time (the
substantive one took ~6–7 minutes, well under the 1800s idle timeout that killed every
phase-6 attempt). One codex-routed review ran; a second, independent Claude-side review
(the convener, i.e. this session) ran in parallel rather than waiting serially. This is
still short of the full 6-prosecutor/jury/overturn machinery `config.json` specifies —
proportionate to the integration seat's actual job (a final look at 7 already-reviewed
phases, not a first-pass review) rather than a second full court.

The codex review's own output included a `🧠 RuvNet Brain jumped in · guidance only, no
source read` marker — the same injected-preamble pattern flagged repeatedly during
per-phase work this session. Distinguishable here from a derailment: the review still
produced four concrete, file:line-cited, independently-verified findings, so it's recorded
as a filed review with a noted injection attempt, not a skipped/derailed one.

## Findings — all four verified true, all four fixed

**1. `check-config-coverage.sh` has the identical zero-floor bug already fixed in
`check-api-coverage.sh` during phase 6 — just never applied to its sibling.** A failing
`extract-config-keys.sh` piped via process substitution (`< <(...)`) does not trip `set -e`
in the parent shell; the `while read` loop simply sees EOF and the script reports "All
config keys documented (0 checked)" — a clean pass on total extraction failure.
Reproduced directly: swapped in a script that `exit 1`s immediately, got exactly that
false-pass output. **Fixed**: replaced the process-substitution read with a captured
`keys="$(...)"` assignment (which _does_ trip `set -e` on failure) plus an explicit
`MIN_EXPECTED_KEYS=100` floor with a distinct error path, mirroring the API-route check's
existing fix. Re-verified against three scenarios: the real repo (156/156, unchanged), a
script that exits 1 (now correctly fails), and a script that exits 0 with empty output
(now correctly fails on the floor check) — plus re-ran all of phase 6's original mutation
survivor cases (`host`, `port`, `embedding.model`, `security.hsts.max_age_secs`,
`HSTS_MAX_AGE`) against the rewritten script: all still correctly caught.

**2. Cross-document contradiction on PostgreSQL support.**
`docs/configuration-reference.md:53` described `database_url` as a "SQLite/PostgreSQL
connection URL," while `docs/deployment-guide.md` (phase 2's own fix) correctly says
`postgres://` is not yet supported and the backend will fail to connect. Phase 2's remit
included `configuration-reference.md` but this specific cell was missed. **Fixed**:
reworded the Description cell to state SQLite-only support and link to the Deployment
Guide's Database Strategy section (verified the link's fragment resolves: the guide's own
heading is `## Database Strategy: SQLite vs PostgreSQL`, and `lychee` confirms zero
broken links across all touched files).

**3. `deployment-guide.md`'s inline config example omitted `openrouter` as a
`generative.provider` value**, while `configuration-reference.md` correctly lists it (and
`openrouter` is confirmed real and wired — referenced in 5 backend source files:
`main.rs`, `vectors/{yaml_config,model_registry,mod,model_download,generative}.rs`). The
omission is not itself wrong — `deployment-guide.md`'s example quotes
`backend/config.yaml`'s own inline comment verbatim, and that comment is _also_ missing
`openrouter` (a separate, minor code-comment staleness, not touched here — out of any
phase's `touches:` scope). **Fixed**: `deployment-guide.md`'s illustrative comment now
lists the complete, accurate provider set.

**4. `docs/audit/api-coverage.md`'s "124 previously-undocumented routes added" domain
breakdown summed to 127, not 124.** The "124" figure was likely carried over from phase
0's unrelated `~124 raw route fragments` estimate rather than independently verified
against the per-domain tally drafted during phase 3. Reconstructing exactly which
historical per-domain count was off is not reliably possible after the fact. **Fixed**:
replaced the unreconcilable "124 newly-added, by domain" claim with a freshly-computed,
independently-verifiable breakdown of the _current_ 136-path total by `openapi.yaml`'s own
`tags:` (one tag per path, confirmed no path carries two tags) — sums to exactly 136,
checked directly against the file rather than reconstructed from memory.

**Bonus, caught during this same review pass** (not from either reviewer's report, found
while re-reading the diff after the fixes above): the optimization-pass commit's own
remediation-status blockquote in `drift-report.md` had a line-wrap defect — a rewrap
landed exactly inside a file path (`.autopilot/court/docs-accuracy-audit/` / `phase-6.md`
split across lines), and the continuation line silently dropped its `>` blockquote marker.
Markdownlint/prettier didn't flag it (valid markdown, just a broken paragraph grouping on
render). **Fixed**: split into three shorter blockquote paragraphs, none containing a long
inline path at risk of an unsafe wrap point.

## Verdict

**REMAND → fixed → re-verified.** All four findings (plus the one self-caught during
fix-up) addressed. Full Tier-1 gate green from repo root after fixes: format-check,
markdownlint, lychee (0 errors across all touched files), shellcheck, both coverage
scripts (156/156 keys, 137/137 routes), build, backend clippy, frontend eslint, full test
suite, audit — all pass. No test tampering; diff scope matches exactly the 5 files these
findings required touching.

This record and its fixes proceed as a follow-up commit on `develop`
(`autopilot/docs-accuracy-audit/integration-fixes`), landing before the `develop → main`
integration PR is opened — so the PR presented for human review already reflects this
pass's corrections rather than requiring a second round.
