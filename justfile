# ============================================================================
# Emailibrium — Root justfile
# ============================================================================
# Delegates to backend/justfile and frontend/justfile.
# Provides cross-cutting recipes for CI, Docker, releases, and docs.
#
# Quick Start:
#   just                      - Show all available recipes (help)
#   just --list               - Terse auto-generated recipe list
#   just install              - Install all dependencies
#   just dev                  - Start full stack (native)
#   just docker-up-dev        - Start full stack (Docker)
#   Database: SQLite by default everywhere. PostgreSQL is one flag — see `just help`.
#   just ci                   - Run full CI pipeline
#   just VERSION=x.y.z release - Tag and release
# ============================================================================

# ============================================================================
# Settings, Variables and Configuration
# ============================================================================

# Match the Makefile's `SHELL := /bin/bash` — several recipes use bash-isms
# ([[ ]], &>, ${!var}). just's default shell is `sh -cu`, which would break them.
set shell := ["bash", "-c"]

BACKEND_DIR  := "backend"
FRONTEND_DIR := "frontend"

COMPOSE     := "docker compose"
COMPOSE_DEV := COMPOSE + " -f docker-compose.yml -f docker-compose.dev.yml"

# Delegation prefixes. just does not have make's `-C`; --justfile picks the file
# and --working-directory sets the cwd the sub-recipes run in.
BACKEND  := "just --justfile " + BACKEND_DIR + "/justfile --working-directory " + BACKEND_DIR
FRONTEND := "just --justfile " + FRONTEND_DIR + "/justfile --working-directory " + FRONTEND_DIR

# Empty when lychee is not on PATH (link checking is optional — see links-check).
LYCHEE := `command -v lychee 2>/dev/null || echo ""`

# Colors. `|| echo ''` is a terminal-capability probe, not an error mask: tput
# fails when TERM is unset (CI, pipes) and we degrade to uncolored output.
BOLD   := `tput bold 2>/dev/null || echo ''`
GREEN  := `tput setaf 2 2>/dev/null || echo ''`
YELLOW := `tput setaf 3 2>/dev/null || echo ''`
BLUE   := `tput setaf 4 2>/dev/null || echo ''`
RED    := `tput setaf 1 2>/dev/null || echo ''`
RESET  := `tput sgr0 2>/dev/null || echo ''`

# Argument-style variables, so `just VERSION=0.1.0 release` works like
# `make release VERSION=0.1.0`. The recipes below also accept the version /
# model positionally, and tolerate a literal `VERSION=`/`MODEL=` prefix.
VERSION := ""
MODEL   := ""

# find with -prune avoids traversing multi-GB Rust target/ and node_modules/ dirs
# (prettier's own glob walker enters all dirs before filtering via .prettierignore)
# Keep this list in sync with .markdownlint-cli2.jsonc's `ignores` — otherwise
# format-check-md walks agent-tool scaffolding (.agents/, .codex/, .optimizer/)
# that lint-md correctly skips, and the two checks disagree about the same file.
PRUNE_DIRS := '\( -name node_modules -o -name target -o -name ruvector -o -name .claude -o -name .claude-flow -o -name .git -o -name .agentic-qe -o -name .swarm -o -name .beads -o -name .agents -o -name .codex -o -name .optimizer -o -name .git-rewrite -o -name dist -o -name storybook-static -o -name coverage \) -prune'

# ============================================================================
# Default Recipe
# ============================================================================

# Show all available recipes, grouped
[group('help')]
help:
    #!/usr/bin/env bash
    set -euo pipefail
    cat <<'HELP'
    {{BOLD}}{{BLUE}}╔════════════════════════════════════════════════════════════════════╗{{RESET}}
    {{BOLD}}{{BLUE}}║                      Emailibrium justfile                          ║{{RESET}}
    {{BOLD}}{{BLUE}}╚════════════════════════════════════════════════════════════════════╝{{RESET}}

    {{BOLD}}Quick Start:{{RESET}}
      just setup             - Guided first-time setup wizard
      just install           - Install all dependencies
      just dev               - Start backend + frontend (native)
      just dev-llm           - Start with built-in LLM (llama.cpp)
      just models            - Show available LLM models
      just embedding-models  - Show available embedding models
      just download-model    - Download a model (MODEL=<id>)
      just docker-up-dev     - Start full stack (Docker)
      just ci                - Run full CI pipeline
      just test              - Run all tests

    {{BOLD}}Database (SQLite by default, PostgreSQL is one flag):{{RESET}}
      native   EMAILIBRIUM_DATABASE_URL=postgres://user:pw@localhost:5432/emailibrium just dev
               (same variable for dev-llm, and for `cargo test` / `cargo run` directly)
      docker   just docker-up-postgres          (or docker-up-dev-postgres)
               — derives the URL from secrets/$APP_ENV/db_password; export
                 EMAILIBRIUM_DATABASE_URL first to point at an external PostgreSQL.
      The URL's scheme IS the selector (ADR-033) — sqlite:… or postgres://…; there is
      no separate backend flag. In Docker the --profile postgres flag those recipes
      pass is what starts the database container the URL points at.

    {{BOLD}}{{BLUE}}═══ Setup & Onboarding ═════════════════════════════════════════════{{RESET}}
      setup                  - Guided first-time setup wizard
      setup-prereqs          - Check all prerequisites
      setup-secrets          - Generate/configure secrets
      setup-ai               - Configure AI providers
      setup-docker           - Set up Docker environment
      setup-validate         - Validate entire setup

    {{BOLD}}{{BLUE}}═══ Install & Build ═════════════════════════════════════════════════{{RESET}}
      install                - Install all dependencies (backend + frontend)
      build                  - Build everything
      dev                    - Start full stack dev servers (native)
      dev-llm                - Start with built-in LLM (llama.cpp)
      clean                  - Clean all build artifacts
      clean-data             - Remove all local data (DB, vectors)
      clean-all              - Clean build artifacts + all local data

    {{BOLD}}{{BLUE}}═══ AI & Models ═════════════════════════════════════════════════════{{RESET}}
      models                 - Show available LLM models
      embedding-models       - Show available embedding models
      download-model MODEL=x - Download a specific model
      download-models        - Download AI models (ONNX + GGUF)
      diagnose               - Show AI configuration diagnostics

    {{BOLD}}{{BLUE}}═══ Test ════════════════════════════════════════════════════════════{{RESET}}
      test                   - Run all tests (backend + frontend)

    {{BOLD}}{{BLUE}}═══ Lint & Format ═══════════════════════════════════════════════════{{RESET}}
      lint                   - Lint everything (code + docs)
      format                 - Format everything (code + docs)
      format-check           - Check formatting (no changes)
      typecheck              - TypeScript type check

    {{BOLD}}{{BLUE}}═══ Security & Quality ═════════════════════════════════════════════{{RESET}}
      audit                  - Security audit all dependencies
      deadcode               - Check for dead code
      ci                     - Full CI pipeline
      ci-full                - CI + link checking

    {{BOLD}}{{BLUE}}═══ Dependency Management ══════════════════════════════════════════{{RESET}}
      upgrade                - Upgrade all deps (within semver)
      outdated               - Show outdated deps (no changes)

    {{BOLD}}{{BLUE}}═══ Documentation ══════════════════════════════════════════════════{{RESET}}
      lint-md                - Lint Markdown files
      lint-yaml              - Lint YAML files
      links-check            - Check internal links in Markdown
      links-check-external   - Check external links (slow)
      links-check-all        - Check all links

    {{BOLD}}{{BLUE}}═══ Docker ═════════════════════════════════════════════════════════{{RESET}}
      docker-up              - Start production stack (SQLite)
      docker-up-postgres     - Start production stack against PostgreSQL
      docker-up-dev          - Start dev stack (hot-reload, SQLite)
      docker-up-dev-postgres - Start dev stack against PostgreSQL
      docker-down            - Stop all containers (incl. the postgres profile)
      docker-down-volumes    - Stop + remove volumes (DESTROYS DATA)
      docker-restart         - Restart all containers (SQLite)
      docker-restart-postgres - Restart all containers against PostgreSQL
      docker-build           - Build Docker images
      docker-build-no-cache  - Build images without cache
      docker-logs            - Tail all container logs
      docker-logs-backend    - Tail backend logs
      docker-logs-frontend   - Tail frontend logs
      docker-ps              - Show container status
      docker-exec-backend    - Shell into backend container
      docker-exec-frontend   - Shell into frontend container
      docker-health          - Health check all containers
      docker-clean           - Prune dangling Docker artifacts
      docker-secrets         - Generate dev secrets

    {{BOLD}}{{BLUE}}═══ Release ════════════════════════════════════════════════════════{{RESET}}
      release-check          - Pre-release CI validation
      release-tag VERSION=x.y.z - Create annotated tag
      release-push           - Push latest tag to trigger release
      release VERSION=x.y.z    - Full release (check + tag + push)
      changelog                - Regenerate CHANGELOG.md

      Run '{{BOLD}}cd backend && just --list{{RESET}}' or '{{BOLD}}cd frontend && just --list{{RESET}}' for layer-specific recipes.
    HELP

# ============================================================================
# Setup & Onboarding
# ============================================================================

# Guided first-time setup wizard
[group('setup')]
setup:
    @bash scripts/setup.sh

# Check all prerequisites
[group('setup')]
setup-prereqs:
    @bash scripts/setup-prereqs.sh

# Generate/configure secrets
[group('setup')]
setup-secrets:
    @bash scripts/setup-secrets.sh

# Configure AI providers
[group('setup')]
setup-ai:
    @bash scripts/setup-ai.sh

# Set up Docker environment
[group('setup')]
setup-docker:
    @bash scripts/setup-docker.sh

# Validate entire setup
[group('setup')]
setup-validate:
    @bash scripts/setup-validate.sh

# ============================================================================
# AI & Models
# ============================================================================

# Download AI models (ONNX embedding + GGUF LLM)
[group('ai')]
download-models:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{BOLD}}{{BLUE}}Downloading AI models...{{RESET}}"
    echo "{{GREEN}}Step 1:{{RESET}} ONNX embedding model"
    # The Makefile hid the real error here (`2>/dev/null || echo hint`) and still
    # exited 0. Keep the hint, but surface stderr and fail honestly.
    if ! (cd {{BACKEND_DIR}} && cargo run -- --download-models); then
        echo "  {{YELLOW}}Backend not built. Run 'just build' first.{{RESET}}" >&2
        exit 1
    fi
    echo "{{GREEN}}Step 2:{{RESET}} GGUF LLM model (qwen2.5-0.5b-q4km)"
    if ! (cd {{FRONTEND_DIR}}/apps/web && npx tsx ../../../scripts/models.ts download --default); then
        echo "  {{YELLOW}}Frontend not installed. Run 'just install' first.{{RESET}}" >&2
        exit 1
    fi
    echo "{{GREEN}}Done.{{RESET}} Models cached for offline use."

# Show AI configuration diagnostics
[group('ai')]
diagnose:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{BOLD}}{{BLUE}}Emailibrium AI Diagnostics{{RESET}}"
    echo "────────────────────────────────────────"
    echo ""
    echo "{{BOLD}}Embedding:{{RESET}}"
    if [[ -d "{{BACKEND_DIR}}/.fastembed_cache" ]]; then
        echo "  Provider: ONNX (all-MiniLM-L6-v2)"
        echo "  Status:   {{GREEN}}cached{{RESET}} ($(du -sh {{BACKEND_DIR}}/.fastembed_cache 2>/dev/null | cut -f1))"
    else
        echo "  Provider: ONNX (all-MiniLM-L6-v2)"
        echo "  Status:   {{YELLOW}}not cached (downloads on first use){{RESET}}"
    fi
    echo ""
    echo "{{BOLD}}Generative (LLM):{{RESET}}"
    CACHE="$HOME/.emailibrium/models/llm"
    if [[ -d "$CACHE" ]] && find "$CACHE" -name "*.gguf" -print -quit 2>/dev/null | grep -q .; then
        LLM_MODEL=$(find "$CACHE" -name "*.gguf" -print -quit 2>/dev/null | xargs basename)
        SIZE=$(du -sh "$CACHE" 2>/dev/null | cut -f1)
        echo "  Provider: builtin ($LLM_MODEL)"
        echo "  Status:   {{GREEN}}cached{{RESET}} ($SIZE)"
    else
        echo "  Provider: builtin (qwen2.5-0.5b-q4km)"
        echo "  Status:   {{YELLOW}}not cached{{RESET}}"
        echo "  Fix:      just download-models"
    fi
    echo ""
    echo "{{BOLD}}Ollama:{{RESET}}"
    if command -v ollama &>/dev/null && ollama list &>/dev/null 2>&1; then
        echo "  Status: {{GREEN}}running{{RESET}}"
    elif command -v ollama &>/dev/null; then
        echo "  Status: {{YELLOW}}installed but not running{{RESET}}"
    else
        echo "  Status: not installed (optional)"
    fi
    echo ""
    echo "{{BOLD}}Cloud APIs:{{RESET}}"
    for var in EMAILIBRIUM_OPENAI_API_KEY EMAILIBRIUM_ANTHROPIC_API_KEY EMAILIBRIUM_GEMINI_API_KEY; do
        name=$(echo "$var" | sed 's/EMAILIBRIUM_//;s/_API_KEY//')
        if [[ -n "${!var:-}" ]]; then
            echo "  $name: {{GREEN}}configured{{RESET}}"
        else
            echo "  $name: not configured"
        fi
    done
    echo ""
    echo "{{BOLD}}Database:{{RESET}}"
    if [[ -f "{{BACKEND_DIR}}/emailibrium-dev.db" ]]; then
        echo "  Status: {{GREEN}}exists{{RESET}} ($(du -sh {{BACKEND_DIR}}/emailibrium-dev.db 2>/dev/null | cut -f1))"
    else
        echo "  Status: not created yet (created on first run)"
    fi

# Show available LLM models with hardware recommendations
[group('ai')]
models:
    @{{BACKEND}} models

# Show available embedding models
[group('ai')]
embedding-models:
    @{{BACKEND}} embedding-models

# Download a model (e.g. just download-model qwen3-8b-q4km)
[group('ai')]
download-model model=MODEL:
    #!/usr/bin/env bash
    set -euo pipefail
    # Accept `just download-model qwen3-8b-q4km`, `just download-model MODEL=qwen3-8b-q4km`
    # and `just MODEL=qwen3-8b-q4km download-model`.
    m="{{model}}"; m="${m#MODEL=}"
    if [[ -z "$m" ]]; then
        echo "{{YELLOW}}Usage: just download-model MODEL=<id>   (see: just models){{RESET}}" >&2
        exit 1
    fi
    {{BACKEND}} download-model "$m"

# ============================================================================
# Install & Build
# ============================================================================

# Install all dependencies (backend `build` is its dependency-fetch step)
[group('build')]
install:
    @{{BACKEND}} build
    @{{FRONTEND}} install

# Build everything
[group('build')]
build:
    @{{BACKEND}} build
    @{{FRONTEND}} build

# Start full stack dev servers (native, loads secrets/dev/ as env vars)
[group('build')]
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{GREEN}}Backend: http://localhost:8080  Frontend: http://localhost:3000{{RESET}}"
    # Missing secret files yield empty values by design (dev works before
    # `just setup-secrets`); an unreadable file still fails loudly under set -e.
    secret() { local f="secrets/dev/$1"; if [[ -f "$f" ]]; then cat "$f"; else printf ''; fi; }
    export EMAILIBRIUM_GOOGLE_CLIENT_ID="$(secret google_client_id)"
    export EMAILIBRIUM_GOOGLE_CLIENT_SECRET="$(secret google_client_secret)"
    export EMAILIBRIUM_MICROSOFT_CLIENT_ID="$(secret microsoft_client_id)"
    export EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="$(secret microsoft_client_secret)"
    export JWT_SECRET="$(secret jwt_secret)"
    export EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD="$(secret oauth_encryption_key)"
    export RATE_LIMIT_PRESET=development
    trap 'kill 0' INT TERM EXIT
    {{BACKEND}} dev &
    {{FRONTEND}} dev &
    wait

# Start full stack with built-in LLM (downloads ~350MB model on first run)
[group('build')]
dev-llm:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{GREEN}}Backend (LLM): http://localhost:8080  Frontend: http://localhost:3000{{RESET}}"
    secret() { local f="secrets/dev/$1"; if [[ -f "$f" ]]; then cat "$f"; else printf ''; fi; }
    export EMAILIBRIUM_GOOGLE_CLIENT_ID="$(secret google_client_id)"
    export EMAILIBRIUM_GOOGLE_CLIENT_SECRET="$(secret google_client_secret)"
    export EMAILIBRIUM_MICROSOFT_CLIENT_ID="$(secret microsoft_client_id)"
    export EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="$(secret microsoft_client_secret)"
    export JWT_SECRET="$(secret jwt_secret)"
    export EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD="$(secret oauth_encryption_key)"
    export RATE_LIMIT_PRESET=development
    trap 'kill 0' INT TERM EXIT
    {{BACKEND}} dev-llm &
    {{FRONTEND}} dev &
    wait

# Clean all build artifacts
[group('build')]
clean:
    @{{BACKEND}} clean
    @{{FRONTEND}} clean

# Remove all local data (DB, vectors) — fresh start
[group('build')]
clean-data:
    @{{BACKEND}} clean-data

# Clean build artifacts + all local data
[group('build')]
clean-all:
    @{{BACKEND}} clean-all
    @{{FRONTEND}} clean

# ============================================================================
# Test
# ============================================================================

# Run all tests
[group('test')]
test:
    @{{BACKEND}} test
    @{{FRONTEND}} test

# ============================================================================
# Lint & Format
# ============================================================================

# Lint everything (code + docs)
[group('lint')]
lint: lint-docs
    @{{BACKEND}} lint
    @{{FRONTEND}} lint

# Format everything (code + docs)
[group('lint')]
format: format-docs
    @{{BACKEND}} format
    @{{FRONTEND}} format

# Check formatting (no changes)
[group('lint')]
format-check: format-check-docs
    @{{BACKEND}} format-check
    @{{FRONTEND}} format-check

# Type check (frontend)
[group('lint')]
typecheck:
    @{{FRONTEND}} typecheck

# ============================================================================
# Security & Quality
# ============================================================================

# Security audit all dependencies
[group('security')]
audit:
    @{{BACKEND}} audit
    @{{FRONTEND}} audit

# Check for dead code
[group('security')]
deadcode:
    @{{BACKEND}} deadcode
    @{{FRONTEND}} deadcode

# Full CI pipeline
[group('security')]
ci: format-check lint typecheck test

# Full CI + link checking
[group('security')]
ci-full: ci links-check

# ============================================================================
# Dependency Management
# ============================================================================

# Upgrade all dependencies (within semver)
[group('deps')]
upgrade:
    @{{BACKEND}} upgrade
    @{{FRONTEND}} upgrade

# Show outdated dependencies (no changes)
[group('deps')]
outdated:
    @{{BACKEND}} outdated
    @{{FRONTEND}} outdated

# ============================================================================
# Documentation (Markdown, YAML, Links)
# ============================================================================

# Lint Markdown files (strict — fails on errors or missing tool)
[group('docs')]
lint-md:
    @echo "{{GREEN}}Linting Markdown...{{RESET}}"
    @command -v markdownlint-cli2 >/dev/null 2>&1 || { echo "{{RED}}markdownlint-cli2 not installed. Run: npm i -g markdownlint-cli2{{RESET}}"; exit 1; }
    @markdownlint-cli2 '**/*.md' '#**/node_modules' '#**/target' '#.claude/worktrees/**' '#ruvector/**'

# Lint YAML files (strict — fails on errors or missing tool)
[group('docs')]
lint-yaml:
    @echo "{{GREEN}}Linting YAML...{{RESET}}"
    @command -v yamllint >/dev/null 2>&1 || { echo "{{RED}}yamllint not installed. Run: pip install yamllint{{RESET}}"; exit 1; }
    @find . \( -name node_modules -o -name target -o -name ruvector -o -name .claude -o -name .claude-flow \) -prune -o \( -name '*.yaml' -o -name '*.yml' \) ! -name 'pnpm-lock.yaml' -print | xargs -r yamllint -c .yamllint.yaml

# Lint all docs (Markdown + YAML)
[group('docs')]
lint-docs: lint-md lint-yaml

# Format Markdown files
[group('docs')]
format-md:
    @find . {{PRUNE_DIRS}} -o -name '*.md' -print | xargs npx prettier --write --no-error-on-unmatched-pattern

# Format YAML files
[group('docs')]
format-yaml:
    @find . {{PRUNE_DIRS}} -o \( -name '*.yaml' -o -name '*.yml' \) ! -name 'pnpm-lock.yaml' -print | xargs npx prettier --write --no-error-on-unmatched-pattern

# Format docs (Markdown + YAML)
[group('docs')]
format-docs: format-md format-yaml

# Check Markdown formatting (no changes)
[group('docs')]
format-check-md:
    @find . {{PRUNE_DIRS}} -o -name '*.md' -print | xargs npx prettier --check --no-error-on-unmatched-pattern

# Check YAML formatting (no changes)
[group('docs')]
format-check-yaml:
    @find . {{PRUNE_DIRS}} -o \( -name '*.yaml' -o -name '*.yml' \) ! -name 'pnpm-lock.yaml' -print | xargs npx prettier --check --no-error-on-unmatched-pattern

# Check docs formatting (Markdown + YAML)
[group('docs')]
format-check-docs: format-check-md format-check-yaml

# Check internal links in Markdown
[group('docs')]
links-check:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{GREEN}}Checking local file links...{{RESET}}"
    if [ -n "{{LYCHEE}}" ]; then
        {{LYCHEE}} --scheme file --include-fragments --config .lychee.toml '**/*.md'
    else
        echo "{{YELLOW}}lychee not installed. Run: cargo install lychee{{RESET}}"
    fi

# Check external links (may take minutes)
[group('docs')]
links-check-external:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "{{GREEN}}Checking external links...{{RESET}}"
    if [ -n "{{LYCHEE}}" ]; then
        {{LYCHEE}} --scheme https --scheme http --config .lychee.toml '**/*.md'
    else
        echo "{{YELLOW}}lychee not installed. Run: cargo install lychee{{RESET}}"
    fi

# Check all links (internal + external)
[group('docs')]
links-check-all: links-check links-check-external

# ============================================================================
# Docker
# ============================================================================

# Start production stack (SQLite — the default backend)
[group('docker')]
docker-up:
    @echo "{{GREEN}}Starting Emailibrium stack (SQLite)...{{RESET}}"
    @{{COMPOSE}} up -d
    @echo "{{GREEN}}Backend: http://localhost:8080  Frontend: http://localhost:3000{{RESET}}"

# Start production stack against PostgreSQL
[group('docker')]
docker-up-postgres:
    @just _compose-up-postgres "{{COMPOSE}}" "production"

# Start dev stack (hot-reload) against PostgreSQL
[group('docker')]
docker-up-dev-postgres:
    @just _compose-up-postgres "{{COMPOSE_DEV}}" "dev"

# Shared body of the two PostgreSQL up recipes.
#
# TWO things have to line up for a docker deployment to be on PostgreSQL, and a
# recipe that supplied only the first was the trap this exists to close: --profile
# postgres starts the database container, but the connection URL is what actually
# selects the backend (ADR-033). Enabling the profile alone leaves the app on SQLite
# next to an idle database — a failure that looks like success.
#
# So this derives the matching URL from the same db_password secret the postgres
# service reads, making `just docker-up-postgres` genuinely one command. An explicit
# EMAILIBRIUM_DATABASE_URL always wins, so pointing at an external PostgreSQL still
# works — and in that case the local container is redundant but harmless.
[private]
_compose-up-postgres compose label:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${EMAILIBRIUM_DATABASE_URL:-}" ]]; then
      pw_file="secrets/${APP_ENV:-dev}/db_password"
      if [[ ! -f "$pw_file" ]]; then
        echo "{{RED}}Missing $pw_file — run 'just docker-secrets' first, or export EMAILIBRIUM_DATABASE_URL to use an external PostgreSQL.{{RESET}}" >&2
        exit 1
      fi
      export EMAILIBRIUM_DATABASE_URL="postgres://emailibrium:$(cat "$pw_file")@postgres:5432/emailibrium"
      echo "{{GREEN}}Using the compose PostgreSQL service (URL derived from $pw_file).{{RESET}}"
    else
      echo "{{GREEN}}Using the EMAILIBRIUM_DATABASE_URL already set in the environment.{{RESET}}"
    fi
    echo "{{GREEN}}Starting Emailibrium {{label}} stack (PostgreSQL)...{{RESET}}"
    {{compose}} --profile postgres up -d
    echo "{{GREEN}}Backend: http://localhost:8080  Frontend: http://localhost:3000{{RESET}}"

# Start dev stack (hot-reload, SQLite — the default backend)
[group('docker')]
docker-up-dev:
    @echo "{{GREEN}}Starting Emailibrium dev stack (SQLite)...{{RESET}}"
    @{{COMPOSE_DEV}} up -d
    @echo "{{GREEN}}Backend: http://localhost:8080  Frontend: http://localhost:3000{{RESET}}"


# --profile postgres is required for teardown, not optional tidiness: `docker compose
# down` computes the project from the ACTIVE profiles, so without it a container
# started by docker-up-postgres is left running while the command reports success.
# (`docker compose ps` does list it either way — the asymmetry is easy to miss.)
# Harmless when postgres was never started.
#
# Stop and remove containers (including the opt-in postgres one)
[group('docker')]
docker-down:
    @{{COMPOSE}} --profile postgres down

# Stop + remove volumes (DESTROYS DATA)
[group('docker')]
docker-down-volumes:
    @{{COMPOSE}} --profile postgres down -v

# Restart is NOT backend-preserving, and cannot be: `down` destroys the containers
# that knew which URL they were started with. Restarting a PostgreSQL deployment
# with this recipe would stop its database (docker-down now tears the profile down
# too) and bring the backend back up on SQLite — use docker-restart-postgres.
#
# Restart all containers (SQLite — the default backend)
[group('docker')]
docker-restart: docker-down docker-up

# Restart all containers against PostgreSQL
[group('docker')]
docker-restart-postgres: docker-down docker-up-postgres

# Build Docker images
[group('docker')]
docker-build:
    @{{COMPOSE}} build

# Build images without cache
[group('docker')]
docker-build-no-cache:
    @{{COMPOSE}} build --no-cache

# Tail logs from all containers
[group('docker')]
docker-logs:
    @{{COMPOSE}} logs -f

# Tail backend logs
[group('docker')]
docker-logs-backend:
    @{{COMPOSE}} logs -f backend

# Tail frontend logs
[group('docker')]
docker-logs-frontend:
    @{{COMPOSE}} logs -f frontend

# Show running containers
[group('docker')]
docker-ps:
    @{{COMPOSE}} ps

# Shell into backend container
[group('docker')]
docker-exec-backend:
    @{{COMPOSE}} exec backend sh

# Shell into frontend container
[group('docker')]
docker-exec-frontend:
    @{{COMPOSE}} exec frontend sh

# Health check all containers
[group('docker')]
docker-health:
    @{{COMPOSE}} ps --format "table {{{{.Name}}\t{{{{.Status}}\t{{{{.Ports}}"

# Prune dangling Docker artifacts
[group('docker')]
docker-clean:
    # `|| true` retained: pruning is best-effort cleanup scoped to this project's
    # label, and a no-op/absent-daemon prune must not fail the recipe.
    @docker system prune -f --filter "label=com.docker.compose.project=emailibrium" 2>/dev/null || true

# Generate development secrets
[group('docker')]
docker-secrets:
    @mkdir -p secrets/dev
    @openssl rand -base64 32 > secrets/dev/jwt_secret
    @openssl rand -base64 32 > secrets/dev/oauth_encryption_key
    # SQLite is the default backend, so that is what a freshly generated secret set
    # points at. Switch a docker deployment to PostgreSQL by replacing this one line
    # with `postgres://emailibrium:devpass@postgres:5432/emailibrium` (or exporting
    # EMAILIBRIUM_DATABASE_URL) and starting the stack via docker-up-postgres.
    @echo "sqlite:/app/data/emailibrium.db?mode=rwc" > secrets/dev/database_url
    @echo "devpass" > secrets/dev/db_password
    @chmod 600 secrets/dev/*
    @echo "{{GREEN}}Secrets generated in secrets/dev/{{RESET}}"

# ============================================================================
# Release
# ============================================================================

# Pre-release CI validation
[group('release')]
release-check: ci
    @echo "{{GREEN}}Release checks passed. Ready to tag.{{RESET}}"

# Tag a release (usage: just release-tag VERSION=0.1.0)
[group('release')]
release-tag version=VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    v="{{version}}"; v="${v#VERSION=}"
    if [ -z "$v" ]; then echo "{{YELLOW}}Usage: just release-tag VERSION=0.1.0{{RESET}}"; exit 1; fi
    git tag -a "v$v" -m "Release v$v"
    echo "{{GREEN}}Tagged v$v. Push with: git push origin v$v{{RESET}}"

# Push latest tag to trigger release workflow
[group('release')]
release-push:
    #!/usr/bin/env bash
    set -euo pipefail
    # `|| TAG=""` is not error masking: "no tags yet" is the expected first-run
    # state, and the -z check below turns it into an explicit exit 1.
    TAG=$(git describe --tags --abbrev=0 2>/dev/null) || TAG=""
    if [ -z "$TAG" ]; then echo "{{YELLOW}}No tags found.{{RESET}}"; exit 1; fi
    echo "{{GREEN}}Pushing $TAG to origin...{{RESET}}"
    git push origin "$TAG"

# Cut a release (bumps versions, updates CHANGELOG, commits, tags, pushes). Usage: just release VERSION=0.1.0
[group('release')]
release version=VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    v="{{version}}"; v="${v#VERSION=}"
    [ -n "$v" ] || { echo "usage: just release VERSION=X.Y.Z" >&2; exit 1; }
    ./scripts/release.sh "$v"

# Regenerate CHANGELOG.md from git history using git-cliff
[group('release')]
changelog:
    git-cliff --output CHANGELOG.md
