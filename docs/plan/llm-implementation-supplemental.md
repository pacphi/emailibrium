# LLM Implementation Supplemental Plan

## Emailibrium: Tiered AI Provider Architecture

Version 1.0 | Date: 2026-03-23 | Status: Sprint-Ready

---

## 1. Overview

This plan supplements the PRIMARY-IMPLEMENTATION-PLAN.md to deliver the tiered AI provider architecture described in docs/research/llm-options.md. It adds ONNX Runtime as the default embedding provider, implements model lifecycle management, and establishes the consent-gated cloud provider pathway.

Cross-references:

- Research: docs/research/llm-options.md
- ADRs: ADR-011 (ONNX Default), ADR-002 (Pluggable Embedding Model), ADR-008 (Privacy & Embedding Security)
- DDDs: DDD-001 (Email Intelligence bounded context)
- Existing code: backend/src/vectors/embedding.rs (EmbeddingModel trait), backend/src/vectors/config.rs (EmbeddingConfig)

### 1.1 Relationship to Primary Plan

The PRIMARY-IMPLEMENTATION-PLAN.md Sprints 1-7 establish the embedding trait, vector store, search, classification, and learning infrastructure. This supplemental plan adds three sprints (LLM-1 through LLM-3) that can run in parallel with or after Primary Plan Sprint 2 (which delivers the ingestion pipeline and hybrid search). The only hard prerequisite is that the `EmbeddingModel` trait and `EmbeddingPipeline` from Primary Plan Sprint 1 exist before LLM-1 begins.

### 1.2 Tiered Architecture Summary

| Tier | Name                  | Embedding                         | Generative                 | External Deps | Network Calls         |
| ---- | --------------------- | --------------------------------- | -------------------------- | ------------- | --------------------- |
| 0    | Default (zero-config) | fastembed ONNX (all-MiniLM-L6-v2) | None (rule-based fallback) | None          | None                  |
| 1    | Local Enhanced        | fastembed ONNX (same)             | Ollama (llama3.2:3b)       | Ollama daemon | Zero (localhost only) |
| 2    | Cloud Opt-in          | Cloud API (OpenAI/Cohere)         | Cloud LLM (Claude/GPT)     | API key       | Per-inference         |

---

## 2. Sprint Plan

### Sprint LLM-1: ONNX Embedding Default (1 week)

**Goal:** Replace mock as default embedding provider, deliver zero-config embedding that produces semantically meaningful vectors.

**Prerequisites:** Primary Plan Sprint 1 complete (EmbeddingModel trait, EmbeddingPipeline, VectorStore facade).

#### Tasks

- [ ] **LLM-1.01**: Add `fastembed` crate to Cargo.toml
  - Add `fastembed = "5.12"` to `backend/Cargo.toml` dependencies
  - fastembed transitively pulls in `ort` (ONNX Runtime bindings) and `tokenizers` (Hugging Face)
  - Verify compilation on macOS (Apple Silicon / aarch64-apple-darwin) and Linux (x86_64-unknown-linux-gnu)
  - Measure binary size impact: expect ~25 MB increase from ONNX Runtime shared library
  - Verify `cargo test` still passes with new dependency (no feature conflicts with existing moka, reqwest, tokio)
  - **File:** backend/Cargo.toml

- [ ] **LLM-1.02**: Implement `OnnxEmbeddingModel` struct
  - Create new struct in backend/src/vectors/embedding.rs implementing the existing `EmbeddingModel` trait (lines 31-46)
  - Struct fields: `model: fastembed::TextEmbedding`, `model_name: String`, `dims: usize`
  - Constructor `new()`: accepts `OnnxConfig`, calls `fastembed::TextEmbedding::try_new()` with `InitOptions`
  - Map config model name string to `fastembed::EmbeddingModel` enum variant (e.g., `"all-MiniLM-L6-v2"` maps to `AllMiniLML6V2`)
  - `embed()`: wraps synchronous fastembed call in `tokio::task::spawn_blocking()` to avoid blocking the async runtime
  - `embed_batch()`: uses fastembed batch API with configurable batch size from config
  - `is_available()`: returns `true` once model is initialized (fastembed handles download internally)
  - `dimensions()`: returns model output dimension (384 for all-MiniLM-L6-v2, 768 for bge-base, etc.)
  - `model_name()`: returns the string model identifier
  - Model auto-downloads from Hugging Face Hub on first construction; fastembed caches to `~/.cache/fastembed/` by default or to `model_path` if configured
  - **File:** backend/src/vectors/embedding.rs

- [ ] **LLM-1.03**: Add `OnnxConfig` to configuration
  - Add `OnnxConfig` struct to backend/src/vectors/config.rs with serde defaults:
    - `model: String` (default: `"all-MiniLM-L6-v2"`)
    - `model_path: Option<String>` (default: None, meaning fastembed default cache)
    - `show_download_progress: bool` (default: true)
    - `use_gpu: bool` (default: false)
    - `num_threads: usize` (default: 0, meaning auto-detect)
  - Add `onnx: OnnxConfig` field to existing `EmbeddingConfig` struct (line 110 of config.rs)
  - Provide `Default` impl with the values above
  - **File:** backend/src/vectors/config.rs

- [ ] **LLM-1.04**: Update `EmbeddingPipeline::new()` provider matching
  - Locate the provider match in EmbeddingPipeline construction (currently handles `"mock"` and `"ollama"`)
  - Add `"onnx"` match arm that constructs `OnnxEmbeddingModel` from config
  - Change `default_provider()` function from returning `"mock"` to returning `"onnx"`
  - Keep `"mock"` available: unit tests should explicitly set `provider: "mock"` in their test configs
  - Keep `"ollama"` available unchanged for Tier 1 users
  - Handle initialization failure gracefully: if ONNX model download fails (e.g., no internet on first run, firewall), log a clear error with instructions and fall back to mock with a warning
  - **Files:** backend/src/vectors/embedding.rs (pipeline construction), backend/src/vectors/config.rs (default change)

- [ ] **LLM-1.05**: Update configuration files
  - Update config/config.development.yaml: set `embedding.provider: "onnx"`, add `embedding.onnx` section
  - Update config/config.test.yaml: keep `embedding.provider: "mock"` so CI tests do not require model download
  - Add environment variable override documentation: `EMAILIBRIUM_EMBEDDING__PROVIDER=onnx`
  - **Files:** config/config.development.yaml, config/config.test.yaml

- [ ] **LLM-1.06**: Tests
  - Unit test: `OnnxEmbeddingModel` produces vectors of exactly 384 dimensions
  - Unit test: same input text returns identical embedding (deterministic output)
  - Unit test: semantically different texts produce different embeddings (cosine similarity < 0.95)
  - Unit test: semantically similar texts produce similar embeddings (cosine similarity > 0.7)
  - Unit test: `embed_batch()` returns correct count matching input count
  - Unit test: empty string input does not panic (returns valid vector or clear error)
  - Integration test: embed text, store in VectorStore, search by embedding, verify retrieval
  - Benchmark test (not in CI): single embed latency, batch throughput of 100 sentences, compare vs mock baseline
  - All tests gated behind `#[cfg(feature = "onnx-tests")]` or `#[ignore]` attribute so standard `cargo test` does not require model download
  - **Files:** backend/src/vectors/embedding.rs (inline tests), tests/integration/ (new integration test file)

- [ ] **LLM-1.07**: Documentation updates
  - Update docs/deployment-guide.md: ONNX is default, no Ollama needed for basic operation, first run downloads ~90 MB model
  - Update docs/configuration-reference.md: document new `embedding.onnx.*` fields
  - Note in docs that Ollama is now optional (Tier 1 enhancement, not a requirement)
  - **Files:** docs/deployment-guide.md, docs/configuration-reference.md

**Exit criteria:**

- `cargo build` succeeds on macOS ARM and Linux x86_64
- `cargo test` passes (mock provider used in test config)
- Running with `provider: "onnx"` auto-downloads model on first launch
- Single embed latency < 50 ms on CI-equivalent hardware
- 384-dimensional vectors produced for default model

---

### Sprint LLM-2: Model Lifecycle Management (1 week)

**Goal:** Manage model downloads, integrity verification, upgrades, and automatic re-indexing when the model changes.

**Prerequisites:** Sprint LLM-1 complete.

#### Tasks

- [ ] **LLM-2.01**: Model manifest system
  - Define `ModelManifest` struct: `model_name`, `provider` (onnx/ollama/cloud), `dimensions`, `sha256_hash`, `download_url`, `size_bytes`, `max_tokens`
  - Embed a static array of known manifests in the binary as a compile-time constant (covers all fastembed-supported models)
  - Allow user-specified models via config by providing a custom manifest entry
  - Store active model manifest in SQLite metadata table so the system can detect model changes across restarts
  - **File:** backend/src/vectors/models.rs (new file)

- [ ] **LLM-2.02**: Model download manager
  - `ModelDownloadManager` struct with async `download()` method
  - Download with progress callback signature `Fn(u64, u64)` (bytes_downloaded, total_bytes) for SSE streaming to frontend
  - SHA-256 verification after download completes; reject and delete file on mismatch
  - Retry logic: 3 attempts with exponential backoff (1s, 4s, 16s)
  - Respect `onnx.model_path` config for storage location; create directory if it does not exist
  - Offline handling: if model already exists in cache, use it regardless of network status; if model does not exist and network is unavailable, return `VectorError::ModelNotAvailable` with a message explaining manual download steps
  - Note: fastembed handles its own downloads internally; this manager wraps fastembed's behavior and adds SHA-256 verification, progress reporting, and the offline error path
  - **File:** backend/src/vectors/models.rs

- [ ] **LLM-2.03**: Re-indexing orchestrator
  - At startup, compare the active model name (from SQLite metadata) against the configured model name
  - If they differ (model changed):
    1. Log warning: "Embedding model changed from {old} to {new}. Re-indexing required."
    2. Update active model name in SQLite metadata
    3. Mark all emails as `embedding_status = 'stale'` in the emails table
    4. Enqueue a background re-embedding job via the existing ingestion pipeline
    5. Old vectors remain searchable during re-indexing (eventual consistency; search quality degrades gracefully until re-indexing completes)
  - If dimensions changed (e.g., switching from 384D to 768D model):
    1. Drop and recreate the HNSW index with new dimension parameter
    2. Mark all embeddings as stale (forces full re-embed)
    3. Log clear warning about the dimension change
  - Track re-indexing progress: count of stale vs. re-embedded emails
  - **File:** backend/src/vectors/reindex.rs (new file)

- [ ] **LLM-2.04**: API endpoints for model management
  - `GET /api/v1/ai/models` -- list all known models (from manifest), indicate which are downloaded and which is active
  - `GET /api/v1/ai/status` -- current provider config, active model name, dimensions, model file path, health check result
  - `POST /api/v1/ai/download` -- trigger model download by name (for frontend pre-download UX); returns SSE stream of progress events
  - `GET /api/v1/vectors/reindex-status` -- return `{ total: N, stale: M, progress_percent: P, estimated_remaining_seconds: S }`
  - All endpoints require authentication (existing auth middleware)
  - **Files:** backend/src/vectors/api.rs or backend/src/api/ai.rs (depending on routing structure)

- [ ] **LLM-2.05**: Frontend: AI status indicator
  - Add "AI" tab to Settings page
  - Display: current provider, model name, model dimensions, model file size, download status
  - First-run experience: if model not yet downloaded, show download button with progress bar
  - Re-indexing indicator: if re-indexing is in progress, show progress bar with estimated time remaining
  - Model selector dropdown (populated from GET /api/v1/ai/models) with "Apply" button that triggers model change
  - Warn user before model change: "Changing the embedding model will require re-indexing all emails. This may take several minutes."
  - **Files:** frontend/src/ (settings components)

- [ ] **LLM-2.06**: SQLite migration
  - Add `ai_metadata` table: `key TEXT PRIMARY KEY, value TEXT, updated_at TEXT`
  - Seed with `active_embedding_model` and `active_embedding_dimensions` keys
  - Add `embedding_status` column to emails table (values: `'current'`, `'stale'`, `'failed'`; default: `'current'`)
  - Migration file follows existing naming convention
  - **File:** backend/migrations/ (new migration file)

- [ ] **LLM-2.07**: Tests
  - Unit test: ModelManifest lookup by name returns correct dimensions and hash
  - Unit test: SHA-256 verification passes for valid file, fails for tampered file
  - Unit test: re-index orchestrator detects model name change and marks emails stale
  - Unit test: re-index orchestrator detects dimension change and triggers index rebuild
  - Unit test: offline behavior -- cached model works, uncached model returns clear error
  - Integration test: change model config, restart, verify re-indexing begins
  - **Files:** backend/src/vectors/models.rs (inline tests), tests/integration/

**Exit criteria:**

- Model auto-downloads on first run with progress reporting
- SHA-256 verification passes for all known models
- Model change triggers re-indexing; old vectors remain searchable during process
- Offline mode: cached model works, uncached model provides actionable error
- GET /api/v1/ai/status returns accurate information

---

### Sprint LLM-3: Generative AI Integration (1 week)

**Goal:** Add generative model support for classification fallback and chat, with consent-gated cloud provider pathway.

**Prerequisites:** Sprint LLM-1 complete. Sprint LLM-2 recommended but not strictly required.

#### Tasks

- [ ] **LLM-3.01**: Define `GenerativeModel` trait
  - New trait in backend/src/vectors/generative.rs (new file):

    ```rust
    #[async_trait]
    pub trait GenerativeModel: Send + Sync {
        /// Generate text from a prompt.
        async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError>;

        /// Classify text into one of the given categories.
        async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError>;

        /// Get the model name.
        fn model_name(&self) -> &str;

        /// Check if the model/service is available.
        async fn is_available(&self) -> bool;
    }
    ```

  - This mirrors the `EmbeddingModel` trait pattern (trait-based abstraction, async, provider-agnostic)
  - **File:** backend/src/vectors/generative.rs (new file)

- [ ] **LLM-3.02**: Implement `OllamaGenerativeModel`
  - Reuses the existing reqwest client pattern from `OllamaEmbeddingModel`
  - `generate()`: POST to `{ollama_url}/api/generate` with `model`, `prompt`, `stream: false`
  - `classify()`: constructs a structured prompt:

    ```
    Classify the following email into exactly one of these categories: [{categories}].
    Respond with only the category name, nothing else.

    Email:
    {text}
    ```

  - Parse response: extract the category string, verify it matches one of the input categories (case-insensitive), return error if no match
  - Configurable model name (default: `llama3.2:3b`), temperature (default: 0.1 for classification, 0.7 for chat), max_tokens
  - `is_available()`: GET `{ollama_url}/api/tags` and check if configured model is in the list
  - **File:** backend/src/vectors/generative.rs

- [ ] **LLM-3.03**: Implement `CloudGenerativeModel`
  - Support OpenAI chat completions API format (`POST /v1/chat/completions`)
  - Support Anthropic messages API format (`POST /v1/messages`)
  - Provider selected via `ai.generative.cloud.provider` config field
  - API key read from environment variable specified in `api_key_env` config (never from config file, never from SQLite)
  - Configurable base URL for OpenRouter / self-hosted API compatibility
  - Rate limiting: respect `rate_limit_rpm` config, use token bucket or sliding window
  - Retry on 429 (rate limit) and 5xx (server error) with exponential backoff
  - Content minimization: truncate input to configured max characters before sending to cloud
  - **File:** backend/src/vectors/generative.rs

- [ ] **LLM-3.04**: Wire generative into VectorCategorizer
  - Locate the classification logic where centroid confidence is evaluated against threshold
  - When centroid confidence < `confidence_threshold` (default 0.7) AND `generative_provider != "none"`:
    1. Call `generative.classify(email_text, category_names)`
    2. Parse the response to extract the predicted category
    3. Return `CategoryResult { category, confidence, method: "llm_fallback" }`
  - When centroid confidence < `confidence_threshold` AND `generative_provider == "none"` (Tier 0):
    1. Apply rule-based heuristic: sender domain patterns (e.g., `@github.com` implies "updates"), header analysis (List-Unsubscribe implies "promotions"), keyword scoring
    2. Return `CategoryResult { category, confidence, method: "rule_fallback" }`
  - When centroid confidence < `minimum_threshold` (default 0.3):
    1. Mark as "uncategorized" regardless of fallback result
  - Log the fallback method used for observability
  - **Files:** backend/src/vectors/categorizer.rs (or equivalent classification module)

- [ ] **LLM-3.05**: Wire generative into Chat API
  - Chat endpoint (`POST /api/v1/chat` or equivalent) receives user message
  - If generative provider is configured and available:
    1. Build context from relevant emails (use vector search to find related emails)
    2. Construct system prompt with email context
    3. Forward to `generative.generate()` with user message
    4. Stream response back via SSE
  - If no generative provider configured (Tier 0):
    1. Return a structured response explaining that chat requires Tier 1 or higher
    2. Provide template-based suggestions for common queries (e.g., "Show me unread emails from this week" maps to a search query)
  - **Files:** backend/src/api/ (chat endpoint)

- [ ] **LLM-3.06**: Consent management service
  - `ConsentManager` struct backed by SQLite
  - SQLite migration: create `ai_consent` table (`provider TEXT, consented_at TEXT, revoked_at TEXT NULL, user_acknowledgment TEXT`)
  - SQLite migration: create `ai_audit_log` table (`id INTEGER PRIMARY KEY, timestamp TEXT, provider TEXT, model TEXT, endpoint TEXT, input_token_count INTEGER, output_token_count INTEGER, input_hash TEXT, latency_ms INTEGER`)
  - `grant_consent(provider)`: records consent timestamp and user acknowledgment text
  - `revoke_consent(provider)`: sets revoked_at, immediately disables cloud calls for that provider
  - `has_consent(provider) -> bool`: checks for unrevoked consent
  - Before any cloud API call, check `has_consent()`; if false, return error prompting user to grant consent via Settings
  - Audit logging: after every cloud API call, write to `ai_audit_log` with request metadata (never log actual email content, only token counts and a SHA-256 hash of the input truncated to 8 hex characters)
  - API endpoints:
    - `GET /api/v1/ai/consent` -- current consent status per provider
    - `POST /api/v1/ai/consent` -- grant consent for a provider (body: `{ provider, acknowledgment }`)
    - `DELETE /api/v1/ai/consent/{provider}` -- revoke consent
    - `GET /api/v1/ai/audit` -- paginated audit log (query params: `page`, `per_page`, `provider`)
  - **Files:** backend/src/vectors/consent.rs (new file), backend/migrations/ (new migration)

- [ ] **LLM-3.07**: Frontend consent dialog
  - When user selects a cloud provider in Settings > AI, display a consent dialog before saving:
    - Title: "Cloud AI Provider Data Sharing"
    - Body: "You are enabling {provider} as your AI provider. When active, email subject lines, sender addresses, and body excerpts will be sent to {provider}'s servers for processing. This data will leave your machine. {provider}'s data handling is governed by their privacy policy."
    - Checkbox: "I understand that my email data will be sent to {provider}"
    - Buttons: "Cancel" / "Enable {provider}"
  - Consent dialog only appears on first enable or after revocation
  - Settings > AI > Privacy tab: show audit log table with pagination
  - **Files:** frontend/src/ (consent dialog component, settings AI tab)

- [ ] **LLM-3.08**: Generative configuration schema
  - Add `GenerativeConfig` struct to config.rs:
    ```yaml
    ai:
      generative:
        provider: 'none' # "none" | "ollama" | "cloud"
        ollama:
          url: 'http://localhost:11434'
          classification_model: 'llama3.2:3b'
          chat_model: 'llama3.2:3b'
          classification_max_tokens: 50
          chat_max_tokens: 1024
          classification_temperature: 0.1
          chat_temperature: 0.7
        cloud:
          provider: 'anthropic' # "anthropic" | "openai" | "google"
          api_key_env: 'ANTHROPIC_API_KEY'
          base_url: null # null = use provider default
          classification_model: 'claude-haiku-4-5-20251001'
          chat_model: 'claude-sonnet-4-20250514'
          rate_limit_rpm: 60
          max_input_chars: 2000 # truncate email text before sending
      consent:
        require_cloud_consent: true
        audit_cloud_calls: true
        show_cloud_data_warning: true
    ```
  - Default `provider: "none"` ensures Tier 0 zero-config works out of the box
  - Add `GenerativeConfig` and `ConsentConfig` to `VectorConfig` struct
  - **File:** backend/src/vectors/config.rs

- [ ] **LLM-3.09**: Tests and documentation
  - Unit test: `OllamaGenerativeModel.classify()` parses valid category from mock response
  - Unit test: `OllamaGenerativeModel.classify()` returns error when response does not match any category
  - Unit test: `CloudGenerativeModel` reads API key from env var, fails clearly when missing
  - Unit test: `ConsentManager.grant_consent()` and `has_consent()` roundtrip
  - Unit test: `ConsentManager` blocks cloud call when consent not granted
  - Unit test: audit log records correct metadata after cloud call
  - Unit test: rule-based fallback classifies `@github.com` sender as "updates"
  - Integration test: classification pipeline with Ollama mock server (wiremock or similar)
  - Integration test: consent flow -- grant, call cloud, audit entry created, revoke, call blocked
  - Update docs/configuration-reference.md with generative and consent config fields
  - Update docs/deployment-guide.md with Tier 1 (Ollama) and Tier 2 (Cloud) setup instructions
  - **Files:** backend/src/vectors/generative.rs (inline tests), backend/src/vectors/consent.rs (inline tests), tests/integration/, docs/

**Exit criteria:**

- `generative.provider: "none"` (default) produces no cloud or Ollama calls; classification uses centroid + rule fallback
- `generative.provider: "ollama"` calls Ollama for classification fallback when centroid confidence is low
- `generative.provider: "cloud"` is blocked until consent is granted via API
- Audit log records all cloud API calls with correct metadata
- Chat endpoint works with Ollama, returns informative response when no generative provider configured

---

## 3. Feature-to-Sprint Mapping

| Feature                                | Sprint | Priority | Notes                                            |
| -------------------------------------- | ------ | -------- | ------------------------------------------------ |
| fastembed crate integration            | LLM-1  | P0       | Foundation for all ONNX work                     |
| OnnxEmbeddingModel implementation      | LLM-1  | P0       | Implements existing EmbeddingModel trait         |
| OnnxConfig + config schema             | LLM-1  | P0       | Extends existing EmbeddingConfig                 |
| Default provider change (mock to onnx) | LLM-1  | P0       | Zero-config experience                           |
| Model manifest system                  | LLM-2  | P0       | Tracks known models and their properties         |
| Model download + SHA-256 verification  | LLM-2  | P0       | Integrity guarantee                              |
| Re-indexing on model change            | LLM-2  | P1       | Handles model upgrades gracefully                |
| AI status API endpoints                | LLM-2  | P1       | Frontend needs model info                        |
| Frontend AI settings tab               | LLM-2  | P1       | User-facing model management                     |
| SQLite ai_metadata migration           | LLM-2  | P0       | Required for model change detection              |
| GenerativeModel trait                  | LLM-3  | P1       | Abstraction for generative providers             |
| OllamaGenerativeModel                  | LLM-3  | P1       | Tier 1 generative capability                     |
| CloudGenerativeModel                   | LLM-3  | P2       | Tier 2 generative capability                     |
| Classification LLM fallback            | LLM-3  | P1       | Improves accuracy on ambiguous emails            |
| Rule-based fallback (Tier 0)           | LLM-3  | P1       | Zero-dependency classification fallback          |
| Consent management + audit log         | LLM-3  | P1       | Required before any cloud calls                  |
| Chat API backend                       | LLM-3  | P2       | Conversational AI features                       |
| Frontend consent dialog                | LLM-3  | P1       | User must consent before cloud usage             |
| Multilingual model support             | Future | P3       | multilingual-e5-small, same 384D                 |
| Image CLIP embedding                   | Future | P3       | Requires ImageEmbeddingModel trait               |
| Embedded GGUF generative (no Ollama)   | Future | P3       | candle or llama-cpp-rs for in-process generative |

---

## 4. Architecture Decisions

### 4.1 Why fastembed Over Direct ort Usage

fastembed wraps ort with model management (download, cache, tokenization) built in. Direct ort usage would require reimplementing tokenizer loading, model downloading, and output post-processing. fastembed eliminates approximately 300-500 lines of infrastructure code.

### 4.2 Why all-MiniLM-L6-v2 as Default

Per the research (docs/research/llm-options.md Section 4.2):

- 22M parameters, ~90 MB ONNX -- smallest viable model
- 384 dimensions -- matches the existing MockEmbeddingModel default and HNSW index configuration in config.rs
- Sub-50ms single-sentence latency on CPU
- MTEB ~56 average -- sufficient for email classification where input is subject + sender + truncated body
- Most widely deployed small embedding model; battle-tested at scale

### 4.3 Why Synchronous fastembed Wrapped in spawn_blocking

fastembed's API is synchronous (no tokio dependency). Rather than introducing a synchronous runtime conflict, wrapping calls in `tokio::task::spawn_blocking()` moves inference to the blocking thread pool. This keeps the async runtime responsive while ONNX inference runs on dedicated threads.

### 4.4 Why Rule-Based Fallback at Tier 0 Instead of Embedded Generative

A small GGUF model (e.g., Gemma-3-270M at 529 MB) could run in-process via candle or llama-cpp-rs. However:

- Adds 500+ MB to first-run download (vs 90 MB for embedding only)
- Adds llama.cpp C dependency or candle compile time
- Classification fallback is needed for only 5-15% of emails
- Sender domain patterns and header analysis handle most ambiguous cases adequately
- This keeps Tier 0 minimal; embedded generative is tracked as a Future item

### 4.5 ONNX Retained for Embedding Even at Tier 1 and 2

Per the research (Section 5.5), fastembed ONNX is faster than Ollama for embeddings (5-40ms vs 50-200ms, no HTTP overhead). Even when users enable Ollama for generative tasks, ONNX remains the embedding provider. This is a deliberate architectural choice: embeddings are high-frequency (every email), generative calls are low-frequency (5-15% of classifications + chat).

---

## 5. File Change Summary

### New Files

| File                                    | Sprint | Description                                |
| --------------------------------------- | ------ | ------------------------------------------ |
| backend/src/vectors/models.rs           | LLM-2  | ModelManifest, ModelDownloadManager        |
| backend/src/vectors/reindex.rs          | LLM-2  | Re-indexing orchestrator                   |
| backend/src/vectors/generative.rs       | LLM-3  | GenerativeModel trait + implementations    |
| backend/src/vectors/consent.rs          | LLM-3  | ConsentManager + audit logging             |
| backend/migrations/NNNN_ai_metadata.sql | LLM-2  | ai_metadata table, embedding_status column |
| backend/migrations/NNNN_ai_consent.sql  | LLM-3  | ai_consent + ai_audit_log tables           |

### Modified Files

| File                               | Sprint       | Changes                                                    |
| ---------------------------------- | ------------ | ---------------------------------------------------------- |
| backend/Cargo.toml                 | LLM-1        | Add fastembed dependency                                   |
| backend/src/vectors/embedding.rs   | LLM-1        | Add OnnxEmbeddingModel, update pipeline provider matching  |
| backend/src/vectors/config.rs      | LLM-1, LLM-3 | Add OnnxConfig, GenerativeConfig, ConsentConfig structs    |
| backend/src/vectors/mod.rs         | LLM-2, LLM-3 | Declare new modules (models, reindex, generative, consent) |
| backend/src/vectors/categorizer.rs | LLM-3        | Wire in generative fallback + rule-based fallback          |
| config/config.development.yaml     | LLM-1        | Change default provider to onnx, add onnx section          |
| config/config.test.yaml            | LLM-1        | Explicitly set provider to mock                            |
| docs/deployment-guide.md           | LLM-1, LLM-3 | Update prerequisites, add tier documentation               |
| docs/configuration-reference.md    | LLM-1, LLM-3 | Document new config fields                                 |

---

## 6. Risk Register

| ID  | Risk                                                                      | Likelihood | Impact | Mitigation                                                                                                                                                                                              |
| --- | ------------------------------------------------------------------------- | ---------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | fastembed crate has breaking API change                                   | Low        | High   | Pin exact version (`=5.12.0`). The `EmbeddingModel` trait facade means swapping fastembed for direct ort or another crate requires changing only one file.                                              |
| R2  | ONNX Runtime binary size unacceptable                                     | Low        | Low    | ~25 MB shared library is within budget for a desktop app. If needed, `ort` minimal-build feature reduces size.                                                                                          |
| R3  | Model download fails on first run (firewall, corporate proxy, air-gapped) | Medium     | Medium | Clear error message with manual download URL. Document `model_path` config for pre-staging models. Provide a `make download-models` target.                                                             |
| R4  | Re-indexing too slow for large inboxes (100k+ emails)                     | Medium     | Medium | Background process with progress reporting. Old vectors remain searchable during re-indexing. Batch processing with configurable concurrency. Estimate: 100k emails at 100 sentences/sec = ~17 minutes. |
| R5  | 384-dimension embeddings insufficient quality for classification          | Low        | Medium | MTEB ~56 is adequate for email triage. Users can upgrade to bge-base-en-v1.5 (768D, MTEB ~61) via config. Dimension change triggers automatic re-indexing.                                              |
| R6  | Cloud consent UX confuses users                                           | Low        | Medium | Clear, plain-language dialog. Consent is revocable. System immediately stops cloud calls on revocation.                                                                                                 |
| R7  | Ollama API changes break generative integration                           | Low        | Medium | Pin to documented API endpoints (/api/generate, /api/tags). Integration tests with wiremock catch regressions.                                                                                          |
| R8  | fastembed model cache conflicts with other apps using fastembed           | Low        | Low    | Configure `model_path` to use Emailibrium-specific directory (`~/.emailibrium/models/`) instead of shared `~/.cache/fastembed/`.                                                                        |
| R9  | ONNX Runtime telemetry on Windows                                         | Low        | Low    | Call `DisableTelemetryEvents()` during ort session initialization. Document in privacy policy. Not applicable on macOS/Linux.                                                                           |

---

## 7. Success Metrics

| Metric                                             | Target                                              | Measurement Method                                   |
| -------------------------------------------------- | --------------------------------------------------- | ---------------------------------------------------- |
| Default embed latency (ONNX, CPU, single sentence) | < 50 ms                                             | Benchmark test in CI, p95 latency                    |
| Batch embed throughput (ONNX, 32 sentences)        | > 100 sentences/sec                                 | Benchmark test                                       |
| First-run model download time (50 Mbps connection) | < 30 seconds                                        | Manual test, ~90 MB download                         |
| Classification accuracy with ONNX embeddings       | > 93%                                               | Evaluation framework from Primary Plan Sprint 7      |
| Memory footprint (ONNX model loaded)               | < 300 MB additional RSS                             | Measured via `/proc/self/status` or Activity Monitor |
| Binary size increase from fastembed + ort          | < 30 MB                                             | `ls -la` on release binary before/after              |
| Zero-config experience                             | `make dev` starts with working embeddings, no setup | Manual smoke test                                    |
| Re-indexing throughput                             | > 50 emails/sec                                     | Benchmark on 10k email dataset                       |
| Cloud audit log completeness                       | 100% of cloud calls logged                          | Integration test assertion                           |
| Consent gate reliability                           | 0 cloud calls without consent                       | Integration test assertion                           |

---

## 8. Dependencies

| Dependency                | Required By        | Version       | Notes                                                       |
| ------------------------- | ------------------ | ------------- | ----------------------------------------------------------- |
| `fastembed`               | LLM-1              | 5.12.x        | Rust crate; auto-downloads ONNX Runtime                     |
| `ort`                     | LLM-1 (transitive) | 2.0.0-rc.12   | Transitive via fastembed                                    |
| ONNX Runtime              | LLM-1 (transitive) | 1.24.x        | Shared library, downloaded by ort                           |
| `tokenizers`              | LLM-1 (transitive) | via fastembed | Hugging Face tokenizer                                      |
| Internet (first run only) | LLM-1              | N/A           | ~90 MB model download from Hugging Face Hub                 |
| `reqwest`                 | LLM-3              | existing      | Already in Cargo.toml; used for Ollama and cloud HTTP calls |
| `sha2`                    | LLM-2              | 0.10.x        | SHA-256 verification; may already be in dependency tree     |
| Ollama (optional)         | LLM-3              | any           | Only for Tier 1 generative features                         |
| Cloud API key (optional)  | LLM-3              | N/A           | Only for Tier 2; read from env var                          |
| Primary Plan Sprint 1     | LLM-1              | N/A           | EmbeddingModel trait, EmbeddingPipeline, VectorStore        |
| Primary Plan Sprint 2     | LLM-2              | N/A           | Ingestion pipeline (for re-indexing)                        |

---

## 9. Open Questions

| ID  | Question                                                                                               | Decision Needed By | Notes                                                                                                                                                               |
| --- | ------------------------------------------------------------------------------------------------------ | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Should the ONNX model cache use `~/.emailibrium/models/` or `~/.cache/fastembed/` (fastembed default)? | LLM-1 start        | Using Emailibrium-specific path avoids conflicts but means fastembed cannot share cache with other apps. Recommendation: use `~/.emailibrium/models/`.              |
| Q2  | Should quantized (INT8) models be the default for smaller download?                                    | LLM-1 start        | INT8 reduces download from ~90 MB to ~23 MB with minimal quality loss. Tradeoff: slightly lower accuracy. Recommendation: FP32 default, INT8 available via config.  |
| Q3  | Should the rule-based fallback (Tier 0, no generative) be a separate pluggable trait or inline logic?  | LLM-3 start        | A trait enables future swapping (e.g., embedded GGUF model). Recommendation: make it a trait with a `RuleBasedClassifier` default implementation.                   |
| Q4  | Should cloud audit logs be exportable (CSV/JSON)?                                                      | LLM-3              | Useful for compliance. Low effort to add. Recommendation: yes, add GET /api/v1/ai/audit?format=csv.                                                                 |
| Q5  | Should re-indexing be cancellable?                                                                     | LLM-2              | If a user changes the model again mid-reindex, the current reindex should be abandoned and restarted with the new model. Recommendation: yes, support cancellation. |
