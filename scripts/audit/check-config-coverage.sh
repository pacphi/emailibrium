#!/usr/bin/env bash
# check-config-coverage.sh — fail if any real config key is undocumented.
#
# The docs-accuracy drift gate (phase 6 of the docs-accuracy-audit pipeline). Runs
# extract-config-keys.sh for the ground truth, then checks every key has a corresponding
# mention in docs/configuration-reference.md. This is the CI-safe form of the check
# docs-accuracy-audit's own phase 2 used by hand to reconcile that file in the first place.
#
# Deliberately NOT the inverse (documented-but-nonexistent keys aren't checked) -- that
# direction has no reliable ground truth to check against and isn't what silently breaks a
# reader: an undocumented key is a real knob nobody can discover; a stray doc row for a
# removed key is merely stale prose, caught by human review instead.
#
# A key counts as documented only if it appears backtick-wrapped (`key`) somewhere in the
# doc -- not merely as a substring. An unanchored substring match (a first draft of this
# script used exactly that) produces real, silent false negatives: `host` "matches" via
# `localhost`, `port` via "exports"/"report-uri", and an unescaped `.` in a dotted key like
# `embedding.model` is a regex wildcard that matches unrelated prose ("embedding·model") or
# even a DIFFERENT key that differs only by `.` vs `_` (`security.hsts.max_age_secs` vs.
# `security.hsts_max_age_secs`). Every one of those was proven with a real key deleted from
# a scratch copy of the doc during phase 6's own adversarial review -- see
# .autopilot/court/docs-accuracy-audit/phase-6.md.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DOC="$REPO_ROOT/docs/configuration-reference.md"

undocumented=()
while IFS= read -r prefixed_key; do
  key="${prefixed_key#yaml:}"
  key="${key#app-yaml:}"
  key="${key#env:}"
  escaped_key=$(printf '%s' "$key" | sed 's/[.[\*^$/]/\\&/g')
  grep -qE -- "\`${escaped_key}\`" "$DOC" || undocumented+=("$prefixed_key")
done < <(bash "$REPO_ROOT/scripts/audit/extract-config-keys.sh")

if [ "${#undocumented[@]}" -gt 0 ]; then
  echo "::error::${#undocumented[@]} config key(s) exist in code but are not documented in docs/configuration-reference.md:" >&2
  for k in "${undocumented[@]}"; do
    echo "  - $k" >&2
  done
  echo "" >&2
  echo "Fix: add a row for each key above to the appropriate table under docs/configuration-reference.md's" >&2
  echo "'Complete Key Reference' section (yaml:/app-yaml: keys) or 'Environment Variables' section (env: keys)." >&2
  echo "See scripts/audit/extract-config-keys.sh's header comment for what each prefix means and where the" >&2
  echo "key is read from (backend/config.yaml, config/app.yaml, or a literal env::var() call)." >&2
  exit 1
fi

echo "All config keys documented ($(bash "$REPO_ROOT/scripts/audit/extract-config-keys.sh" | wc -l | tr -d ' ') checked)."
