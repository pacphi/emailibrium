# Rust Backend Built-in LLM Implementation Plan

## Emailibrium: Tier 0.5 — llama-cpp-2 in the Rust Backend

Version 1.0 | Date: 2026-03-26 | Status: Sprint-Ready

---

## 1. Overview

This plan implements the ADR-021 addendum (Rust backend LLM). It adds `llama-cpp-2` as a new `GenerativeModel` provider in the existing Rust backend, enabling built-in local LLM inference accessible via the REST API with no changes to the frontend's API calls.

### Cross-references

- ADR-021 Addendum: Rust Backend Built-in LLM via llama-cpp-2
- ADR-012: Tiered AI Provider Architecture
- DDD-006: AI Providers Domain
- Existing code: `backend/src/vectors/generative.rs` (GenerativeModel trait)
- Existing code: `backend/src/vectors/generative_router.rs` (provider failover)
- Existing code: `backend/src/vectors/model_registry.rs` (ProviderType enum)
- Existing code: `backend/src/vectors/config.rs` (GenerativeConfig)

### 1.1 Prerequisites

- Sprint BL-1 through BL-4 complete (frontend implementation serves as reference)
- Rust 1.95+, CMake 3.14+, C/C++ compiler (Xcode CLT on macOS)

---

## 2. Sprint Plan

### Sprint RBL-1: Core Integration (1 week)

**Goal:** Add llama-cpp-2 dependency, implement BuiltInGenerativeModel, register with GenerativeRouter.

#### Tasks

- [ ] **RBL-1.01**: Add dependencies to Cargo.toml
  - Add `llama-cpp-2` and `hf-hub` as optional dependencies behind `builtin-llm` feature
  - Add `builtin-llm-metal` and `builtin-llm-cuda` compound features
  - Verify `cargo build --features builtin-llm` succeeds on macOS
  - Verify `cargo build` (without feature) still works unchanged
  - **File:** `backend/Cargo.toml`

- [ ] **RBL-1.02**: Add `BuiltIn` to ProviderType enum
  - Add `BuiltIn` variant to `ProviderType` in `model_registry.rs`
  - Update `Display` impl to format as `"builtin"`
  - **File:** `backend/src/vectors/model_registry.rs`

- [ ] **RBL-1.03**: Add `BuiltInLlmConfig` to configuration
  - Add `BuiltInLlmConfig` struct: `model_id`, `context_size`, `gpu_layers`, `idle_timeout_secs`, `cache_dir`
  - Add `builtin` field to `GenerativeConfig`
  - Add `"builtin"` as valid value for `generative.provider`
  - Add defaults: model `"qwen2.5-0.5b-q4km"`, context 2048, gpu_layers 99, idle 300s
  - Add config test
  - **File:** `backend/src/vectors/config.rs`

- [ ] **RBL-1.04**: Implement GGUF model manifest and download
  - Create `backend/src/vectors/gguf_download.rs`
  - Define `GgufModelManifest` struct with model metadata (same 5 models as frontend manifest)
  - Use `hf-hub` crate for HuggingFace downloads with caching
  - `ensure_model_downloaded(manifest) -> Result<PathBuf>` — downloads if not cached
  - Progress logging via `tracing::info!`
  - Gate behind `#[cfg(feature = "builtin-llm")]`
  - **File:** `backend/src/vectors/gguf_download.rs`

- [ ] **RBL-1.05**: Implement `BuiltInGenerativeModel`
  - Create `backend/src/vectors/builtin_llm.rs`
  - `BuiltInGenerativeModel` struct wrapping `Arc<LlamaModel>` + `LlamaBackend`
  - Implement `GenerativeModel` trait:
    - `generate()` — create context per-request in `spawn_blocking`, tokenize, decode, sample
    - `classify()` — build classification prompt, apply GBNF grammar, parse JSON response
    - `model_name()` — return model identifier
    - `is_available()` — return `true` when model is loaded
  - Lazy loading: model loads on first `generate()`/`classify()` call
  - GBNF grammar for classification with dynamic category injection
  - Gate behind `#[cfg(feature = "builtin-llm")]`
  - **File:** `backend/src/vectors/builtin_llm.rs`

- [ ] **RBL-1.06**: Register with GenerativeRouter
  - In `VectorService::new()`, when `config.generative.provider == "builtin"`:
    - Download model via `gguf_download::ensure_model_downloaded()`
    - Create `BuiltInGenerativeModel`
    - Register with `GenerativeRouter` at priority 5 (between RuleBased:10 and Ollama:3)
  - Update config.yaml comment to document `"builtin"` as valid provider
  - Gate behind `#[cfg(feature = "builtin-llm")]`
  - **Files:** `backend/src/vectors/mod.rs`, `backend/config.yaml`

- [ ] **RBL-1.07**: Write tests
  - Unit tests for `BuiltInLlmConfig` defaults and deserialization
  - Unit test for GBNF grammar generation with dynamic categories
  - Integration test: model loads, classifies, returns valid JSON (requires `builtin-llm` feature + model file)
  - Test that builds without `builtin-llm` feature still compile (no regressions)
  - **Files:** `backend/src/vectors/builtin_llm.rs` (inline tests), `backend/src/vectors/config.rs` (extend existing tests)

---

### Sprint RBL-2: Production Hardening (1 week)

**Goal:** Idle timeout, memory management, streaming chat, CLI model download.

#### Tasks

- [ ] **RBL-2.01**: Idle timeout and model lifecycle
  - Add `IdleModelGuard` that unloads model after `idle_timeout_secs` of inactivity
  - Use `tokio::time::sleep` with reset on each inference call
  - Lazy reload on next request
  - **File:** `backend/src/vectors/builtin_llm.rs`

- [ ] **RBL-2.02**: Memory checks
  - Before loading model, check available memory via `sysinfo` crate (optional dep)
  - Warn if RAM < model's estimate \* 1.2
  - Refuse to load if RAM < model's estimate
  - Log memory usage after load
  - **File:** `backend/src/vectors/builtin_llm.rs`

- [ ] **RBL-2.03**: Streaming chat via SSE
  - Implement token-by-token generation in `spawn_blocking`
  - Send tokens via `tokio::sync::mpsc` channel to the SSE endpoint
  - Integrate with existing `POST /api/v1/ai/chat/stream` endpoint
  - **File:** `backend/src/vectors/builtin_llm.rs`, `backend/src/api/ai.rs`

- [ ] **RBL-2.04**: CLI model management
  - Extend existing `--download-models` CLI flag to include GGUF models
  - Add `--download-llm` flag specifically for built-in LLM
  - Show download progress in terminal
  - **File:** `backend/src/main.rs`

- [ ] **RBL-2.05**: Update frontend generative router
  - Update `frontend/apps/web/src/services/ai/generative-router.ts`
  - When provider is `'builtin'`, route to backend API (`POST /api/v1/vectors/classify`) instead of in-process node-llama-cpp
  - This is the connection point: frontend settings → backend API
  - **File:** `frontend/apps/web/src/services/ai/generative-router.ts`

- [ ] **RBL-2.06**: Integration tests
  - E2E: frontend selects builtin → backend classifies via llama-cpp-2
  - Test idle timeout unloads model
  - Test concurrent classification requests share model weights
  - Test fallback to rule-based when model unavailable
  - **File:** `backend/src/vectors/builtin_llm.rs` (inline integration tests)

---

## 3. File Summary

### New Files

| File                                   | Sprint | Purpose                               |
| -------------------------------------- | ------ | ------------------------------------- |
| `backend/src/vectors/builtin_llm.rs`   | RBL-1  | BuiltInGenerativeModel implementation |
| `backend/src/vectors/gguf_download.rs` | RBL-1  | GGUF manifest + HuggingFace download  |

### Modified Files

| File                                    | Sprint | Changes                                      |
| --------------------------------------- | ------ | -------------------------------------------- |
| `backend/Cargo.toml`                    | RBL-1  | Add llama-cpp-2, hf-hub, builtin-llm feature |
| `backend/src/vectors/model_registry.rs` | RBL-1  | Add `BuiltIn` to ProviderType                |
| `backend/src/vectors/config.rs`         | RBL-1  | Add BuiltInLlmConfig                         |
| `backend/src/vectors/mod.rs`            | RBL-1  | Register new modules, wire startup           |
| `backend/config.yaml`                   | RBL-1  | Document builtin provider option             |
| `backend/src/main.rs`                   | RBL-2  | CLI flag for LLM download                    |
| `backend/src/api/ai.rs`                 | RBL-2  | SSE streaming integration                    |
| `frontend/.../generative-router.ts`     | RBL-2  | Route builtin to backend API                 |

---

## 4. Risk Assessment

| Risk                                           | Impact | Mitigation                                                   |
| ---------------------------------------------- | ------ | ------------------------------------------------------------ |
| llama-cpp-2 API breaks with upstream llama.cpp | Medium | Pin crate version; update on minor releases only             |
| CMake build fails on some systems              | Low    | Feature-gated; CPU fallback; documented prerequisites        |
| GBNF grammar edge cases                        | Low    | Hand-written grammar is simple; test with all model variants |
| Model download slow on first run               | Low    | CLI pre-download; progress logging; lazy loading             |

---

## 5. Success Criteria

| Metric                               | Target                               |
| ------------------------------------ | ------------------------------------ |
| `cargo build` without feature        | No regression (same build time)      |
| `cargo build --features builtin-llm` | < 6 min clean build                  |
| Classification latency (CPU)         | < 3 seconds per email                |
| Classification latency (Metal)       | < 1 second per email                 |
| RAM overhead (0.5B model)            | < 400 MB (Rust, no Node.js VM)       |
| Concurrent requests                  | 4+ simultaneous classifications      |
| Frontend API calls                   | Zero changes to existing fetch calls |
