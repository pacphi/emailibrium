# ADR-032: Replace Make with Just as the Task Runner

- **Status:** Accepted
- **Date:** 2026-07-31
- **Deciders:** Chris Phillipson
- **Context:** The repository drove every developer and CI workflow through three GNU Makefiles (root, `backend/`, `frontend/` — 110 targets, ~987 lines combined). Make is a _build system_ being used purely as a _task runner_: none of these targets declare real file dependencies, every one is `.PHONY`, and nothing relies on Make's timestamp-based rebuild logic. That mismatch cost real correctness, not just elegance — see §2.

---

## 1. Problem Statement

Make is doing a job it was not designed for here, and the mismatch had already produced shipping bugs.

- **Everything is `.PHONY`.** All 110 targets are task aliases; not one expresses a file→file dependency. Make's core value — incremental rebuild from timestamps — is unused, while its footguns remain.
- **Silent error swallowing was endemic and invisible.** Four targets masked failure exit codes. All were found while wiring an automated quality gate — the gate is only as trustworthy as the commands it runs, and several of the commands it depended on could never fail:
  - `frontend/Makefile: test: @$(TURBO) test || true` — reported success while Vitest failed.
  - `frontend/Makefile: audit: @$(PNPM) audit --prod 2>/dev/null || echo "..."` — a real vulnerability finding exited 0.
  - `backend/Makefile: audit: @$(CARGO) audit 2>/dev/null || echo "..."` — same defect.
  - `Makefile: download-models` — `cargo run ... 2>/dev/null || echo "Backend not built"`, exiting 0 on a genuine failure.

  **Timeline, for accuracy:** the two `frontend/Makefile` instances were fixed in place _before_ this migration (they were blocking the gate and could not wait). The `backend/Makefile: audit` swallow — tracked as `pl-mk-audit-swallow` — and the root `download-models` swallow are fixed _by_ this migration. So at the moment the Makefiles were deleted, two of the four were already patched; the pattern is listed in full because the recurrence across three separate files over time is the actual argument, not any single instance.

- **Shell-per-line semantics are implicit.** Make runs each recipe line in its own shell unless `.ONESHELL` is set, which is easy to violate accidentally in multi-line logic.
- **Recursive `$(MAKE) -C` delegation** obscures which sub-command actually failed.
- **Tab-vs-space significance** remains a recurring paper cut with no upside.

## 2. Decision

**Adopt [`just`](https://github.com/casey/just) as the sole task runner. Delete all three Makefiles.**

Three justfiles mirror the previous structure — root `justfile` delegating to `backend/justfile` and `frontend/justfile` — and recipe names are preserved verbatim (`build`, `test`, `lint`, `format-check`, `audit`, `ci`, `docker-up`, …) so existing muscle memory, documentation, and any external scripts need minimal change.

### Why `just` specifically

| Property           | Make                                   | Just                                                 |
| ------------------ | -------------------------------------- | ---------------------------------------------------- |
| Purpose            | build system (file DAG)                | task runner                                          |
| Phony declarations | required for every target              | not a concept                                        |
| Recipe listing     | hand-written `help` target that drifts | `just --list`, generated from doc comments           |
| Multi-line shell   | per-line shells unless `.ONESHELL`     | explicit shebang recipes with `set -euo pipefail`    |
| Arguments          | awkward (`$(filter-out ...)`)          | first-class recipe parameters                        |
| Leading whitespace | tabs significant                       | insignificant                                        |
| Failure semantics  | easy to mask accidentally              | same shell rules, but recipes are short and explicit |

### What changed beyond a mechanical port

- **The swallowed exit codes are fixed** (see the §1 timeline for which were patched before vs by this change). `just test`, `just audit` and `just download-models` now fail when the underlying tool fails. `backend/justfile`'s `outdated` got the same treatment, since it carried an identical swallow.
- **Multi-line recipes are explicit.** Anything with `if`/`for`/shared variables became a shebang recipe with `set -euo pipefail`, rather than relying on implicit per-line shells.
- **Deliberate `|| true` survives, with a comment.** A small number of genuinely informational recipes (`outdated`, docker prune-style cleanup) keep a tolerant exit and now say _why_ in a comment, so the distinction between "tolerant on purpose" and "accidentally swallowed" is legible.

## 3. Consequences

**Positive**

- The quality gate's commands are honest: a failing test or a real vulnerability now fails the command.
- `just --list` is generated from the recipes themselves, so it cannot drift from reality the way a hand-maintained `help` target does.
- Recipe intent is clearer: no `.PHONY` noise, no tab sensitivity, explicit shell semantics.

**Negative / costs**

- **`just` becomes a required developer dependency.** Make is preinstalled nearly everywhere; `just` is not. Install: `brew install just`, `cargo install just`, `mise use -g just`, or see the project README.
- Existing muscle memory needs a one-word change (`make test` → `just test`). Mitigated by keeping every recipe name identical.
- Anyone with a personal script wrapping `make <target>` must update it.

**Neutral**

- CI is unaffected in substance: no GitHub Actions workflow invoked `make` directly — the workflows call `cargo`/`pnpm` themselves. The task runner was only ever a developer and local-gate surface.

## 4. Alternatives Considered

- **Keep Make, just fix the swallowed exit codes.** Cheapest option, and it would have addressed the immediate correctness bugs. Rejected because it leaves the structural mismatch in place: the next multi-line recipe or `.PHONY` omission reintroduces the same class of problem, and the hand-written `help` target keeps drifting.
- **`cargo-make`.** Rust-native and capable, but TOML-configured and Rust-centric; this repo's task surface is half frontend (pnpm/Turborepo), so a language-neutral runner fits better.
- **npm scripts as the top-level entry point.** Would put the Rust backend behind a Node tool and require Node to run backend tasks. Rejected.
- **Shell scripts in `scripts/`.** Maximum portability, zero new dependency — but loses discoverability (`just --list`), argument handling, and grouping, and the repo already uses `scripts/` for genuine scripts rather than task aliases.

## 5. References

- `justfile`, `backend/justfile`, `frontend/justfile` — the implementation.
- `.autopilot/profile.yml` — the automated quality gate's command set, migrated from `make` to `just` in the same change so the gate never points at deleted targets.
- Parking-lot item `pl-mk-audit-swallow` (`.autopilot/discovered/ci-build-optimization.jsonl`) — the backend audit swallow this ADR's change resolves.
- [just manual](https://just.systems/man/en/)
