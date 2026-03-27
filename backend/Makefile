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
	@echo "$(BOLD)Models$(RESET)"
	@echo "  models             - Show available LLM models"
	@echo "  embedding-models   - Show available embedding models"
	@echo "  download-model     - Download a model (MODEL=<id>)"
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
	@$(CARGO) build -p emailibrium

.PHONY: build-release
build-release: ## Build backend (release, optimized)
	@$(CARGO) build --release -p emailibrium

.PHONY: dev
dev: ## Start dev server
	@RATE_LIMIT_PRESET=development $(CARGO) run

.PHONY: check-llm-deps
check-llm-deps: ## Check that cmake is installed (required for llama.cpp compilation)
	@command -v cmake >/dev/null 2>&1 || { \
		echo "$(YELLOW)ERROR: cmake is required to build the built-in LLM.$(RESET)"; \
		echo "  Install with: brew install cmake"; \
		exit 1; \
	}
	@echo "$(GREEN)cmake found: $$(cmake --version | head -1)$(RESET)"

.PHONY: dev-llm
dev-llm: check-llm-deps ## Start dev server with built-in LLM (downloads model on first run)
	@$(CARGO) run --features builtin-llm

.PHONY: build-llm
build-llm: check-llm-deps ## Build backend with built-in LLM support
	@$(CARGO) build --features builtin-llm

.PHONY: clean
clean: ## Clean build artifacts
	@$(CARGO) clean

.PHONY: models
models: ## Show available LLM models (reads from config/models-llm.yaml)
	@echo "$(BOLD)$(BLUE)═══ Available LLM Models ════════════════════════════════════════$(RESET)"
	@echo ""
	@RAM=$$(sysctl -n hw.memsize 2>/dev/null || echo $$(grep MemTotal /proc/meminfo 2>/dev/null | awk '{print $$2 * 1024}') 2>/dev/null || echo 8589934592); \
	RAM_GB=$$(( $$RAM / 1073741824 )); \
	AVAIL_MB=$$(( $$RAM / 1048576 - 4096 )); \
	echo "  $(BOLD)System:$(RESET) $${RAM_GB}GB RAM ($$(( $$AVAIL_MB / 1024 ))GB available for models)"; \
	echo ""; \
	awk -v avail="$$AVAIL_MB" -v G="$$(tput setaf 2 2>/dev/null)" -v Y="$$(tput setaf 3 2>/dev/null)" -v B="$$(tput bold 2>/dev/null)" -v R="$$(tput sgr0 2>/dev/null)" ' \
	/^  [a-z][a-z_]*:$$/ { \
	  prov=$$1; gsub(/:/, "", prov); \
	  label = (prov == "builtin") ? "builtin (onnx)" : prov; \
	  if (n++) printf "\n"; \
	  printf "  %s%s%s\n", B, label, R; \
	  printf "  ───────────────────────────────────────────────────────────────────────────\n"; \
	  printf "  %s%-30s %-10s %-8s %-8s %7s   %s%s\n", B, "MODEL ID", "PARAMS", "DISK", "RAM", "CTX", "QUALITY", R; \
	  printf "  ───────────────────────────────────────────────────────────────────────────\n"; \
	} \
	/^      - id:/ { id=$$3 } \
	/^        name:/ { $$1=""; name=$$0; gsub(/^ *"?|"? *$$/,"",name) } \
	/^        params:/ { params=$$2; gsub(/"/,"",params) } \
	/^        context_size:/ { ctx=$$2 } \
	/^        disk_mb:/ { disk=$$2 } \
	/^        min_ram_mb:/ { ram=$$2 } \
	/^        quality:/ { q=$$2; \
	  if (ram+0 <= avail+0 || ram+0 == 0) c=G; else c=Y; \
	  ds = (disk+0 >= 1000) ? sprintf("%.1fGB", disk/1000) : sprintf("%dMB", disk); \
	  rs = (ram+0 >= 1000) ? sprintf("%.1fGB", ram/1000) : sprintf("%dMB", ram); \
	  printf "  %s%-30s %-10s %-8s %-8s %7d   %s%s\n", c, id, params, ds, rs, ctx, q, R; \
	  id=""; name=""; params=""; ctx=0; disk=0; ram=0; q="" \
	}' ../config/models-llm.yaml 2>/dev/null || echo "  $(YELLOW)Cannot read config/models-llm.yaml$(RESET)"; \
	echo ""; \
	echo "  $(GREEN)Green$(RESET) = fits your hardware   $(YELLOW)Yellow$(RESET) = may not fit"; \
	echo ""; \
	echo "  $(BOLD)Usage:$(RESET)"; \
	echo "    make download-model MODEL=<id>   Pre-download a model"; \
	echo "    Edit config/models-llm.yaml      Add or modify models"; \
	echo ""

.PHONY: embedding-models
embedding-models: ## Show available embedding models (reads from config/models-embedding.yaml)
	@echo "$(BOLD)$(BLUE)═══ Available Embedding Models ══════════════════════════════════$(RESET)"
	@echo ""
	@awk -v G="$$(tput setaf 2 2>/dev/null)" -v B="$$(tput bold 2>/dev/null)" -v R="$$(tput sgr0 2>/dev/null)" ' \
	/^  [a-z][a-z_]*:$$/ { \
	  prov=$$1; gsub(/:/, "", prov); \
	  label = (prov == "onnx") ? "builtin (onnx)" : prov; \
	  if (n++) printf "\n"; \
	  printf "  %s%s%s\n", B, label, R; \
	  printf "  ───────────────────────────────────────────────────────────────────────────\n"; \
	  printf "  %s%-30s %-6s %-10s %-8s %-8s %s%s\n", B, "MODEL ID", "DIMS", "MAX TOKENS", "DISK", "RAM", "QUALITY", R; \
	  printf "  ───────────────────────────────────────────────────────────────────────────\n"; \
	} \
	/^      - id:/ { id=$$3 } \
	/^        dimensions:/ { dims=$$2 } \
	/^        max_tokens:/ { maxtok=$$2 } \
	/^        disk_mb:/ { disk=$$2 } \
	/^        min_ram_mb:/ { ram=$$2 } \
	/^        quality:/ { q=$$2; \
	  ds = (disk+0 >= 1000) ? sprintf("%.1fGB", disk/1000) : sprintf("%dMB", disk); \
	  rs = (ram+0 >= 1000) ? sprintf("%.1fGB", ram/1000) : sprintf("%dMB", ram); \
	  printf "  %s%-30s %-6d %-10d %-8s %-8s %s%s\n", G, id, dims, maxtok, ds, rs, q, R; \
	  id=""; dims=0; maxtok=0; disk=0; ram=0; q="" \
	}' ../config/models-embedding.yaml 2>/dev/null || echo "  $(YELLOW)Cannot read config/models-embedding.yaml$(RESET)"; \
	echo ""; \
	echo "  $(BOLD)Usage:$(RESET)"; \
	echo "    Edit config/models-embedding.yaml   Add or modify embedding models"; \
	echo ""

.PHONY: download-model
download-model: ## Download a model by ID (e.g., make download-model MODEL=qwen3-8b-q4km)
	@if [ -z "$(MODEL)" ]; then \
		echo "$(BOLD)Usage:$(RESET) make download-model MODEL=<model-id>"; \
		echo ""; \
		echo "$(BOLD)Examples:$(RESET)"; \
		echo "  make download-model MODEL=qwen3-8b-q4km    # GGUF via Hugging Face"; \
		echo "  make download-model MODEL=qwen3:8b          # via Ollama"; \
		echo ""; \
		echo "Run '$(BOLD)make models$(RESET)' to see available models."; \
		exit 1; \
	fi
	@$(CARGO) run --features builtin-llm -- --download-model $(MODEL)

.PHONY: clean-data
clean-data: ## Remove all local data (DB, vectors, sidecars) — fresh start
	@echo "$(YELLOW)Removing local data...$(RESET)"
	@rm -f emailibrium.db emailibrium.db-wal emailibrium.db-shm
	@rm -f emailibrium-dev.db "emailibrium 2.db"
	@rm -f data/emailibrium.db
	@rm -rf data/vectors/*/vectors.db data/vectors/*/documents.json
	@echo "$(GREEN)Local data removed. Next 'make dev' will start fresh.$(RESET)"
	@echo "  $(YELLOW)Note: .fastembed_cache/ (ONNX models) preserved to avoid re-download.$(RESET)"
	@echo "  To also remove cached models: rm -rf .fastembed_cache/"

.PHONY: clean-all
clean-all: clean clean-data ## Clean build artifacts + all local data
	@echo "$(GREEN)Full clean complete.$(RESET)"

# ============================================================================
# Test
# ============================================================================

.PHONY: test
test: ## Run all tests (emailibrium only)
	@$(CARGO) test -p emailibrium

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
lint: ## Run clippy (emailibrium only, excludes ruvector submodule)
	@$(CARGO) clippy -p emailibrium -- -D warnings 2>&1 | grep -v "dead_code\|never used\|never constructed\|never read" || true

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
