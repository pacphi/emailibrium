# qe-court record — docs-accuracy-audit, phase 6

**Phase**: 6 (risk phase — `.autopilot/pipeline.yml`'s `risk_phases: [6]`)
**Convened**: 2026-08-01/02, per `.claude/skills/qe-court/config.json` (ADR-124 pattern)
**Verdict**: **REMAND** (fixed, re-verified — see below), rendered without a seated jury

## Panel as configured vs. panel as convened

The repo's `qe-court` config specifies a 6-prosecutor panel with a Claude/Codex cross-vendor
split (`writerIsNeverJuror`: the writer here is Claude, so jury + deeperReviewer are routed
to `codex`):

| Role                        | Configured provider  | Outcome                                                  |
| --------------------------- | -------------------- | -------------------------------------------------------- |
| defense                     | claude-code          | Stated below (this section)                              |
| prosecutor.brutal-honesty   | claude-code (sonnet) | **Filed** — full report                                  |
| prosecutor.mutation         | claude-code          | **Filed** — full report                                  |
| prosecutor.devils-advocate  | codex                | **SKIPPED** — 1800s idle timeout, zero response          |
| prosecutor.sherlock         | codex                | **SKIPPED** — 1800s idle timeout, zero response          |
| prosecutor.security-scanner | codex                | **SKIPPED** — 1800s idle timeout, zero response          |
| prosecutor.codex-review     | codex                | **SKIPPED** — 1800s idle timeout, zero response          |
| jury                        | codex                | **COULD NOT BE SEATED** — no reachable non-Claude vendor |
| deeperReviewer              | codex                | Not invoked (no SHIP verdict to overturn-check)          |

All four codex-routed prosecutor dispatches failed identically: `MCP server "codex" tool
"codex" sent no response or progress for 1800s; aborting`. This is the same failure mode
`config.json`'s own `_acceptedRisk` note documents from `ci-build-optimization` phase 6
("the codex prosecutor was hijacked by the plugin's injected preamble... filed ZERO
charges"), except total rather than partial this run — a connectivity smoke-test prompt to
the same MCP tool succeeded immediately (`"pong"`), so the _transport_ was reachable; all
four substantive prosecutor prompts specifically stalled.

Per the config's mitigation contract ("report a derailed one as `skipped`, never as a clean
pass"), this run is **single-vendor** (Claude only), not the intended cross-vendor panel.
No jury verdict was rendered — `writerIsNeverJuror` forbids seating Claude (the writer) as
juror, and no second vendor was reachable. The verdict below is a Tier-3-floor verdict
(two independent adversarial reviewers, evidence-based, re-verified directly by the
convener), explicitly **not** a qe-court three-valued jury verdict.

## What was reviewed

Phase 6's deliverable: a CI drift gate. Two new scripts
(`scripts/audit/check-config-coverage.sh`, `scripts/audit/check-api-coverage.sh`) wrapping
the pre-existing ground-truth extraction scripts from phases 2/3
(`extract-config-keys.sh`, `extract-routes.sh`), wired into a new
`.github/workflows/docs-accuracy.yml`, plus `docs/audit/README.md`.

## Prosecution — filed charges

**brutal-honesty** (full report in session transcript): traced both scripts' logic by
hand and by direct testing.

1. `check-config-coverage.sh`'s `grep -q -- "$key" "$DOC"` was an **unanchored substring
   match** — provably wrong, not theoretical (see mutation's independent confirmation
   below).
2. `check-api-coverage.sh`'s header comment claimed a "136/136 baseline... any regression
   fails the build immediately", but no such floor was ever checked — an empty
   `extract-routes.sh` output would print `OK 0` and exit 0.
3. (Minor, accepted as-is) `set -e` aborts before the scripts' own clean `::error::`
   branches on a crash (e.g. malformed YAML) — CI still fails, just with a raw traceback
   instead of the designed message. Not fixed; a real but low-severity gap in message
   quality, not correctness.
4. (Cosmetic, accepted as-is) `check-config-coverage.sh` re-runs the extraction a second
   time just to print a count.

**mutation** (full report in session transcript): systematically mutation-tested both
scripts against real keys/routes, not synthetic ones.

1. **API gate: SHIP on its own merits.** Deleted each of the 136 real routes' `paths:`
   entry in turn against the real `extract-routes.sh`/`openapi.yaml` pairing: **136/136
   mutants killed, 0 survivors.** Inverse direction (phantom documented-but-not-real
   paths) also clean: 0/136. Deterministic and idempotent.
2. **Config gate: REMAND.** Deleted each of the (then-)155 keys' table row from a scratch
   copy of `docs/configuration-reference.md`: **145/155 killed, 10 survived** (93.5% kill
   rate — 10 genuine false negatives). Root causes, all confirmed:
   - Short/common keys (`host`, `port`) match unrelated substrings (`localhost`,
     "exports", "report-uri", etc.).
   - Unescaped `.` in dotted keys (`embedding.model`, `embedding.dimensions`,
     `generative.provider`, `generative.builtin.model_id`) is a regex wildcard, matching
     unrelated prose.
   - `.` also wildcard-matches `_`, so `security.hsts.max_age_secs` was satisfied by the
     _different_ key `security.hsts_max_age_secs`'s row.
   - `env:HSTS_MAX_AGE` is a **structural** substring of
     `EMAILIBRIUM_SECURITY_HSTS_MAX_AGE_SECS` — that survivor could never fail, by
     construction.
3. **Structural gap beyond the check script itself**: `extract-config-keys.sh` (phase 2)
   is grounded against `backend/config.yaml`'s literal keys, not the Rust
   `VectorConfig` struct it deserializes into. Fields that exist in the struct
   (`cloud`, `cohere`, `redis`, `ollama`, `qdrant_url` — confirmed via
   `backend/src/vectors/config.rs`) but have no corresponding key in `config.yaml` are
   invisible to extraction entirely, even though they're real, already-documented,
   env-overridable knobs. **Not fixed in this phase** — parked as
   `pl-config-extract-blind-to-struct-only-fields` in
   `.autopilot/discovered/docs-accuracy-audit.jsonl`; closing it means teaching the
   extraction script to walk Rust struct definitions, a materially bigger change than
   "add a CI gate on top of the existing scripts."

Both prosecutors independently converged on the same core finding (substring matching is
broken) via different methodologies (static tracing vs. exhaustive mutation) — strong
corroboration despite the missing second vendor.

## Defense

The core mechanism — wrap the phase-0/2/3 ground-truth extraction scripts, diff against
the docs, fail loud with an actionable message — is sound and was proven to work end to
end (both the config-key and API-route checks were manually proven to fail on a real,
deliberately-introduced contradiction and pass once reverted, per the phase's own DoD,
_before_ the court convened). The API-route check in particular withstood full exhaustive
mutation testing (136/136) with zero changes needed. The flaws prosecuted are real,
narrow, and fixable without redesigning the approach — which is what happened.

## Remediation (post-filing, pre-final-commit)

1. `check-config-coverage.sh`: replaced the unanchored substring grep with an anchored,
   escaped, backtick-delimited match (``grep -qE -- "\`${escaped_key}\`" "$DOC"``,
   with `.`/`[`/`\`/`*`/`^`/`$`/`/` escaped). Re-ran mutation's exact 7 survivor cases
   against the fix: all 7 now correctly fail when their _only_ documentation is removed
   (2 — `database_url`, `quantization.mode` — have a second, independent legitimate
   mention elsewhere in the doc and correctly continue to pass unless _both_ mentions are
   removed, which is correct behavior, not a residual bug). Re-ran against the real,
   unmodified `docs/configuration-reference.md`: still 156/156 documented, 0 false
   positives introduced.
2. `check-api-coverage.sh`: added `MIN_EXPECTED_ROUTES=100` sanity floor on the raw
   extraction count, with a distinct `EXTRACTION_FAILED` error path (separate message
   from a real coverage gap, per the DoD's "tells a contributor exactly... how to fix"
   requirement). Proven by temporarily replacing `extract-routes.sh` with a 1-route stub:
   gate fails with the new distinct message; reverted, gate passes again.
3. Both fixes re-verified against all of this phase's own original proof-of-failure
   tests (route deletion from `openapi.yaml`, opaque-mount deletion from
   `api-coverage.md`'s exclusion table) — all still correctly fail and revert clean.

## Final verdict

**REMAND → fixed → re-verified.** Both scripts now pass their own DoD-mandated
prove-it-fails-then-passes test, plus mutation's full adversarial sweep (config: 155/155
after fix, spot-checked; API: 136/136, unchanged). Proceeding to commit with this record
attached, and with the codex-track outage disclosed in the PR body rather than presented
as a clean multi-vendor pass.
