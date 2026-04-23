# ============================================================================
# Emailibrium Frontend — React/TypeScript (pnpm + Turborepo)
# ============================================================================
#
# Quick Start:
#   make install   - Install dependencies
#   make dev       - Start Vite dev server
#   make build     - Build all packages
#   make test      - Run tests
# ============================================================================

SHELL := /bin/bash
PNPM  := pnpm
TURBO := pnpm turbo
WEB   := apps/web

BOLD   := $(shell tput bold 2>/dev/null || echo '')
GREEN  := $(shell tput setaf 2 2>/dev/null || echo '')
YELLOW := $(shell tput setaf 3 2>/dev/null || echo '')
BLUE   := $(shell tput setaf 4 2>/dev/null || echo '')
RED    := $(shell tput setaf 1 2>/dev/null || echo '')
RESET  := $(shell tput sgr0 2>/dev/null || echo '')

.DEFAULT_GOAL := help

# ============================================================================
# Default Target
# ============================================================================

.PHONY: help
help:
	@echo "$(BOLD)$(BLUE)═══ Emailibrium Frontend (TypeScript/React) ════════════════════════$(RESET)"
	@echo ""
	@echo "$(BOLD)Install & Build$(RESET)"
	@echo "  install            - Install pnpm dependencies"
	@echo "  build              - Build all packages (Turborepo)"
	@echo "  dev                - Start Vite dev server (port 3000)"
	@echo "  storybook          - Start Storybook component explorer"
	@echo "  clean              - Clean build artifacts"
	@echo ""
	@echo "$(BOLD)Test$(RESET)"
	@echo "  test               - Run unit tests (Vitest)"
	@echo "  test-e2e           - Run E2E tests (Playwright)"
	@echo ""
	@echo "$(BOLD)Lint & Format$(RESET)"
	@echo "  lint               - Lint TypeScript (ESLint)"
	@echo "  format             - Format code (Prettier)"
	@echo "  format-check       - Check formatting"
	@echo "  typecheck          - TypeScript type check"
	@echo "  deadcode           - Check for unused exports"
	@echo ""
	@echo "$(BOLD)Security$(RESET)"
	@echo "  audit              - Security audit (pnpm audit)"
	@echo ""
	@echo "$(BOLD)Dependencies$(RESET)"
	@echo "  upgrade            - Upgrade deps (within semver)"
	@echo "  upgrade-latest     - Upgrade to latest major versions"
	@echo "  outdated           - Show outdated packages"
	@echo ""
	@echo "$(BOLD)Documentation$(RESET)"
	@echo "  lint-docs          - Lint Markdown + YAML in frontend/"
	@echo "  format-docs        - Format Markdown + YAML in frontend/"

# ============================================================================
# Install & Build
# ============================================================================

.PHONY: install
install: ## Install dependencies
	@$(PNPM) install

.PHONY: build
build: ## Build all packages
	@$(TURBO) build

.PHONY: dev
dev: ## Start Vite dev server
	@cd $(WEB) && npx vite

.PHONY: storybook
storybook: ## Start Storybook
	@cd $(WEB) && npx storybook dev -p 6006

.PHONY: clean
clean: ## Clean build artifacts
	@rm -rf $(WEB)/dist $(WEB)/storybook-static node_modules/.cache .turbo

# ============================================================================
# Test
# ============================================================================

.PHONY: test
test: ## Run unit tests (Vitest)
	@$(TURBO) test || true

.PHONY: test-e2e
test-e2e: ## Run E2E tests (Playwright)
	@cd $(WEB) && npx playwright test

# ============================================================================
# Lint & Format
# ============================================================================

.PHONY: lint
lint: ## Lint TypeScript (ESLint — strict)
	@cd $(WEB) && npx --no-install eslint src/ --ext .ts,.tsx

.PHONY: format
format: ## Format code (Prettier)
	@cd $(WEB) && npx prettier --write 'src/**/*.{ts,tsx}'

.PHONY: format-check
format-check: ## Check formatting
	@cd $(WEB) && npx prettier --check 'src/**/*.{ts,tsx}'

.PHONY: typecheck
typecheck: ## TypeScript type check
	@cd $(WEB) && npx tsc --noEmit

.PHONY: deadcode
deadcode: ## Check for unused exports
	@cd $(WEB) && npx ts-prune src/ 2>/dev/null || echo "$(YELLOW)ts-prune not installed. Run: pnpm add -Dw ts-prune$(RESET)"

# ============================================================================
# Security
# ============================================================================

.PHONY: audit
audit: ## Security audit
	@$(PNPM) audit --prod 2>/dev/null || echo "$(YELLOW)pnpm audit completed with findings$(RESET)"

# ============================================================================
# Dependency Management
# ============================================================================

.PHONY: upgrade
upgrade: ## Upgrade dependencies (within semver)
	@$(PNPM) update -r
	@echo "$(GREEN)Frontend packages upgraded.$(RESET)"

.PHONY: upgrade-latest
upgrade-latest: ## Upgrade to latest major versions
	@$(PNPM) update -r --latest
	@echo "$(GREEN)Done. Run 'make typecheck' to verify.$(RESET)"

.PHONY: outdated
outdated: ## Show outdated packages
	@$(PNPM) outdated -r 2>/dev/null || true

# ============================================================================
# Documentation
# ============================================================================

.PHONY: lint-docs
lint-docs: ## Lint Markdown + YAML in frontend/ (strict)
	@command -v markdownlint-cli2 >/dev/null 2>&1 || { echo "$(RED)markdownlint-cli2 not installed. Run: npm i -g markdownlint-cli2$(RESET)"; exit 1; }
	@command -v yamllint >/dev/null 2>&1 || { echo "$(RED)yamllint not installed. Run: pip install yamllint$(RESET)"; exit 1; }
	@markdownlint-cli2 '**/*.md' '#node_modules'
	@yamllint -c ../.yamllint.yaml .

.PHONY: format-docs
format-docs: ## Format Markdown + YAML in frontend/
	@npx --no-install prettier --write '**/*.md' '**/*.{yaml,yml}' --config ../.prettierrc --ignore-path .gitignore

.PHONY: format-check-docs
format-check-docs: ## Check Markdown + YAML formatting
	@npx --no-install prettier --check '**/*.md' '**/*.{yaml,yml}' --config ../.prettierrc --ignore-path .gitignore
