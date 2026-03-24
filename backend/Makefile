# ============================================================================
# Emailibrium Backend — Rust (Cargo)
# ============================================================================
#
# Quick Start:
#   make build     - Build the backend
#   make test      - Run all tests
#   make dev       - Start dev server
#   make bench     - Run benchmarks
# ============================================================================

SHELL := /bin/bash
CARGO := cargo

BOLD   := $(shell tput bold 2>/dev/null || echo '')
GREEN  := $(shell tput setaf 2 2>/dev/null || echo '')
YELLOW := $(shell tput setaf 3 2>/dev/null || echo '')
BLUE   := $(shell tput setaf 4 2>/dev/null || echo '')
RESET  := $(shell tput sgr0 2>/dev/null || echo '')

.DEFAULT_GOAL := help

# ============================================================================
# Default Target
# ============================================================================

.PHONY: help
help:
	@echo "$(BOLD)$(BLUE)═══ Emailibrium Backend (Rust) ═════════════════════════════════════$(RESET)"
	@echo ""
	@echo "$(BOLD)Build & Run$(RESET)"
	@echo "  build              - Build backend (debug)"
	@echo "  build-release      - Build backend (release, optimized)"
	@echo "  dev                - Start dev server"
	@echo "  clean              - Clean build artifacts"
	@echo ""
	@echo "$(BOLD)Test$(RESET)"
	@echo "  test               - Run all tests"
	@echo "  test-lib           - Run library tests only"
	@echo "  test-integration   - Run integration tests only"
	@echo "  bench              - Run benchmarks"
	@echo ""
	@echo "$(BOLD)Lint & Format$(RESET)"
	@echo "  lint               - Run clippy"
	@echo "  format             - Format code (cargo fmt)"
	@echo "  format-check       - Check formatting"
	@echo "  deadcode           - Check for dead code"
	@echo ""
	@echo "$(BOLD)Security$(RESET)"
	@echo "  audit              - Security audit (cargo-audit)"
	@echo ""
	@echo "$(BOLD)Dependencies$(RESET)"
	@echo "  upgrade            - Upgrade deps (within semver)"
	@echo "  outdated           - Show outdated crates"
	@echo ""
	@echo "$(BOLD)Documentation$(RESET)"
	@echo "  lint-docs          - Lint Markdown + YAML in backend/"
	@echo "  format-docs        - Format Markdown + YAML in backend/"

# ============================================================================
# Build & Run
# ============================================================================

.PHONY: build
build: ## Build backend (debug)
	@$(CARGO) build

.PHONY: build-release
build-release: ## Build backend (release, optimized)
	@$(CARGO) build --release

.PHONY: dev
dev: ## Start dev server
	@$(CARGO) run

.PHONY: clean
clean: ## Clean build artifacts
	@$(CARGO) clean

# ============================================================================
# Test
# ============================================================================

.PHONY: test
test: ## Run all tests
	@$(CARGO) test

.PHONY: test-lib
test-lib: ## Run library tests only
	@$(CARGO) test --lib

.PHONY: test-integration
test-integration: ## Run integration tests only
	@$(CARGO) test --test '*'

.PHONY: bench
bench: ## Run benchmarks
	@$(CARGO) bench

# ============================================================================
# Lint & Format
# ============================================================================

.PHONY: lint
lint: ## Run clippy
	@$(CARGO) clippy -- -D warnings 2>&1 | grep -v "dead_code\|never used\|never constructed\|never read" || true

.PHONY: format
format: ## Format code
	@$(CARGO) fmt

.PHONY: format-check
format-check: ## Check formatting
	@$(CARGO) fmt -- --check

.PHONY: deadcode
deadcode: ## Check for dead code
	@$(CARGO) clippy -- -W dead_code 2>&1 | grep "warning.*never" | sort | uniq -c | sort -rn

# ============================================================================
# Security
# ============================================================================

.PHONY: audit
audit: ## Security audit dependencies
	@$(CARGO) audit 2>/dev/null || echo "$(YELLOW)cargo-audit not installed. Run: cargo install cargo-audit$(RESET)"

# ============================================================================
# Dependency Management
# ============================================================================

.PHONY: upgrade
upgrade: ## Upgrade dependencies (within semver)
	@$(CARGO) update
	@echo "$(GREEN)Cargo.lock updated.$(RESET)"

.PHONY: outdated
outdated: ## Show outdated crates
	@$(CARGO) outdated --root-deps-only 2>/dev/null || echo "$(YELLOW)Install with: cargo install cargo-outdated$(RESET)"

# ============================================================================
# Documentation
# ============================================================================

.PHONY: lint-docs
lint-docs: ## Lint Markdown + YAML in backend/
	@markdownlint-cli2 '**/*.md' '#target' 2>/dev/null || true
	@yamllint -c ../.yamllint.yaml . 2>/dev/null || true

.PHONY: format-docs
format-docs: ## Format Markdown + YAML in backend/
	@npx prettier --write '**/*.md' '**/*.{yaml,yml}' --config ../.prettierrc 2>/dev/null || true

.PHONY: format-check-docs
format-check-docs: ## Check Markdown + YAML formatting
	@npx prettier --check '**/*.md' '**/*.{yaml,yml}' --config ../.prettierrc 2>/dev/null || true
