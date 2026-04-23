# Release Process

## Overview

Emailibrium uses a tag-based release workflow. Pushing a semver tag (e.g., `v0.1.0`) triggers the automated release pipeline which:

1. Validates the tag format
2. Runs the full CI gate (backend format + tests; frontend typecheck, format, lint, and unit tests)
3. Generates a changelog from conventional commits
4. Builds and pushes Docker images to GHCR
5. Creates a GitHub Release with the changelog
6. Auto-updates `CHANGELOG.md` on main

## Quick Release

**One-time setup:**

```bash
cargo install cargo-edit  # provides `cargo set-version`
cargo install git-cliff   # provides changelog generation
```

**Cut a release (single command):**

```bash
make release VERSION=0.1.0

# Or for pre-releases:
make release VERSION=0.2.0-alpha.1
```

`make release` handles all version bumping, changelog regeneration, git commit, tag, and push.

## Step-by-Step

### 1. One-time Setup

Install the required tools if not already present:

```bash
cargo install cargo-edit
cargo install git-cliff
```

### 2. Version Bumping (automated)

`make release VERSION=x.y.z` automatically updates:

| File                               | Field                |
| ---------------------------------- | -------------------- |
| `backend/Cargo.toml`               | `version = "x.y.z"`  |
| `frontend/apps/web/package.json`   | `"version": "x.y.z"` |
| `frontend/packages/*/package.json` | `"version": "x.y.z"` |

It also refreshes `backend/Cargo.lock` and regenerates `CHANGELOG.md` via git-cliff.

### 3. Script Pre-flight Checks

Before making any changes, `scripts/release.sh` validates:

- You are on the `main` branch
- Working tree is clean
- Local `main` is in sync with `origin/main`
- The target tag does not already exist

### 4. Commit, Tag, and Push

After bumping versions and regenerating the changelog the script prompts:

```
Commit and tag v0.1.0? [y/N]
Push main and tag to origin? [y/N]
```

Answering `y` to both triggers `.github/workflows/release.yml`.

### 5. Monitor the Release

Watch the Actions tab: `https://github.com/pacphi/emailibrium/actions/workflows/release.yml`

The workflow will:

- Verify the tag version matches `backend/Cargo.toml` and `frontend/apps/web/package.json`
- Run CI gate (backend format + test, frontend typecheck + format + lint + vitest)
- Generate release notes via git-cliff (`--latest`)
- Build `ghcr.io/pacphi/emailibrium/backend:<version>` and `ghcr.io/pacphi/emailibrium/frontend:<version>`
- Create GitHub Release with the generated notes
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

| Command                          | Description                                                   |
| -------------------------------- | ------------------------------------------------------------- |
| `make release-check`             | Run full CI pipeline                                          |
| `make release-tag VERSION=x.y.z` | Create annotated tag                                          |
| `make release-push`              | Push latest tag to origin                                     |
| `make release VERSION=x.y.z`     | Full one-command release (bump, changelog, commit, tag, push) |
| `make changelog`                 | Regenerate CHANGELOG.md via git-cliff                         |
| `make release-check`             | Run full CI pipeline only                                     |
| `make release-tag VERSION=x.y.z` | Create annotated tag only                                     |
| `make release-push`              | Push latest tag to origin only                                |
