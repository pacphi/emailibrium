# Plan: Externalized Model Catalog via YAML Configuration

- **Status**: Implemented (Phases 1-3 complete)
- **Research**: [2026 Model Leaderboard Research](../research/2026-model-leaderboard-research.md)
- **Date**: 2026-03-27
- **Scope**: Model selection, configuration, validation, and download for all AI providers

## Objective

Replace hardcoded model lists in Rust (`model_catalog.rs`, `generative_builtin.rs`) and
TypeScript (`model-manifest.ts`, `AISettings.tsx`) with two external YAML config files
that serve as the single source of truth for all model metadata. Maintainers edit YAML
to add/update models. The application reads these files at startup and surfaces them
in the UI with hardware-aware filtering and provider validation.

## Architecture

```text
config/
  models-llm.yaml        ← LLM models (generative/chat)
  models-embedding.yaml  ← Embedding models (vectorization)

        ┌─────────────────────────┐
        │     YAML Config Files   │
        │  (single source of truth)│
        └──────────┬──────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
  ┌────▼────┐ ┌───▼────┐ ┌───▼────────┐
  │  Rust   │ │ Node   │ │  Frontend  │
  │ Backend │ │(future)│ │ Settings UI│
  └─────────┘ └────────┘ └────────────┘
```

## YAML Schema: models-llm.yaml

```yaml
# Model catalog for LLM (generative) providers.
# Maintainers: add models here. The app validates provider compatibility at startup.
# Hardware filtering: models are shown to users only if their RAM meets min_ram_mb.

version: '1.0'

providers:
  builtin:
    description: 'Local GGUF models via llama.cpp (Rust backend)'
    download_required: true
    models:
      - id: 'qwen2.5-7b-q4km'
        name: 'Qwen 2.5 7B Instruct'
        family: 'qwen2'
        params: '7B'
        quantization: 'Q4_K_M'
        context_size: 32768
        disk_mb: 4700
        min_ram_mb: 5000
        quality: 'excellent'
        chat_template: 'chatml'
        repo_id: 'Qwen/Qwen2.5-7B-Instruct-GGUF'
        filename: 'qwen2.5-7b-instruct-q4_k_m.gguf'
        rag_capable: true
        default_for_ram_mb: 16384 # Recommended when user has >= 16GB
        tuning:
          temperature: 0.7
          top_p: 0.9
          repeat_penalty: 1.1
          max_tokens: 4096

  ollama:
    description: 'Local models via Ollama server'
    download_required: true
    base_url: 'http://localhost:11434'
    models:
      - id: 'llama3.2:3b'
        name: 'Llama 3.2 3B'
        params: '3B'
        context_size: 8192
        min_ram_mb: 2500
        quality: 'good'
        rag_capable: true
        tuning:
          temperature: 0.7
          num_predict: 512

  openai:
    description: 'OpenAI cloud models'
    download_required: false
    api_key_env: 'OPENAI_API_KEY'
    models:
      - id: 'gpt-4o'
        name: 'GPT-4o'
        context_size: 128000
        quality: 'excellent'
        rag_capable: true
        cost_per_1k_input: 0.0025
        cost_per_1k_output: 0.01
        tuning:
          temperature: 0.7
          max_tokens: 4096

  anthropic:
    description: 'Anthropic cloud models'
    download_required: false
    api_key_env: 'ANTHROPIC_API_KEY'
    models:
      - id: 'claude-sonnet-4-6'
        name: 'Claude Sonnet 4.6'
        context_size: 200000
        quality: 'excellent'
        rag_capable: true
        cost_per_1k_input: 0.003
        cost_per_1k_output: 0.015

  openrouter:
    description: 'OpenRouter proxy — access many models via one API'
    download_required: false
    api_key_env: 'OPENROUTER_API_KEY'
    base_url: 'https://openrouter.ai/api/v1'
    api_format: 'openai' # OpenAI-compatible API
    models:
      - id: 'meta-llama/llama-3.1-70b-instruct'
        name: 'Llama 3.1 70B (via OpenRouter)'
        context_size: 131072
        quality: 'excellent'
        rag_capable: true
        cost_per_1k_input: 0.00035
        cost_per_1k_output: 0.0004
```

## YAML Schema: models-embedding.yaml

```yaml
version: '1.0'

providers:
  onnx:
    description: 'Local ONNX models via fastembed (Rust backend)'
    download_required: true
    models:
      - id: 'all-MiniLM-L6-v2'
        name: 'MiniLM L6 v2'
        dimensions: 384
        max_tokens: 512
        disk_mb: 23
        min_ram_mb: 100
        quality: 'good'
        default: true
        description: 'Fast, lightweight English embedding (22M params)'

      - id: 'bge-small-en-v1.5'
        name: 'BGE Small EN v1.5'
        dimensions: 384
        max_tokens: 512
        disk_mb: 33
        min_ram_mb: 150
        quality: 'good'
        description: 'BAAI General Embedding, good for retrieval'

  ollama:
    description: 'Embedding via Ollama server'
    download_required: true
    base_url: 'http://localhost:11434'
    models:
      - id: 'nomic-embed-text'
        name: 'Nomic Embed Text'
        dimensions: 768
        context_size: 8192
        min_ram_mb: 500
        quality: 'excellent'

  openai:
    description: 'OpenAI embedding API'
    download_required: false
    api_key_env: 'OPENAI_API_KEY'
    models:
      - id: 'text-embedding-3-small'
        name: 'Text Embedding 3 Small'
        dimensions: 1536
        max_tokens: 8191
        quality: 'excellent'
        cost_per_1k_tokens: 0.00002
```

## Implementation Steps

### Phase 1: YAML Files + Rust Loader (Backend)

1. **Create `config/models-llm.yaml` and `config/models-embedding.yaml`**
   - Populate with researched models (2026 leaderboard data)
   - Include all providers: builtin, ollama, openai, anthropic, openrouter

2. **Create `src/vectors/model_config.rs`** — YAML deserializer
   - Structs: `LlmCatalog`, `EmbeddingCatalog`, `ProviderConfig`, `ModelEntry`
   - Load at startup, validate, filter by hardware
   - Replace `model_catalog.rs` and hardcoded `resolve_model()` in `generative_builtin.rs`

3. **Provider validation**
   - At startup: verify each model's provider compatibility
   - For builtin: check GGUF filename exists in HF repo format
   - For ollama: validate model tag format
   - For cloud: validate API key env var is set

4. **Update API endpoints**
   - `GET /api/v1/ai/model-catalog` reads from YAML, filters by hardware
   - `GET /api/v1/ai/embedding-catalog` new endpoint for embedding models
   - Include `providerCompatible: true/false` in response

### Phase 2: Frontend Integration

5. **Update `AISettings.tsx`**
   - Fetch model catalog from API (already started)
   - Remove hardcoded `LLM_MODELS` and `EMBEDDING_MODELS`
   - Show provider-filtered models with hardware recommendations

6. **Remove `model-manifest.ts`** and `ModelDownloadProgress` hardcoded manifest
   - Replace with API-driven data from the YAML catalog

### Phase 3: Download CLI + OpenRouter

7. **`make download-model MODEL=qwen2.5-7b-q4km`**
   - Reads YAML to find HF repo + filename
   - Downloads via `hf-hub` (Rust) or `ollama pull` (Ollama)
   - Verifies file integrity

8. **OpenRouter provider implementation**
   - OpenAI-compatible API format (reuse CloudGenerativeModel with custom base_url)
   - API key from `OPENROUTER_API_KEY`
   - Add to generative router as a new provider type

### Phase 4: Node Consistency

9. **Frontend YAML reader** (for Electron/desktop)
   - Read same YAML files for built-in-llm-manager model selection
   - Or: fetch from backend API (preferred for web app)

## Provider Validation Matrix

| Provider   | Validation                        | At Startup  | At Selection            |
| ---------- | --------------------------------- | ----------- | ----------------------- |
| builtin    | GGUF file exists or downloadable  | Check cache | Download if needed      |
| ollama     | Model tag valid, server reachable | Ping server | `ollama pull` if needed |
| openai     | API key env set                   | Check env   | Test API call           |
| anthropic  | API key env set                   | Check env   | Test API call           |
| openrouter | API key env set                   | Check env   | Test API call           |

## File Changes Summary

### New Files

- `config/models-llm.yaml` — LLM model catalog
- `config/models-embedding.yaml` — Embedding model catalog
- `backend/src/vectors/model_config.rs` — YAML loader + validator

### Modified Files

- `backend/src/vectors/model_catalog.rs` — Replace hardcoded catalog with YAML-driven
- `backend/src/vectors/generative_builtin.rs` — `resolve_model()` reads from YAML
- `backend/src/api/ai.rs` — Endpoints serve YAML-driven data
- `frontend/apps/web/src/features/settings/AISettings.tsx` — Remove hardcoded lists
- `frontend/apps/web/src/services/ai/model-manifest.ts` — Remove (replaced by API)
- `backend/Makefile` — Add `download-model` target

### Removed Files

- Hardcoded model lists in `model_catalog.rs` (replaced by YAML)
- Static `MODEL_MANIFEST` in `model-manifest.ts` (replaced by API)

## Success Criteria

1. Maintainer can add a new model by editing YAML — no code changes
2. `make models` shows hardware-filtered models from YAML
3. `make download-model MODEL=X` pre-caches any model
4. Settings UI shows only provider-compatible, hardware-capable models
5. Same YAML drives both Rust and Node implementations
6. OpenRouter works as a provider with any model in its catalog
