# Release Process

## Overview

Emailibrium uses a tag-based release workflow. Pushing a semver tag (e.g., `v0.1.0`) triggers the automated release pipeline which:

1. Validates the tag format
2. Runs the full CI gate (backend + frontend)
3. Generates a changelog from conventional commits
4. Builds and pushes Docker images to GHCR
5. Creates a GitHub Release with the changelog
6. Auto-updates `CHANGELOG.md` on main

## Quick Release

```bash
# 1. Ensure you're on main with clean working tree
git checkout main
git pull origin main
git status  # should be clean

# 2. Run pre-release checks
make release-check

# 3. Tag and push (triggers the release workflow)
make release VERSION=0.1.0

# Or for pre-releases:
make release VERSION=0.2.0-alpha.1
```

## Step-by-Step

### 1. Pre-release Validation

```bash
make release-check
```

This runs the full `make ci` pipeline: format check, lint, typecheck, and all tests.

### 2. Version Bumping

Update version numbers in:

| File                               | Field                |
| ---------------------------------- | -------------------- |
| `backend/Cargo.toml`               | `version = "0.1.0"`  |
| `frontend/apps/web/package.json`   | `"version": "0.1.0"` |
| `frontend/packages/*/package.json` | `"version": "0.1.0"` |

Then commit:

```bash
git add -A
git commit -m "chore: bump version to 0.1.0"
```

### 3. Tagging

```bash
make release-tag VERSION=0.1.0
```

This creates an annotated git tag `v0.1.0`.

### 4. Push to Trigger Release

```bash
make release-push
```

This pushes the tag to origin, triggering `.github/workflows/release.yml`.

### 5. Monitor the Release

Watch the Actions tab: `https://github.com/pacphi/emailibrium/actions/workflows/release.yml`

The workflow will:

- Run CI gate (format, test, typecheck)
- Generate changelog from commit messages since last tag
- Build `ghcr.io/pacphi/emailibrium/backend:<version>` and `ghcr.io/pacphi/emailibrium/frontend:<version>`
- Create GitHub Release with changelog body
- Push updated CHANGELOG.md to main

## Versioning

We follow [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0): Breaking API changes, major architecture shifts
- **Minor** (0.2.0): New features, non-breaking enhancements
- **Patch** (0.1.1): Bug fixes, dependency updates
- **Pre-release** (0.2.0-alpha.1): Early access, testing

## Commit Conventions

Changelogs are auto-generated from [Conventional Commits](https://www.conventionalcommits.org/):

| Prefix      | Changelog Section | Example                                      |
| ----------- | ----------------- | -------------------------------------------- |
| `feat:`     | Features          | `feat: add subscription detection`           |
| `fix:`      | Bug Fixes         | `fix: correct RRF score calculation`         |
| `perf:`     | Performance       | `perf: optimize HNSW search latency`         |
| `docs:`     | Documentation     | `docs: update deployment guide`              |
| `refactor:` | Refactoring       | `refactor: extract embedding pipeline trait` |
| `test:`     | Testing           | `test: add clustering evaluation suite`      |
| `ci:`       | CI/CD             | `ci: add shellcheck validation`              |
| `deps:`     | Dependencies      | `deps: upgrade axum to 0.8.8`                |
| `chore:`    | Maintenance       | `chore: bump version to 0.2.0`               |

## Docker Images

Release images are published to GitHub Container Registry:

```bash
# Pull specific version
docker pull ghcr.io/pacphi/emailibrium/backend:0.1.0
docker pull ghcr.io/pacphi/emailibrium/frontend:0.1.0

# Pull latest stable
docker pull ghcr.io/pacphi/emailibrium/backend:latest
docker pull ghcr.io/pacphi/emailibrium/frontend:latest
```

Stable releases get three tags: `0.1.0`, `0.1`, and `latest`.
Pre-releases get only the exact version tag.

## Rollback

To rollback to a previous release:

```bash
# Revert to previous Docker images
docker compose pull  # pulls :latest
# Or pin a specific version in docker-compose.yml

# Or checkout a previous tag
git checkout v0.1.0
make install
make dev
```

## Useful Commands

| Command                          | Description                         |
| -------------------------------- | ----------------------------------- |
| `make release-check`             | Run full CI pipeline                |
| `make release-tag VERSION=x.y.z` | Create annotated tag                |
| `make release-push`              | Push latest tag to origin           |
| `make release VERSION=x.y.z`     | All three in sequence               |
| `make changelog VERSION=x.y.z`   | Preview changelog without releasing |
