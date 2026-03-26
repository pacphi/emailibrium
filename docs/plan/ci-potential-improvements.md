# CI/CD Potential Improvements for Built-in LLM

Date: 2026-03-26 | Status: Proposed | Context: ADR-021, BL-1 through BL-4, UX alignment

---

## Current State

All existing CI pipelines pass with zero impact from the built-in LLM implementation:

- **GitHub Actions** (ci.yml: 9 jobs, release.yml: 5 jobs, docker.yml: 2 jobs) — all green
- **Makefile** — `ci`, `test`, `build`, `typecheck`, `format-check` all pass
- **Docker** — backend and frontend images build correctly; `node-llama-cpp` excluded from production bundle
- **Pre-commit** — clippy, fmt, lint-staged all pass

No changes are required for CI to continue working. The improvements below are optional enhancements.

---

## Improvement 1: Fast AI Test Target

**Priority:** High | **Effort:** 5 min

Add a dedicated Makefile target for running only the AI service tests (~124 tests in ~300ms), useful during development iteration.

```makefile
# Root Makefile
.PHONY: test-ai
test-ai: ## Run AI service tests only (fast)
	@cd $(FRONTEND_DIR)/apps/web && npx vitest run src/services/ai/__tests__/
```

**Why:** Full test suite includes email, routing, and other tests. During LLM development, `make test-ai` gives faster feedback.

---

## Improvement 2: Model Manifest Validation in CI

**Priority:** High | **Effort:** 10 min

Add a CI step that validates the model manifest entries (model IDs, HuggingFace repo names, file sizes) are consistent and the manifest module compiles correctly.

```yaml
# In .github/workflows/ci.yml, under frontend-quality job
- name: Validate AI model manifest
  run: |
    cd frontend/apps/web
    npx vitest run src/services/ai/__tests__/model-management.test.ts
```

**Why:** Manifest changes (new models, updated repos, corrected sizes) should be caught in CI before merge. The existing `model-management.test.ts` already validates manifest structure.

---

## Improvement 3: Native Dependency Security Audit

**Priority:** Medium | **Effort:** 10 min

`node-llama-cpp` includes native C++ bindings compiled from llama.cpp. Native dependencies carry higher CVE risk than pure JavaScript packages. Add explicit auditing.

```yaml
# In .github/workflows/ci.yml, under frontend-quality job
- name: Security audit (including native deps)
  run: cd frontend && pnpm audit --audit-level moderate
  continue-on-error: true  # Advisory — don't block CI for moderate findings
```

**Why:** `pnpm audit` catches known vulnerabilities in the dependency tree. `continue-on-error: true` ensures it's informational, not blocking (native packages often have unresolved advisories).

---

## Improvement 4: GGUF Model Cache for Nightly Integration Tests

**Priority:** Low | **Effort:** 30 min

For weekly or nightly CI runs, cache the default GGUF model to enable real inference smoke tests. This should NOT run on every PR — only on schedule.

```yaml
# New file: .github/workflows/nightly-llm.yml
name: Nightly LLM Smoke Test
on:
  schedule:
    - cron: '0 4 * * 1'  # Monday 4 AM UTC
  workflow_dispatch: {}

jobs:
  llm-smoke:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Cache GGUF model
        uses: actions/cache@v4
        with:
          path: ~/.emailibrium/models/llm
          key: gguf-qwen2.5-0.5b-q4km-v1

      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: pnpm
          cache-dependency-path: frontend/pnpm-lock.yaml

      - name: Install frontend
        run: cd frontend && pnpm install --frozen-lockfile

      - name: Download default model (if not cached)
        run: cd frontend/apps/web && npx tsx ../../../scripts/models.ts download --default

      - name: Run AI integration tests
        run: cd frontend/apps/web && npx vitest run src/services/ai/__tests__/

      # Future: Run benchmark script when Rust backend has llama-cpp-2
      # - name: Run LLM benchmarks
      #   run: cd frontend/apps/web && npx tsx src/services/ai/__tests__/built-in-llm-bench.ts
```

**Why:** Real model inference tests catch issues that mocked tests cannot (grammar failures, model format changes, native binding crashes). Weekly cadence avoids bandwidth waste on every PR.

---

## Improvement 5: CI Summary with AI Provider Status

**Priority:** Low | **Effort:** 5 min

Add a summary line at the end of the CI pipeline showing which AI providers are configured. Helps maintainers quickly verify defaults haven't drifted.

```makefile
# Append to existing `ci` target in root Makefile
ci: format-check lint typecheck test
	@echo ""
	@echo "$(BOLD)AI Configuration:$(RESET)"
	@grep -A1 'generative:' $(BACKEND_DIR)/config.yaml | head -2
	@echo "$(GREEN)CI passed.$(RESET)"
```

**Why:** Catches accidental default changes (e.g., someone changing `provider: "builtin"` back to `"none"`) during code review.

---

## Improvement 6: Makefile `test-ai` in Backend

**Priority:** Medium | **Effort:** 5 min

When `llama-cpp-2` is added to the Rust backend (RBL-1), add a feature-gated test target:

```makefile
# backend/Makefile
.PHONY: test-llm
test-llm: ## Run built-in LLM tests (requires builtin-llm feature)
	cargo test --features builtin-llm -- builtin_llm
```

**Why:** `llama-cpp-2` tests require the feature flag enabled and potentially a cached model. Keeping them in a separate target prevents accidental CI failures.

---

## Improvement 7: Docker Multi-Stage with Optional LLM

**Priority:** Low | **Effort:** 1 hour

When the Rust backend gains `llama-cpp-2` (RBL-1), the Docker build will need a feature-gated build stage:

```dockerfile
# backend/Dockerfile — future enhancement
ARG ENABLE_BUILTIN_LLM=false

# Build stage with optional LLM
FROM rust:1.94 AS builder
ARG ENABLE_BUILTIN_LLM
RUN if [ "$ENABLE_BUILTIN_LLM" = "true" ]; then \
      apt-get update && apt-get install -y cmake; \
    fi
RUN if [ "$ENABLE_BUILTIN_LLM" = "true" ]; then \
      cargo build --release --features builtin-llm; \
    else \
      cargo build --release; \
    fi
```

```bash
# Build with LLM support
docker build --build-arg ENABLE_BUILTIN_LLM=true -t emailibrium-backend .

# Build without (default, smaller image)
docker build -t emailibrium-backend .
```

**Why:** The `llama-cpp-2` build requires CMake and adds ~90s + ~8 MB to the binary. Feature-gating the Docker build keeps the default image lean while allowing opt-in LLM support.

---

## Implementation Priority

| # | Improvement | Priority | Effort | When |
|---|------------|----------|--------|------|
| 1 | `make test-ai` target | High | 5 min | Now |
| 2 | Manifest validation in CI | High | 10 min | Now |
| 3 | Native dep security audit | Medium | 10 min | Before release |
| 4 | Nightly LLM smoke test | Low | 30 min | After RBL-1 |
| 5 | CI summary with AI status | Low | 5 min | Now |
| 6 | Backend `test-llm` target | Medium | 5 min | During RBL-1 |
| 7 | Docker multi-stage LLM | Low | 1 hour | During RBL-2 |
