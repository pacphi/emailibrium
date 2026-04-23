#!/usr/bin/env bash
# Run cargo audit with exceptions sourced from backend/audit-ignore.
# Mirrors the pattern used in the sindri project.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IGNORE_FILE="$REPO_ROOT/backend/audit-ignore"

if [[ ! -f "$IGNORE_FILE" ]]; then
  echo "No audit-ignore file found at $IGNORE_FILE"
  echo "Running cargo audit without exceptions..."
  cd "$REPO_ROOT/backend"
  exec cargo audit --deny warnings "$@"
fi

# Build --ignore flags from the exceptions file (strips comments + blank lines)
IGNORE_FLAGS=()
ADVISORY_IDS=()
while IFS= read -r line; do
  line="${line%%#*}"
  line="$(echo "$line" | xargs)"
  [[ -z "$line" ]] && continue
  IGNORE_FLAGS+=(--ignore "$line")
  ADVISORY_IDS+=("$line")
done < "$IGNORE_FILE"

if [[ ${#ADVISORY_IDS[@]} -gt 0 ]]; then
  echo "Ignoring ${#ADVISORY_IDS[@]} documented advisory exception(s):"
  for id in "${ADVISORY_IDS[@]}"; do
    echo "  - $id"
  done
fi

cd "$REPO_ROOT/backend"
exec cargo audit --deny warnings "${IGNORE_FLAGS[@]}" "$@"
