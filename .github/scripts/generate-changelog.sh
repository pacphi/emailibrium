#!/usr/bin/env bash
# Changelog Generation Script for Emailibrium
# Usage: ./generate-changelog.sh <version> [output-file]
#
# Examples:
#   ./generate-changelog.sh 0.1.0 changelog.md
#   ./generate-changelog.sh 0.2.0-alpha.1

set -euo pipefail

VERSION="${1:?Usage: $0 <version> [output-file]}"
OUTPUT_FILE="${2:-changelog.md}"
CURRENT_TAG="v${VERSION}"
REPO="${GITHUB_REPOSITORY:-pacphi/emailibrium}"

echo "Generating changelog for $CURRENT_TAG" >&2

# Find previous tag
PREVIOUS_TAG=$(git tag -l "v*" --sort=-version:refname | grep -A1 "^${CURRENT_TAG}$" | tail -1) || true

if [[ -z "$PREVIOUS_TAG" || "$PREVIOUS_TAG" == "$CURRENT_TAG" ]]; then
  # First release or no previous tag found
  PREVIOUS_TAG=$(git rev-list --max-parents=0 HEAD | head -1)
  COMMIT_RANGE="${PREVIOUS_TAG}..${CURRENT_TAG}"
  echo "First release — using all commits" >&2
else
  COMMIT_RANGE="${PREVIOUS_TAG}..${CURRENT_TAG}"
  echo "Generating diff: $PREVIOUS_TAG -> $CURRENT_TAG" >&2
fi

# Categorize commits by conventional commit prefix
declare -A CATEGORIES
CATEGORIES=(
  ["feat"]="Features"
  ["fix"]="Bug Fixes"
  ["perf"]="Performance"
  ["refactor"]="Refactoring"
  ["docs"]="Documentation"
  ["test"]="Testing"
  ["ci"]="CI/CD"
  ["deps"]="Dependencies"
  ["chore"]="Maintenance"
)

# Start output
{
  echo "## [${VERSION}] - $(date +%Y-%m-%d)"
  echo ""

  for prefix in feat fix perf refactor docs test ci deps chore; do
    section="${CATEGORIES[$prefix]}"
    commits=$(git log "$COMMIT_RANGE" --pretty=format:"- %s (%h)" --grep="^${prefix}" 2>/dev/null || true)

    if [[ -n "$commits" ]]; then
      echo "### ${section}"
      echo ""
      echo "$commits"
      echo ""
    fi
  done

  # Uncategorized commits
  uncategorized=$(git log "$COMMIT_RANGE" --pretty=format:"%s" \
    --invert-grep --grep="^feat" --grep="^fix" --grep="^perf" \
    --grep="^refactor" --grep="^docs" --grep="^test" --grep="^ci" \
    --grep="^deps" --grep="^chore" 2>/dev/null || true)

  if [[ -n "$uncategorized" ]]; then
    echo "### Other Changes"
    echo ""
    git log "$COMMIT_RANGE" --pretty=format:"- %s (%h)" \
      --invert-grep --grep="^feat" --grep="^fix" --grep="^perf" \
      --grep="^refactor" --grep="^docs" --grep="^test" --grep="^ci" \
      --grep="^deps" --grep="^chore" 2>/dev/null || true
    echo ""
  fi

  # Installation section
  echo "### Installation"
  echo ""
  echo '```bash'
  echo "# Docker"
  echo "docker compose pull"
  echo "docker compose up -d"
  echo ""
  echo "# From source"
  echo "git checkout v${VERSION}"
  echo "make install"
  echo "make dev"
  echo '```'
  echo ""
  echo "**Full Changelog**: https://github.com/${REPO}/compare/${PREVIOUS_TAG}...${CURRENT_TAG}"

} > "$OUTPUT_FILE"

echo "Changelog written to $OUTPUT_FILE" >&2
