#!/usr/bin/env bash
# extract-config-keys.sh — emit the backend's ACTUAL configuration surface.
#
# Ground truth for docs/configuration-reference.md (phase 2 of the docs-accuracy-audit
# pipeline). Reproducible, deterministic, no network, no prompts — safe to run in CI (phase 6
# reuses it as the drift gate).
#
# Three sources, each labeled so a doc-reconciler knows which mechanism a key belongs to:
#
#   yaml:<dotted.path>   — a key in backend/config.yaml (VectorConfig, loaded via Figment:
#                           config.yaml -> config.{APP_ENV}.yaml -> config.local.yaml -> env).
#                           Every key here is ALSO settable as EMAILIBRIUM_<PATH_UPPERCASED>
#                           (dots -> underscores) per backend/config.yaml's own header comment.
#   app-yaml:<dotted.path> — a key in config/app.yaml (YamlConfig, loaded directly, no Figment
#                           env-override layer as of this writing — see vectors/yaml_config.rs
#                           if that changes).
#   env:<NAME>           — a literal environment variable name read directly via
#                           std::env::var("NAME") / env::var("NAME") somewhere in backend/src,
#                           independent of the two YAML-driven mechanisms above.
#
# Output: one key per line, sorted and de-duplicated within each source.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

flatten_yaml() {
  local file="$1" label="$2"
  python3 - "$file" "$label" <<'PY'
import sys, yaml

path, label = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as f:
    data = yaml.safe_load(f) or {}

def walk(node, prefix):
    if isinstance(node, dict):
        for k, v in node.items():
            walk(v, f"{prefix}.{k}" if prefix else str(k))
    else:
        print(f"{label}:{prefix}")

walk(data, "")
PY
}

flatten_yaml "$REPO_ROOT/backend/config.yaml" "yaml" | sort -u
flatten_yaml "$REPO_ROOT/config/app.yaml" "app-yaml" | sort -u

# Literal std::env::var("X") / env::var("X") reads across the backend, excluding the
# EMAILIBRIUM_-prefixed Figment override path (already covered by the yaml: keys above via
# the documented dots->underscores convention) so each knob is reported exactly once.
grep -rhoE '(std::)?env::var\("[A-Za-z0-9_]+"' "$REPO_ROOT/backend/src" \
  | sed -E 's/^(std::)?env::var\("//; s/"$//' \
  | grep -v '^EMAILIBRIUM_' \
  | sed 's/^/env:/' \
  | sort -u
