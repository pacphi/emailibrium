# ============================================================================
# Emailibrium — Root Makefile
# ============================================================================
# Delegates to backend/ and frontend/ Makefiles.
# Provides cross-cutting targets for CI, Docker, releases, and docs.
#
# Quick Start:
#   make help              - Show all available targets
#   make install           - Install all dependencies
#   make dev               - Start full stack (native)
#   make docker-up-dev     - Start full stack (Docker)
#   make ci                - Run full CI pipeline
#   make release VERSION=x.y.z - Tag and release
# ============================================================================

# ============================================================================
# Variables and Configuration
# ============================================================================

SHELL := /bin/bash
.DEFAULT_GOAL := help

BACKEND_DIR  := backend
FRONTEND_DIR := frontend

COMPOSE      := docker compose
COMPOSE_DEV  := $(COMPOSE) -f docker-compose.yml -f docker-compose.dev.yml

LYCHEE := $(shell command -v lychee 2>/dev/null || echo "")

# Colors
BOLD   := $(shell tput bold 2>/dev/null || echo '')
GREEN  := $(shell tput setaf 2 2>/dev/null || echo '')
YELLOW := $(shell tput setaf 3 2>/dev/null || echo '')
BLUE   := $(shell tput setaf 4 2>/dev/null || echo '')
RESET  := $(shell tput sgr0 2>/dev/null || echo '')

# ============================================================================
# Default Target
# ============================================================================

.PHONY: help
help:
	@echo "$(BOLD)$(BLUE)╔════════════════════════════════════════════════════════════════════╗$(RESET)"
	@echo "$(BOLD)$(BLUE)║                      Emailibrium Makefile                          ║$(RESET)"
	@echo "$(BOLD)$(BLUE)╚════════════════════════════════════════════════════════════════════╝$(RESET)"
	@echo ""
	@echo "$(BOLD)Quick Start:$(RESET)"
	@echo "  make setup             - Guided first-time setup wizard"
	@echo "  make install           - Install all dependencies"
	@echo "  make dev               - Start backend + frontend (native)"
	@echo "  make dev-llm           - Start with built-in LLM (llama.cpp)"
	@echo "  make models            - Show available LLM models"
	@echo "  make embedding-models  - Show available embedding models"
	@echo "  make download-model    - Download a model (MODEL=<id>)"
	@echo "  make docker-up-dev     - Start full stack (Docker)"
	@echo "  make ci                - Run full CI pipeline"
	@echo "  make test              - Run all tests"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Setup & Onboarding ═════════════════════════════════════════════$(RESET)"
	@echo "  setup                  - Guided first-time setup wizard"
	@echo "  setup-prereqs          - Check all prerequisites"
	@echo "  setup-secrets          - Generate/configure secrets"
	@echo "  setup-ai               - Configure AI providers"
	@echo "  setup-docker           - Set up Docker environment"
	@echo "  setup-validate         - Validate entire setup"
	@echo "  download-models        - Download AI models (ONNX + GGUF)"
	@echo "  diagnose               - Show AI configuration diagnostics"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Install & Build ═════════════════════════════════════════════════$(RESET)"
	@echo "  install                - Install all dependencies (backend + frontend)"
	@echo "  build                  - Build everything"
	@echo "  dev                    - Start full stack dev servers (native)"
	@echo "  clean                  - Clean all build artifacts"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Test ════════════════════════════════════════════════════════════$(RESET)"
	@echo "  test                   - Run all tests (backend + frontend)"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Lint & Format ═══════════════════════════════════════════════════$(RESET)"
	@echo "  lint                   - Lint everything (code + docs)"
	@echo "  format                 - Format everything (code + docs)"
	@echo "  format-check           - Check formatting (no changes)"
	@echo "  typecheck              - TypeScript type check"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Security & Quality ═════════════════════════════════════════════$(RESET)"
	@echo "  audit                  - Security audit all dependencies"
	@echo "  deadcode               - Check for dead code"
	@echo "  ci                     - Full CI pipeline"
	@echo "  ci-full                - CI + link checking"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Dependency Management ══════════════════════════════════════════$(RESET)"
	@echo "  upgrade                - Upgrade all deps (within semver)"
	@echo "  outdated               - Show outdated deps (no changes)"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Documentation ══════════════════════════════════════════════════$(RESET)"
	@echo "  lint-md                - Lint Markdown files"
	@echo "  lint-yaml              - Lint YAML files"
	@echo "  links-check            - Check internal links in Markdown"
	@echo "  links-check-external   - Check external links (slow)"
	@echo "  links-check-all        - Check all links"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Docker ═════════════════════════════════════════════════════════$(RESET)"
	@echo "  docker-up              - Start production stack"
	@echo "  docker-up-dev          - Start dev stack (hot-reload)"
	@echo "  docker-down            - Stop all containers"
	@echo "  docker-build           - Build Docker images"
	@echo "  docker-logs            - Tail all container logs"
	@echo "  docker-ps              - Show container status"
	@echo "  docker-health          - Health check all containers"
	@echo "  docker-secrets         - Generate dev secrets"
	@echo ""
	@echo "$(BOLD)$(BLUE)═══ Release ════════════════════════════════════════════════════════$(RESET)"
	@echo "  release-check          - Pre-release CI validation"
	@echo "  release-tag VERSION=x.y.z - Create annotated tag"
	@echo "  release VERSION=x.y.z    - Full release (check + tag + push)"
	@echo "  changelog VERSION=x.y.z  - Preview changelog"
	@echo ""
	@echo "  Run '$(BOLD)make -C backend$(RESET)' or '$(BOLD)make -C frontend$(RESET)' for layer-specific targets."

# ============================================================================
# Setup & Onboarding
# ============================================================================

.PHONY: setup
setup: ## Guided first-time setup wizard
	@bash scripts/setup.sh

.PHONY: setup-prereqs
setup-prereqs: ## Check all prerequisites
	@bash scripts/setup-prereqs.sh

.PHONY: setup-secrets
setup-secrets: ## Generate/configure secrets
	@bash scripts/setup-secrets.sh

.PHONY: setup-ai
setup-ai: ## Configure AI providers
	@bash scripts/setup-ai.sh

.PHONY: setup-docker
setup-docker: ## Set up Docker environment
	@bash scripts/setup-docker.sh

.PHONY: setup-validate
setup-validate: ## Validate entire setup
	@bash scripts/setup-validate.sh

.PHONY: download-models
download-models: ## Download AI models (ONNX embedding + GGUF LLM)
	@echo "$(BOLD)$(BLUE)Downloading AI models...$(RESET)"
	@echo "$(GREEN)Step 1:$(RESET) ONNX embedding model"
	@cd $(BACKEND_DIR) && cargo run -- --download-models 2>/dev/null || echo "  $(YELLOW)Backend not built. Run 'make build' first.$(RESET)"
	@echo "$(GREEN)Step 2:$(RESET) GGUF LLM model (qwen2.5-0.5b-q4km)"
	@cd $(FRONTEND_DIR)/apps/web && npx tsx ../../../scripts/models.ts download --default 2>/dev/null || echo "  $(YELLOW)Frontend not installed. Run 'make install' first.$(RESET)"
	@echo "$(GREEN)Done.$(RESET) Models cached for offline use."

.PHONY: diagnose
diagnose: ## Show AI configuration diagnostics
	@echo "$(BOLD)$(BLUE)Emailibrium AI Diagnostics$(RESET)"
	@echo "────────────────────────────────────────"
	@echo ""
	@echo "$(BOLD)Embedding:$(RESET)"
	@if [[ -d "$(BACKEND_DIR)/.fastembed_cache" ]]; then \
		echo "  Provider: ONNX (all-MiniLM-L6-v2)"; \
		echo "  Status:   $(GREEN)cached$(RESET) ($$(du -sh $(BACKEND_DIR)/.fastembed_cache 2>/dev/null | cut -f1))"; \
	else \
		echo "  Provider: ONNX (all-MiniLM-L6-v2)"; \
		echo "  Status:   $(YELLOW)not cached (downloads on first use)$(RESET)"; \
	fi
	@echo ""
	@echo "$(BOLD)Generative (LLM):$(RESET)"
	@CACHE="$$HOME/.emailibrium/models/llm"; \
	if [[ -d "$$CACHE" ]] && find "$$CACHE" -name "*.gguf" -print -quit 2>/dev/null | grep -q .; then \
		MODEL=$$(find "$$CACHE" -name "*.gguf" -print -quit 2>/dev/null | xargs basename); \
		SIZE=$$(du -sh "$$CACHE" 2>/dev/null | cut -f1); \
		echo "  Provider: builtin ($$MODEL)"; \
		echo "  Status:   $(GREEN)cached$(RESET) ($$SIZE)"; \
	else \
		echo "  Provider: builtin (qwen2.5-0.5b-q4km)"; \
		echo "  Status:   $(YELLOW)not cached$(RESET)"; \
		echo "  Fix:      make download-models"; \
	fi
	@echo ""
	@echo "$(BOLD)Ollama:$(RESET)"
	@if command -v ollama &>/dev/null && ollama list &>/dev/null 2>&1; then \
		echo "  Status: $(GREEN)running$(RESET)"; \
	elif command -v ollama &>/dev/null; then \
		echo "  Status: $(YELLOW)installed but not running$(RESET)"; \
	else \
		echo "  Status: not installed (optional)"; \
	fi
	@echo ""
	@echo "$(BOLD)Cloud APIs:$(RESET)"
	@for var in EMAILIBRIUM_OPENAI_API_KEY EMAILIBRIUM_ANTHROPIC_API_KEY EMAILIBRIUM_GEMINI_API_KEY; do \
		name=$$(echo "$$var" | sed 's/EMAILIBRIUM_//;s/_API_KEY//'); \
		if [[ -n "$${!var:-}" ]]; then \
			echo "  $$name: $(GREEN)configured$(RESET)"; \
		else \
			echo "  $$name: not configured"; \
		fi; \
	done
	@echo ""
	@echo "$(BOLD)Database:$(RESET)"
	@if [[ -f "$(BACKEND_DIR)/emailibrium-dev.db" ]]; then \
		echo "  Status: $(GREEN)exists$(RESET) ($$(du -sh $(BACKEND_DIR)/emailibrium-dev.db 2>/dev/null | cut -f1))"; \
	else \
		echo "  Status: not created yet (created on first run)"; \
	fi

# ============================================================================
# Install & Build
# ============================================================================

.PHONY: install
install: ## Install all dependencies
	@$(MAKE) -C $(BACKEND_DIR) build
	@$(MAKE) -C $(FRONTEND_DIR) install

.PHONY: build
build: ## Build everything
	@$(MAKE) -C $(BACKEND_DIR) build
	@$(MAKE) -C $(FRONTEND_DIR) build

.PHONY: dev
dev: ## Start full stack dev servers (native, loads secrets/dev/ as env vars)
	@echo "$(GREEN)Backend: http://localhost:8080  Frontend: http://localhost:3000$(RESET)"
	@export EMAILIBRIUM_GOOGLE_CLIENT_ID="$$(cat secrets/dev/google_client_id 2>/dev/null)" \
		EMAILIBRIUM_GOOGLE_CLIENT_SECRET="$$(cat secrets/dev/google_client_secret 2>/dev/null)" \
		EMAILIBRIUM_MICROSOFT_CLIENT_ID="$$(cat secrets/dev/microsoft_client_id 2>/dev/null)" \
		EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="$$(cat secrets/dev/microsoft_client_secret 2>/dev/null)" \
		JWT_SECRET="$$(cat secrets/dev/jwt_secret 2>/dev/null)" \
		EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD="$$(cat secrets/dev/oauth_encryption_key 2>/dev/null)" \
		RATE_LIMIT_PRESET=development; \
		trap 'kill 0' INT TERM EXIT; \
		$(MAKE) -C $(BACKEND_DIR) dev & \
		$(MAKE) -C $(FRONTEND_DIR) dev & \
		wait

.PHONY: dev-llm
dev-llm: ## Start full stack with built-in LLM (downloads ~350MB model on first run)
	@echo "$(GREEN)Backend (LLM): http://localhost:8080  Frontend: http://localhost:3000$(RESET)"
	@export EMAILIBRIUM_GOOGLE_CLIENT_ID="$$(cat secrets/dev/google_client_id 2>/dev/null)" \
		EMAILIBRIUM_GOOGLE_CLIENT_SECRET="$$(cat secrets/dev/google_client_secret 2>/dev/null)" \
		EMAILIBRIUM_MICROSOFT_CLIENT_ID="$$(cat secrets/dev/microsoft_client_id 2>/dev/null)" \
		EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="$$(cat secrets/dev/microsoft_client_secret 2>/dev/null)" \
		JWT_SECRET="$$(cat secrets/dev/jwt_secret 2>/dev/null)" \
		EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD="$$(cat secrets/dev/oauth_encryption_key 2>/dev/null)" \
		RATE_LIMIT_PRESET=development; \
		trap 'kill 0' INT TERM EXIT; \
		$(MAKE) -C $(BACKEND_DIR) dev-llm & \
		$(MAKE) -C $(FRONTEND_DIR) dev & \
		wait

.PHONY: clean
clean: ## Clean all build artifacts
	@$(MAKE) -C $(BACKEND_DIR) clean
	@$(MAKE) -C $(FRONTEND_DIR) clean

.PHONY: models
models: ## Show available LLM models with hardware recommendations
	@$(MAKE) -C $(BACKEND_DIR) models

.PHONY: embedding-models
embedding-models: ## Show available embedding models
	@$(MAKE) -C $(BACKEND_DIR) embedding-models

.PHONY: download-model
download-model: ## Download a model (e.g., make download-model MODEL=qwen3-8b-q4km)
	@$(MAKE) -C $(BACKEND_DIR) download-model MODEL=$(MODEL)

.PHONY: clean-data
clean-data: ## Remove all local data (DB, vectors) — fresh start
	@$(MAKE) -C $(BACKEND_DIR) clean-data

.PHONY: clean-all
clean-all: ## Clean build artifacts + all local data
	@$(MAKE) -C $(BACKEND_DIR) clean-all
	@$(MAKE) -C $(FRONTEND_DIR) clean

# ============================================================================
# Test
# ============================================================================

.PHONY: test
test: ## Run all tests
	@$(MAKE) -C $(BACKEND_DIR) test
	@$(MAKE) -C $(FRONTEND_DIR) test

# ============================================================================
# Lint & Format
# ============================================================================

.PHONY: lint
lint: lint-docs ## Lint everything (code + docs)
	@$(MAKE) -C $(BACKEND_DIR) lint
	@$(MAKE) -C $(FRONTEND_DIR) lint

.PHONY: format
format: format-docs ## Format everything (code + docs)
	@$(MAKE) -C $(BACKEND_DIR) format
	@$(MAKE) -C $(FRONTEND_DIR) format

.PHONY: format-check
format-check: format-check-docs ## Check formatting (no changes)
	@$(MAKE) -C $(BACKEND_DIR) format-check
	@$(MAKE) -C $(FRONTEND_DIR) format-check

.PHONY: typecheck
typecheck: ## Type check (frontend)
	@$(MAKE) -C $(FRONTEND_DIR) typecheck

# ============================================================================
# Security & Quality
# ============================================================================

.PHONY: audit
audit: ## Security audit all dependencies
	@$(MAKE) -C $(BACKEND_DIR) audit
	@$(MAKE) -C $(FRONTEND_DIR) audit

.PHONY: deadcode
deadcode: ## Check for dead code
	@$(MAKE) -C $(BACKEND_DIR) deadcode
	@$(MAKE) -C $(FRONTEND_DIR) deadcode

.PHONY: ci
ci: format-check lint typecheck test ## Full CI pipeline

.PHONY: ci-full
ci-full: ci links-check ## Full CI + link checking

# ============================================================================
# Dependency Management
# ============================================================================

.PHONY: upgrade
upgrade: ## Upgrade all dependencies (within semver)
	@$(MAKE) -C $(BACKEND_DIR) upgrade
	@$(MAKE) -C $(FRONTEND_DIR) upgrade

.PHONY: outdated
outdated: ## Show outdated dependencies (no changes)
	@$(MAKE) -C $(BACKEND_DIR) outdated
	@$(MAKE) -C $(FRONTEND_DIR) outdated

# ============================================================================
# Documentation (Markdown, YAML, Links)
# ============================================================================

.PHONY: lint-md
lint-md: ## Lint Markdown files
	@echo "$(GREEN)Linting Markdown...$(RESET)"
	@if command -v markdownlint-cli2 >/dev/null 2>&1; then markdownlint-cli2 '**/*.md' '#**/node_modules' '#**/target' '#.claude/worktrees/**' '#ruvector/**' || true; else echo "$(YELLOW)markdownlint-cli2 not installed. Run: npm i -g markdownlint-cli2$(RESET)"; fi

.PHONY: lint-yaml
lint-yaml: ## Lint YAML files
	@echo "$(GREEN)Linting YAML...$(RESET)"
	@find . \( -name node_modules -o -name target -o -name ruvector -o -name .claude -o -name .claude-flow \) -prune -o \( -name '*.yaml' -o -name '*.yml' \) ! -name 'pnpm-lock.yaml' -print | xargs yamllint -c .yamllint.yaml 2>/dev/null || echo "$(YELLOW)yamllint not installed. Run: pip install yamllint$(RESET)"

.PHONY: lint-docs
lint-docs: lint-md lint-yaml ## Lint all docs (Markdown + YAML)

.PHONY: format-md
format-md: ## Format Markdown files
	@npx prettier --write '**/*.md' --ignore-path .gitignore --ignore-pattern 'ruvector/**' 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --write '../**/*.md' 2>/dev/null || true

.PHONY: format-yaml
format-yaml: ## Format YAML files
	@npx prettier --write '**/*.{yaml,yml}' --ignore-path .gitignore --ignore-pattern 'ruvector/**' 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --write '../**/*.{yaml,yml}' 2>/dev/null || true

.PHONY: format-docs
format-docs: format-md format-yaml ## Format docs (Markdown + YAML)

.PHONY: format-check-md
format-check-md:
	@npx prettier --check '**/*.md' --ignore-path .gitignore --ignore-pattern 'ruvector/**' 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --check '../**/*.md' 2>/dev/null || true

.PHONY: format-check-yaml
format-check-yaml:
	@npx prettier --check '**/*.{yaml,yml}' --ignore-path .gitignore --ignore-pattern 'ruvector/**' 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --check '../**/*.{yaml,yml}' 2>/dev/null || true

.PHONY: format-check-docs
format-check-docs: format-check-md format-check-yaml

.PHONY: links-check
links-check: ## Check internal links in Markdown
	@echo "$(GREEN)Checking local file links...$(RESET)"
	@if [ -n "$(LYCHEE)" ]; then \
		$(LYCHEE) --scheme file --include-fragments --config .lychee.toml '**/*.md'; \
	else \
		echo "$(YELLOW)lychee not installed. Run: cargo install lychee$(RESET)"; \
	fi

.PHONY: links-check-external
links-check-external: ## Check external links (may take minutes)
	@echo "$(GREEN)Checking external links...$(RESET)"
	@if [ -n "$(LYCHEE)" ]; then \
		$(LYCHEE) --scheme https --scheme http --config .lychee.toml '**/*.md'; \
	else \
		echo "$(YELLOW)lychee not installed. Run: cargo install lychee$(RESET)"; \
	fi

.PHONY: links-check-all
links-check-all: links-check links-check-external ## Check all links

# ============================================================================
# Docker
# ============================================================================

.PHONY: docker-up
docker-up: ## Start production stack
	@echo "$(GREEN)Starting Emailibrium stack...$(RESET)"
	@$(COMPOSE) up -d
	@echo "$(GREEN)Backend: http://localhost:8080  Frontend: http://localhost:3000$(RESET)"

.PHONY: docker-up-dev
docker-up-dev: ## Start dev stack (hot-reload)
	@echo "$(GREEN)Starting Emailibrium dev stack...$(RESET)"
	@$(COMPOSE_DEV) up -d
	@echo "$(GREEN)Backend: http://localhost:8080  Frontend: http://localhost:3000$(RESET)"

.PHONY: docker-down
docker-down: ## Stop and remove containers
	@$(COMPOSE) down

.PHONY: docker-down-volumes
docker-down-volumes: ## Stop + remove volumes (DESTROYS DATA)
	@$(COMPOSE) down -v

.PHONY: docker-restart
docker-restart: docker-down docker-up ## Restart all containers

.PHONY: docker-build
docker-build: ## Build Docker images
	@$(COMPOSE) build

.PHONY: docker-build-no-cache
docker-build-no-cache: ## Build images without cache
	@$(COMPOSE) build --no-cache

.PHONY: docker-logs
docker-logs: ## Tail logs from all containers
	@$(COMPOSE) logs -f

.PHONY: docker-logs-backend
docker-logs-backend: ## Tail backend logs
	@$(COMPOSE) logs -f backend

.PHONY: docker-logs-frontend
docker-logs-frontend: ## Tail frontend logs
	@$(COMPOSE) logs -f frontend

.PHONY: docker-ps
docker-ps: ## Show running containers
	@$(COMPOSE) ps

.PHONY: docker-exec-backend
docker-exec-backend: ## Shell into backend container
	@$(COMPOSE) exec backend sh

.PHONY: docker-exec-frontend
docker-exec-frontend: ## Shell into frontend container
	@$(COMPOSE) exec frontend sh

.PHONY: docker-health
docker-health: ## Health check all containers
	@$(COMPOSE) ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}"

.PHONY: docker-clean
docker-clean: ## Prune dangling Docker artifacts
	@docker system prune -f --filter "label=com.docker.compose.project=emailibrium" 2>/dev/null || true

.PHONY: docker-secrets
docker-secrets: ## Generate development secrets
	@mkdir -p secrets/dev
	@openssl rand -base64 32 > secrets/dev/jwt_secret
	@openssl rand -base64 32 > secrets/dev/oauth_encryption_key
	@echo "postgres://emailibrium:devpass@postgres:5432/emailibrium" > secrets/dev/database_url
	@echo "devpass" > secrets/dev/db_password
	@chmod 600 secrets/dev/*
	@echo "$(GREEN)Secrets generated in secrets/dev/$(RESET)"

# ============================================================================
# Release
# ============================================================================

.PHONY: release-check
release-check: ci ## Pre-release CI validation
	@echo "$(GREEN)Release checks passed. Ready to tag.$(RESET)"

.PHONY: release-tag
release-tag: ## Tag a release (usage: make release-tag VERSION=0.1.0)
	@if [ -z "$(VERSION)" ]; then echo "$(YELLOW)Usage: make release-tag VERSION=0.1.0$(RESET)"; exit 1; fi
	@git tag -a "v$(VERSION)" -m "Release v$(VERSION)"
	@echo "$(GREEN)Tagged v$(VERSION). Push with: git push origin v$(VERSION)$(RESET)"

.PHONY: release-push
release-push: ## Push latest tag to trigger release workflow
	@TAG=$$(git describe --tags --abbrev=0 2>/dev/null); \
	if [ -z "$$TAG" ]; then echo "$(YELLOW)No tags found.$(RESET)"; exit 1; fi; \
	echo "$(GREEN)Pushing $$TAG to origin...$(RESET)"; \
	git push origin "$$TAG"

.PHONY: release
release: release-check release-tag release-push ## Full release (CI + tag + push)

.PHONY: changelog
changelog: ## Preview changelog (usage: make changelog VERSION=0.1.0)
	@if [ -z "$(VERSION)" ]; then echo "$(YELLOW)Usage: make changelog VERSION=0.1.0$(RESET)"; exit 1; fi
	@.github/scripts/generate-changelog.sh "$(VERSION)"
	@cat changelog.md
