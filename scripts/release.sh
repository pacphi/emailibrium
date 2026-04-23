#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-}"
[ -n "$VERSION" ] || { echo "Usage: $0 <version>" >&2; exit 1; }

# Validate version format
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]] || {
  echo "Error: version must be X.Y.Z or X.Y.Z-prerelease" >&2; exit 1
}

# Pre-flight checks
[[ "$(git branch --show-current)" == "main" ]] || { echo "Error: must be on main" >&2; exit 1; }
[[ -z "$(git status --porcelain)" ]] || { echo "Error: working tree dirty" >&2; exit 1; }
git fetch origin main --quiet
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || {
  echo "Error: not in sync with origin/main" >&2; exit 1
}
git rev-parse "v$VERSION" &>/dev/null && { echo "Error: tag v$VERSION already exists" >&2; exit 1; }

echo "Cutting release v$VERSION..."

# 1. Bump backend Cargo.toml
(cd backend && cargo set-version "$VERSION")

# 2. Bump all frontend packages
for pkg in frontend/apps/web frontend/packages/ui frontend/packages/types frontend/packages/core frontend/packages/api; do
  jq --arg v "$VERSION" '.version = $v' "$pkg/package.json" > /tmp/_emailibrium_pkg.json
  mv /tmp/_emailibrium_pkg.json "$pkg/package.json"
done

# 3. Refresh Cargo.lock
(cd backend && cargo check --quiet)

# 4. Regenerate CHANGELOG.md
git-cliff --tag "v$VERSION" --output CHANGELOG.md
# Format to satisfy pre-commit prettier check
if command -v npx >/dev/null 2>&1; then
  npx --no-install prettier --write --config .prettierrc CHANGELOG.md >/dev/null 2>&1 || \
    npx prettier --write --config .prettierrc CHANGELOG.md >/dev/null
fi

# 5. Stage changes
git add backend/Cargo.toml \
  frontend/apps/web/package.json \
  frontend/packages/ui/package.json \
  frontend/packages/types/package.json \
  frontend/packages/core/package.json \
  frontend/packages/api/package.json \
  backend/Cargo.lock \
  CHANGELOG.md

read -rp "Commit and tag v$VERSION? [y/N] " yn
[[ "$yn" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 0; }

git commit -m "chore(release): v$VERSION"
git tag "v$VERSION"

read -rp "Push main and tag to origin? [y/N] " yn
[[ "$yn" =~ ^[Yy]$ ]] || { echo "Committed and tagged locally. Push manually when ready."; exit 0; }

git push origin main
git push origin "v$VERSION"
echo "Done. Release v$VERSION triggered on CI."
