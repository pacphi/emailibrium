# qe-court record — ci-build-optimization, integration (develop -> main)

**Delivery:** the full 8-phase CI/build/Docker optimization, `main...develop`
(44 files, +2387/-1438) plus the cross-phase optimization pass.
**Date:** 2026-07-31 · **Seat:** the release go/no-go, per ADR-124.

## Panel — properly seated for the first time in this pipeline

The user fixed the jury routing before this convening (`.claude/skills/qe-court/config.json`).
Earlier phase courts could not seat a jury at all because 4 of 8 roles, including the
jury, routed to Cognitum, which is unconfigured in this environment.

| Role                         | Provider            | Vendor |
| ---------------------------- | ------------------- | ------ |
| Writer (author under review) | claude-code         | Claude |
| Jury                         | codex               | GPT    |
| Deeper reviewer              | codex (high effort) | GPT    |

`writerIsNeverJuror`: **SATISFIED** — the jury is a different vendor from the author.
`minDistinctVendors: 2`: **SATISFIED**.

**Caveat, stated plainly:** the jury's run was partly polluted by an injected plugin
preamble (the accepted risk recorded in `config.json._acceptedRisk`). It nonetheless
read the real diff — the transcript shows it running
`git diff --unified=40 main...develop` over the justfiles, ADR-032 and the setup
scripts — and returned a substantive, file-cited verdict. It is counted as a real
juror. The kill round and overturn round were NOT run, so this is still weaker than
the full ADR-124 protocol.

## Verdict: REMAND — charges upheld and fixed

Both charges were verified against the repo before being accepted, and both were
things the author's own Tier-3 reviews had missed.

### Charge 1 — false-green release assurance (UPHELD, FIXED)

Phase 6 added a paths-scoped `pull_request` trigger to `docker.yml` so image
packaging is verified pre-merge. The filter omitted inputs the image actually
consumes: `backend/entrypoint.sh`, `backend/migrations/**`,
`backend/rust-toolchain.toml`, and the `ruvector` submodule gitlink — all of them
`COPY`ed by `backend/Dockerfile`. A packaging regression in any of those could merge
with the Docker jobs never running, defeating the guarantee the trigger was added to
provide.

**Fix:** filter extended to every COPYed input, with a comment tying it back to the
Dockerfile's COPY lines. `backend/src/**` is deliberately excluded and the reason
recorded inline — it is COPYed, but the rust-build/rust-test jobs already compile it
and a cold release build per source change is not worth the cost.

### Charge 2 — incomplete task-runner migration (UPHELD, FIXED)

Phase 7 deleted the Makefiles, but live `make` commands survived in user-facing
places the author's sweep never covered — it grepped docs and `make <target>` in
shell scripts, but not source code, and not the `make -C <dir>` form in scripts:

- `frontend/apps/web/src/features/onboarding/OnboardingFlow.tsx:122` — the **product
  onboarding UI** displayed `make dev` to end users.
- `backend/src/main.rs:207` — the CLI printed `Run 'make models'`.
- `scripts/setup-ai.sh:92`, `scripts/setup-validate.sh:108` — `make -C backend build`
  / `make -C frontend install`.

Every one pointed at a command that no longer exists.

**Fix:** all corrected; a repo-wide sweep across `.sh`, `.rs`, `.tsx`, `.ts`, `.yml`,
`.json` now returns no live `make` invocation. Verified after the edits:
`cargo check` clean, `tsc --noEmit` clean, `shellcheck` clean, `just lint` and
`just format-check` green.

### Charge 3 — ADR-032 status (NOT UPHELD)

The jury flagged ADR-032 as "Accepted" while the branch claims implementation. This
repo's ADR convention uses **Accepted** for decisions that are made and implemented
(ADR-031 uses **Proposed** for design-only). No change.

## For the human judge

**Strongest case FOR merging:** the core engineering holds up — 1177-test parity
proven by execution across the nextest migration, toolchain drift removed and then
de-duplicated, four swallowed exit codes fixed, and two genuinely broken release
images (wrong build stage; glibc mismatch that built clean but could not run) now
fixed and verified on the amd64 architecture that actually ships.

**Strongest case AGAINST:** CI proves images _build_, not that the system _runs_.
Three disclosed gaps remain open and are not addressed by this branch — the Compose
healthcheck passes a flag `main.rs` does not parse (so the container can never report
healthy), the frontend image cannot serve standalone, and the Lighthouse budgets are
all `warn` so that job still cannot fail. None is a regression introduced here; all
three are recorded as parking-lot items. A container-start/health smoke test is the
single highest-value thing still missing.

## Re-convening (protocol: REMAND -> fix -> re-convene once)

Re-convened on the fix commit only. The jury ran clean this time (no plugin
derailment) and checked the current files directly:

> CHARGE 1: RESOLVED — All four Docker inputs are now included in the PR paths filter.
> CHARGE 2: RESOLVED — All four live commands now use `just`; no reviewed files differ from the fix commit.
> **FINAL: SHIP**

Verdict stands as **SHIP**, from a jury of a different vendor than the author.
The kill round and overturn round were still not run, so this remains short of
the full ADR-124 protocol — but the seating invariant (`writerIsNeverJuror`) held
and both upheld charges were reproduced, fixed, and independently re-verified.
