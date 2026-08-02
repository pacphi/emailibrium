#!/usr/bin/env bash
# check-api-coverage.sh — fail if any real REST route is undocumented in the OpenAPI spec.
#
# The docs-accuracy drift gate (phase 6 of the docs-accuracy-audit pipeline). Runs
# extract-routes.sh for the ground truth, then checks every route has a corresponding
# entry in docs/api/openapi.yaml's `paths:`. Baseline is the 136/136 (100%) coverage phase
# 3 established -- any regression below that fails the build immediately.
#
# Opaque service mounts (currently just the MCP transport at /api/v1/mcp) are not REST
# paths and can never appear in an OpenAPI paths: block -- those are checked against
# docs/audit/api-coverage.md's "Deliberate exclusions" table instead, so a *new* opaque
# mount still has to be consciously exempted with a documented reason, not silently ignored.
#
# MIN_EXPECTED_ROUTES is a sanity floor on the *extraction*, not the coverage ratio: without
# it, extract-routes.sh returning zero routes (its Rust-router-parsing regex breaking on a
# future refactor, say) reads as "0 real routes, 0 documented, 100% covered" -- a silent,
# maximally-wrong pass on exactly the failure this gate exists to prevent. Set comfortably
# below the real count (137 as of this writing) so route churn doesn't require touching this.
MIN_EXPECTED_ROUTES=100
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OPENAPI="$REPO_ROOT/docs/api/openapi.yaml"
COVERAGE_DOC="$REPO_ROOT/docs/audit/api-coverage.md"

result=$(python3 - "$REPO_ROOT" "$OPENAPI" "$COVERAGE_DOC" "$MIN_EXPECTED_ROUTES" <<'PY'
import subprocess, sys, yaml, re

repo_root, openapi_path, coverage_doc_path = sys.argv[1], sys.argv[2], sys.argv[3]
min_expected_routes = int(sys.argv[4])

routes = subprocess.run(
    ["bash", f"{repo_root}/scripts/audit/extract-routes.sh"],
    capture_output=True, text=True, check=True,
).stdout.splitlines()

if len(routes) < min_expected_routes:
    print("EXTRACTION_FAILED")
    print(f"{len(routes)}\t{min_expected_routes}")
    sys.exit(0)

with open(openapi_path, encoding="utf-8") as f:
    spec = yaml.safe_load(f)
documented = set(spec.get("paths", {}).keys())

server_prefix = "/api/v1"

with open(coverage_doc_path, encoding="utf-8") as f:
    coverage_doc = f.read()

missing_rest = []
missing_exclusion = []
for route in routes:
    if route.endswith("(opaque service mount)"):
        prefix = route[: -len("(opaque service mount)")].strip()
        if prefix not in coverage_doc:
            missing_exclusion.append(prefix)
        continue
    if "<unresolved module" in route or "<no pub fn routes" in route:
        # A script-resolution failure, not a doc-coverage gap -- surface it distinctly.
        missing_rest.append(f"{route}  [extract-routes.sh could not resolve this -- fix the script, not the docs]")
        continue
    stripped = route[len(server_prefix):] if route.startswith(server_prefix) else route
    if stripped not in documented:
        missing_rest.append(route)

if missing_rest or missing_exclusion:
    print("FAIL")
    for r in missing_rest:
        print(f"REST\t{r}")
    for r in missing_exclusion:
        print(f"MOUNT\t{r}")
else:
    print(f"OK\t{len(routes)}")
PY
)

status="${result%%$'\n'*}"

if [ "$status" = "EXTRACTION_FAILED" ]; then
  detail="$(echo "$result" | sed -n '2p')"
  found="${detail%%$'\t'*}"
  minimum="${detail#*$'\t'}"
  echo "::error::scripts/audit/extract-routes.sh returned only $found route(s) (expected at least $minimum)." >&2
  echo "This almost certainly means the extraction script itself is broken (e.g. its regex-based Rust" >&2
  echo "router-composition parser no longer matches after a refactor to backend/src/main.rs or an" >&2
  echo "api/**/mod.rs), NOT that the API surface shrank by ~30 routes. Fix scripts/audit/extract-routes.sh" >&2
  echo "(run it standalone -- \`bash scripts/audit/extract-routes.sh\` -- and compare its output against" >&2
  echo "\`grep -rhoE '\\.route\\(\"[^\"]+\"' backend/src\` for a sanity cross-check) before touching any doc." >&2
  exit 1
fi

if [ "$status" = "FAIL" ]; then
  echo "::error::One or more real routes are missing from docs/api/openapi.yaml (or, for opaque mounts, from docs/audit/api-coverage.md's exclusion table):" >&2
  echo "$result" | tail -n +2 | while IFS=$'\t' read -r kind route; do
    if [ "$kind" = "REST" ]; then
      echo "  - $route" >&2
    else
      echo "  - $route (opaque mount, not in the Deliberate Exclusions table)" >&2
    fi
  done
  echo "" >&2
  echo "Fix: add an OpenAPI path entry under docs/api/openapi.yaml's paths: for each REST route above" >&2
  echo "(read the real handler in backend/src/api/**.rs for its request/response shapes -- don't guess" >&2
  echo "from the route name). For an opaque mount, add a row to docs/audit/api-coverage.md's" >&2
  echo "'Deliberate exclusions' table explaining why it can't be an OpenAPI path." >&2
  exit 1
fi

count="${status#OK$'\t'}"
echo "All routes accounted for ($count total: REST routes documented in openapi.yaml or opaque mounts listed in api-coverage.md; 136 REST + 1 mount is the phase-3 baseline)."
