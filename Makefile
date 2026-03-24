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
	@echo "  make install           - Install all dependencies"
	@echo "  make dev               - Start backend + frontend (native)"
	@echo "  make docker-up-dev     - Start full stack (Docker)"
	@echo "  make ci                - Run full CI pipeline"
	@echo "  make test              - Run all tests"
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
	@echo "  release-tag VER=x.y.z  - Create annotated tag"
	@echo "  release VER=x.y.z      - Full release (check + tag + push)"
	@echo "  changelog VER=x.y.z    - Preview changelog"
	@echo ""
	@echo "  Run '$(BOLD)make -C backend$(RESET)' or '$(BOLD)make -C frontend$(RESET)' for layer-specific targets."

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
dev: ## Start full stack dev servers (native)
	@$(MAKE) -C $(BACKEND_DIR) dev &
	@$(MAKE) -C $(FRONTEND_DIR) dev &
	@echo "$(GREEN)Backend: http://localhost:8080  Frontend: http://localhost:3000$(RESET)"

.PHONY: clean
clean: ## Clean all build artifacts
	@$(MAKE) -C $(BACKEND_DIR) clean
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
	@markdownlint-cli2 '**/*.md' '#**/node_modules' '#**/target' 2>/dev/null || echo "$(YELLOW)markdownlint-cli2 not installed. Run: npm i -g markdownlint-cli2$(RESET)"

.PHONY: lint-yaml
lint-yaml: ## Lint YAML files
	@echo "$(GREEN)Linting YAML...$(RESET)"
	@yamllint -c .yamllint.yaml . 2>/dev/null || echo "$(YELLOW)yamllint not installed. Run: pip install yamllint$(RESET)"

.PHONY: lint-docs
lint-docs: lint-md lint-yaml ## Lint all docs (Markdown + YAML)

.PHONY: format-md
format-md: ## Format Markdown files
	@npx prettier --write '**/*.md' --ignore-path .gitignore 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --write '../**/*.md' 2>/dev/null || true

.PHONY: format-yaml
format-yaml: ## Format YAML files
	@npx prettier --write '**/*.{yaml,yml}' --ignore-path .gitignore 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --write '../**/*.{yaml,yml}' 2>/dev/null || true

.PHONY: format-docs
format-docs: format-md format-yaml

.PHONY: format-check-md
format-check-md:
	@npx prettier --check '**/*.md' --ignore-path .gitignore 2>/dev/null || \
		cd $(FRONTEND_DIR) && npx prettier --check '../**/*.md' 2>/dev/null || true

.PHONY: format-check-yaml
format-check-yaml:
	@npx prettier --check '**/*.{yaml,yml}' --ignore-path .gitignore 2>/dev/null || \
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
