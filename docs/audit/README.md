# Documentation Accuracy Audit

This directory is the durable record and tooling for keeping the docs corpus honest about
what the code actually does. It's the output of the `docs-accuracy-audit` pipeline
(phases 0–6) and the home of the CI gate phase 6 built to keep it from drifting again.

## What's here

| File              | What it is                                                                                                                                                             |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `inventory.md`    | Phase 0's catalogue of every doc in the repo, by consumer persona (end user, operator, API/MCP consumer, contributor, AI coding agent, decision reader).               |
| `drift-report.md` | Phase 0's findings — 74 evidenced doc-vs-code contradictions, each with the doc claim, the reality, and a severity. Phases 1–5 fixed the ones tagged to their persona. |
| `api-coverage.md` | Phase 3's REST-route coverage report: `docs/api/openapi.yaml` vs. the real Axum route tree, plus the "Deliberate exclusions" table the CI gate (below) also reads.     |
| `README.md`       | This file.                                                                                                                                                             |

## The CI gate: what it checks, and — just as important — what it doesn't

`.github/workflows/docs-accuracy.yml` runs two jobs, both mechanical and both able to
prove a specific claim false:

1. **Config Key Coverage** — every config key the backend actually reads (`backend/config.yaml`,
   `config/app.yaml`, or a literal `env::var()` call) is mentioned in
   `docs/configuration-reference.md`. Runs `scripts/audit/check-config-coverage.sh`, which
   wraps `scripts/audit/extract-config-keys.sh` (the same ground-truth script phase 2 used
   by hand).
2. **API Route Coverage** — every real REST route Axum's router composition actually
   serves has a `paths:` entry in `docs/api/openapi.yaml`, or — for a route that
   structurally can't be an OpenAPI path (the one case today: the MCP transport mount,
   which is JSON-RPC, not REST) — a documented reason in `api-coverage.md`'s "Deliberate
   exclusions" table. Runs `scripts/audit/check-api-coverage.sh`, which wraps
   `scripts/audit/extract-routes.sh` (phase 3's ground-truth script).

**Deliberately not checked**: wording, tone, prose completeness, whether a description is
well-written, whether an ADR's reasoning still holds up. Those need human judgment, and a
gate that tries to mechanically enforce them produces false positives — which is exactly
how a doc gate earns a reputation for being noisy and gets disabled within a week. See
`.autopilot/pipeline.yml`'s phase 6 entry for the reasoning: "a doc gate that fires on
false positives will get disabled within a week, and a gate that never fires is theater."

Both checks are one-directional on purpose: they catch a **real key/route that's
undocumented**, not a **doc that mentions something that no longer exists**. The latter is
stale prose — worth fixing, but not something a script can reliably tell apart from
intentional forward-looking documentation, so it's left to review instead of a hard fail.

## Re-running the audit locally

```bash
# What the gate runs in CI:
bash scripts/audit/check-config-coverage.sh
bash scripts/audit/check-api-coverage.sh

# The underlying ground-truth extraction, if you want to see the raw data:
bash scripts/audit/extract-config-keys.sh    # yaml:/app-yaml:/env: prefixed keys
bash scripts/audit/extract-routes.sh         # full route paths, opaque mounts flagged
```

Both check scripts print `::error::`-prefixed GitHub Actions annotations naming exactly
which key or route is missing and where to fix it, so a red run in CI tells you what to do
without needing to reproduce it locally first — but reproducing locally is faster than
waiting on CI when iterating.

## When a failure is real vs. when to re-baseline

**A real gap (the common case, fix the docs):**

- New config key added to `backend/config.yaml`, `config/app.yaml`, or a new
  `env::var()` call → add a row to `docs/configuration-reference.md`.
- New REST endpoint added to `backend/src/api/**.rs` → add a `paths:` entry to
  `docs/api/openapi.yaml`, reading the actual handler and its request/response structs
  for the schema — never inventing a shape from the route name (this is the single most
  common source of drift these scripts exist to catch: `#[serde(rename_all)]` is
  per-struct in this codebase, not per-file, so two structs in the same handler file can
  serialize differently).

**A legitimate exception (re-baseline, don't fight the gate):**

- A new endpoint is structurally not a REST resource (another opaque mount, à la MCP) →
  add a row to `docs/audit/api-coverage.md`'s "Deliberate exclusions" table with a real
  reason. The CI check reads that table, so this is the one first-class way to tell it
  "this one's fine on purpose" rather than editing the check script itself.
- A config key is genuinely internal/deprecated and shouldn't be publicly documented →
  add it to `docs/configuration-reference.md` anyway, explicitly marked internal (e.g. "internal
  use only, not part of the supported surface") — the check is a substring match against
  the whole file, so a key mentioned _anywhere_, including in an explicit "not for public
  use" note, satisfies it. Removing keys from the check silently (editing
  `extract-config-keys.sh`/`extract-routes.sh` to skip them) defeats the ground-truth
  guarantee these scripts give every other phase of this pipeline and any future
  contributor — don't.

## If the gate itself needs to change

The extraction scripts (`extract-config-keys.sh`, `extract-routes.sh`) are also load-bearing
for future `docs-accuracy-audit`-style work — they're what make "the real config/route
surface" a reproducible, no-network, no-prompts command instead of something only
discoverable by hand-reading `backend/src/`. Changing their output format is a bigger
decision than it looks; if you need to, update both check scripts and this README in the
same change.
