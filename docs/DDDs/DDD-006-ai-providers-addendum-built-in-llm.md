# DDD-006 Addendum: Built-in Local LLM Provider

| Field  | Value                              |
| ------ | ---------------------------------- |
| Status | Proposed                           |
| Date   | 2026-03-26                         |
| Type   | Addendum to DDD-006 (AI Providers) |
| ADR    | ADR-021 (Built-in Local LLM)       |

## Overview

This addendum extends DDD-006 (AI Providers Domain) with the structures, commands, events, and adapters needed to support Tier 0.5 — a built-in local LLM powered by `node-llama-cpp` and GGUF models. It does not replace any existing DDD-006 constructs; it adds to them.

---

## Changes to Existing Aggregates

### ProviderConfigAggregate — Extended

**ProviderType enum — add variant:**

```
enum ProviderType {
    Onnx,       // Tier 0: In-process ONNX Runtime via fastembed (embedding)
    BuiltInLlm, // Tier 0.5: In-process llama.cpp via node-llama-cpp (generative) [NEW]
    Ollama,     // Tier 1: Local Ollama HTTP API
    Cloud,      // Tier 2: Cloud API (OpenAI, Cohere, Anthropic)
    None,       // No provider (valid only for generative in Tier 0)
    Mock,       // Development/testing only
}
```

**ProviderTier enum — add variant:**

```
enum ProviderTier {
    Tier0,    // Zero-config default: ONNX embedding, no generative, no cloud
    Tier0_5,  // Built-in LLM: ONNX embedding + node-llama-cpp generative [NEW]
    Tier1,    // Local enhanced: ONNX embedding + Ollama generative
    Tier2,    // Cloud opt-in: Cloud embedding and/or generative (consent required)
}
```

**New command:**

- `SetGenerativeProvider { provider: ProviderType::BuiltInLlm, model_id: ModelId }` — switches generative to built-in LLM. Requires the model to be in `Ready` status in the ModelRegistry.

**New invariants:**

- `BuiltInLlm` provider requires `download_status == Ready` and `verified == true` for the selected GGUF model in ModelRegistryAggregate.
- `BuiltInLlm` provider does NOT require cloud consent (data never leaves the machine).
- Setting `generative_provider` to `BuiltInLlm` emits `ProviderChanged` with `requires_reindex: false` (generative model changes do not affect embeddings).

### ModelRegistryAggregate — Extended

**ModelManifest value object — add fields:**

| Field                 | Type             | Description                           |
| --------------------- | ---------------- | ------------------------------------- |
| format                | ModelFormat      | Onnx, Gguf [NEW]                      |
| quantization_level    | Option\<String\> | e.g., "Q4_K_M", "Q8_0", "FP16" [NEW]  |
| ram_estimate_bytes    | u64              | Estimated RAM usage when loaded [NEW] |
| hardware_requirements | Vec\<String\>    | e.g., ["metal", "cuda", "cpu"] [NEW]  |

**ModelFormat enum (new):**

```
enum ModelFormat {
    Onnx,   // ONNX Runtime models (.onnx files)
    Gguf,   // llama.cpp models (.gguf files)
}
```

**ModelCapability enum — add variant:**

```
enum ModelCapability {
    TextEmbedding,
    ImageEmbedding,
    Generative,
    GenerativeLocal,  // Built-in generative via GGUF [NEW] — distinct from cloud/Ollama generative
    CrossModal,
}
```

**New invariant:**

- GGUF models are stored under `~/.emailibrium/models/llm/` (separate from embedding models at `~/.emailibrium/models/embedding/`). The `path` field in ModelEntry reflects this separation.

**Default GGUF model manifest entries:**

| model_id          | model_name                   | format | quantization | dimensions | size_bytes | sha256   | download_url |
| ----------------- | ---------------------------- | ------ | ------------ | ---------- | ---------- | -------- | ------------ |
| qwen2.5-0.5b-q4km | Qwen2.5-0.5B-Instruct Q4_K_M | Gguf   | Q4_K_M       | N/A        | ~350 MB    | (pinned) | HF Hub       |
| smollm2-360m-q4km | SmolLM2-360M-Instruct Q4_K_M | Gguf   | Q4_K_M       | N/A        | ~250 MB    | (pinned) | HF Hub       |
| smollm2-1.7b-q4km | SmolLM2-1.7B-Instruct Q4_K_M | Gguf   | Q4_K_M       | N/A        | ~1 GB      | (pinned) | HF Hub       |
| llama3.2-3b-q4km  | Llama-3.2-3B-Instruct Q4_K_M | Gguf   | Q4_K_M       | N/A        | ~1.8 GB    | (pinned) | HF Hub       |
| phi3.5-mini-q4km  | Phi-3.5-mini-instruct Q4_K_M | Gguf   | Q4_K_M       | N/A        | ~2.3 GB    | (pinned) | HF Hub       |

### InferenceSessionAggregate — Extended

**Session behavior for BuiltInLlm:**

- Session creation loads the GGUF model into memory via `llama.loadModel()`. This is a heavyweight operation (~2-5 seconds) and should happen once, not per-request.
- Session holds a `LlamaModel` reference and creates `LlamaContext` instances for concurrent request handling.
- Memory management: the model uses `mmap` for efficient memory mapping. The OS manages page faults, so actual RAM usage depends on which parts of the model are hot.
- Session close calls `model.dispose()` to release native memory.
- Idle timeout: if no inference request for a configurable period (default: 5 minutes), the session is automatically closed to free RAM. Next request triggers a new session creation.

**New invariant:**

- Only one `BuiltInLlm` session may exist at a time (one model loaded in memory). Creating a session with a different GGUF model closes the existing session first.

---

## New Domain Service: BuiltInLlmManager

Manages the lifecycle of the built-in LLM: model discovery, download, loading, inference, and unloading.

### Responsibilities

1. **Model discovery** — Lists available GGUF models from the hardcoded manifest (pinned HF repos, pinned SHA-256 checksums). Does not discover arbitrary models.
2. **Model download** — Downloads GGUF files from Hugging Face Hub with progress reporting. Uses the existing `ModelDownloader` service from DDD-006, extended with GGUF support.
3. **Hardware detection** — At startup, probes available hardware acceleration (Metal, CUDA, Vulkan, CPU) and selects the optimal backend. Reports this to the UI so users understand performance expectations.
4. **Model loading** — Loads a GGUF model into an InferenceSession. Handles the `getLlama() → loadModel() → createContext()` lifecycle.
5. **Classification inference** — Executes structured JSON generation with grammar constraints for email classification.
6. **Chat inference** — Manages `LlamaChatSession` instances for conversational AI.
7. **Model unloading** — Releases native memory when idle or when switching models.

### Classification Schema

The grammar-constrained output schema for email classification:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "properties": {
    "category": {
      "type": "string",
      "enum": ["<dynamic from user categories>"]
    },
    "confidence": {
      "type": "number",
      "minimum": 0,
      "maximum": 1
    },
    "reasoning": {
      "type": "string",
      "maxLength": 200
    }
  },
  "required": ["category", "confidence"],
  "additionalProperties": false
}
```

The `enum` values for `category` are dynamically populated from the user's configured categories (DDD-007 Rules Domain). Grammar enforcement guarantees the model can only output a valid category — no parsing or retry needed.

---

## New Anti-Corruption Layer: BuiltInLlmAdapter

Wraps `node-llama-cpp` behind the `GenerativeModel` trait defined in DDD-006.

| Domain Method         | node-llama-cpp Operation                                                 |
| --------------------- | ------------------------------------------------------------------------ |
| `classify(prompt)`    | `session.prompt(prompt, { grammar: jsonSchema })` with structured output |
| `chat(messages)`      | `LlamaChatSession` with conversation history, streaming tokens           |
| `is_available()`      | Model file exists at expected path + `verified == true` in ModelRegistry |
| `load()`              | `getLlama() → loadModel({ modelPath }) → createContext()`                |
| `unload()`            | `model.dispose()` — releases native memory                               |
| `get_hardware_info()` | `llama.getGpuDeviceNames()`, `llama.getGpuVramState()`                   |

**Isolation guarantees:**

- All `node-llama-cpp` types are confined to the adapter. The domain never references `LlamaModel`, `LlamaContext`, or `LlamaChatSession` directly.
- The adapter translates `node-llama-cpp` errors to domain error types (`InferenceFailed`, `ModelLoadFailed`, `OutOfMemory`).
- Native memory management (dispose) is handled by the adapter's lifecycle hooks, not by domain code.
- Chat template selection is automatic (node-llama-cpp reads it from the GGUF metadata). The adapter does not expose template configuration to the domain.

---

## New Domain Events

| Event                      | Fields                                                      | Published When                            |
| -------------------------- | ----------------------------------------------------------- | ----------------------------------------- |
| BuiltInLlmModelSelected    | model_id, format, quantization, estimated_ram               | User selects a GGUF model in settings     |
| BuiltInLlmLoaded           | model_id, hardware_backend, load_time_ms, ram_used_bytes    | Model loaded into memory                  |
| BuiltInLlmUnloaded         | model_id, session_duration_ms, total_requests               | Model unloaded (idle timeout or explicit) |
| BuiltInLlmHardwareDetected | backends: Vec\<HardwareBackend\>, selected: HardwareBackend | Hardware probed at startup                |

### Event Consumers

| Event                      | Consumed By        | Purpose                                                     |
| -------------------------- | ------------------ | ----------------------------------------------------------- |
| BuiltInLlmModelSelected    | ModelRegistry      | Triggers download if not cached                             |
| BuiltInLlmLoaded           | Email Intelligence | Built-in generative now available for classification        |
| BuiltInLlmUnloaded         | Email Intelligence | Falls back to rule-based until next request triggers reload |
| BuiltInLlmHardwareDetected | Settings UI        | Displays hardware info and performance estimate             |

---

## Changes to Domain Services

### GenerativeRouter — Updated fallback chain

```
1. Read generative_provider from ProviderConfig
2. If None: return rule-based heuristic result (Tier 0 behavior)
3. If BuiltInLlm:                                         [NEW]
   a. Check if model is in Ready status in ModelRegistry
   b. If not downloaded, trigger lazy download (with progress event)
   c. If session not active, create one (load model)
   d. Execute inference via BuiltInLlmAdapter
   e. On failure: fall back to None behavior (rule-based)
4. If Ollama: delegate to Ollama HTTP client
5. If Cloud: verify consent, delegate to cloud API client
6. On failure at any tier: fall back one tier down
```

**Updated fallback chain:**

```
Cloud → Ollama → Built-in LLM → Rule-based heuristics
```

### ModelDownloader — Extended

Add GGUF download support:

- GGUF files are typically a single file (unlike ONNX which may have model + tokenizer + config)
- Download URL pattern: `https://huggingface.co/{owner}/{repo}/resolve/main/{filename}.gguf`
- SHA-256 verification: same as ONNX models, verified after download completes
- Progress reporting: uses existing `ModelDownloadProgress` event

---

## Changes to Value Objects

### HardwareBackend (new)

```
enum HardwareBackend {
    Metal,    // macOS GPU acceleration
    Cuda,     // NVIDIA GPU
    Vulkan,   // Cross-platform GPU
    Cpu,      // Software fallback
}
```

### GenerativeModelInfo (new)

| Field                | Type            | Description                            |
| -------------------- | --------------- | -------------------------------------- |
| model_id             | ModelId         | Model identifier                       |
| format               | ModelFormat     | Gguf                                   |
| quantization         | String          | e.g., "Q4_K_M"                         |
| context_length       | u32             | Max tokens in context window           |
| ram_estimate         | u64             | Estimated RAM in bytes                 |
| hardware_backend     | HardwareBackend | Active hardware acceleration           |
| supports_grammar     | bool            | Whether structured output is available |
| tok_per_sec_estimate | f32             | Estimated generation speed             |

---

## Configuration Mapping — Additions

| YAML Path                                  | Aggregate                     | Description                                                        |
| ------------------------------------------ | ----------------------------- | ------------------------------------------------------------------ |
| `ai.generative.builtin.*`                  | ProviderConfig, ModelRegistry | Built-in LLM settings [NEW]                                        |
| `ai.generative.builtin.model`              | ProviderConfig                | Selected GGUF model ID                                             |
| `ai.generative.builtin.idle_timeout_secs`  | InferenceSession              | Seconds before unloading idle model (default: 300)                 |
| `ai.generative.builtin.max_context_tokens` | InferenceSession              | Max context window for inference (default: 2048)                   |
| `ai.generative.builtin.temperature`        | InferenceSession              | Sampling temperature for chat (default: 0.7)                       |
| `ai.models.llm_cache_dir`                  | ModelRegistry                 | GGUF model cache directory (default: `~/.emailibrium/models/llm/`) |

Environment variable overrides: `EMAILIBRIUM_AI__GENERATIVE__BUILTIN__MODEL=qwen2.5-0.5b-q4km`

---

## Context Map Integration — Additions

```
AI Providers ──[Published Language]──> Email Intelligence
  Events: BuiltInLlmLoaded, BuiltInLlmUnloaded (NEW)
  Purpose: Signals when built-in generative is available/unavailable

AI Providers ──[Customer/Supplier]──> Hugging Face Hub (external)
  Direction: AI Providers downloads GGUF models per manifest spec (NEW format)
  Purpose: Model acquisition for built-in LLM

AI Providers ──[Published Language]──> Settings UI
  Events: BuiltInLlmHardwareDetected (NEW)
  Purpose: UI displays hardware capabilities and performance estimates
```

---

## Ubiquitous Language — Additions

| Term                               | Definition                                                                                                                                                                      |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Built-in LLM**                   | A small language model (0.5B-3.8B parameters) that runs in-process via node-llama-cpp, requiring no external service. Tier 0.5 in the provider hierarchy.                       |
| **GGUF**                           | The model file format used by llama.cpp. A single binary file containing model weights, tokenizer, and metadata. The standard format for local LLM inference.                   |
| **Grammar-constrained generation** | A technique where the LLM's token sampling is restricted by a formal grammar (JSON schema), guaranteeing that the output is valid structured data. Eliminates parsing failures. |
| **Hardware backend**               | The compute accelerator used for inference: Metal (macOS GPU), CUDA (NVIDIA GPU), Vulkan (cross-platform GPU), or CPU (software fallback). Detected automatically.              |
| **Idle timeout**                   | The period after which an unused built-in LLM session is closed to free RAM. The model is reloaded on the next inference request.                                               |
| **Lazy download**                  | Model download triggered by the first inference request rather than by explicit user action. Provides a seamless experience at the cost of a one-time delay.                    |

---

## Boundaries — Clarifications

- The `BuiltInLlmAdapter` lives in the AI Providers context, NOT in Email Intelligence. Email Intelligence calls `GenerativeRouter.classify()` without knowing whether the backend is built-in, Ollama, or cloud.
- The `node-llama-cpp` dependency exists only in the frontend/Electron layer (TypeScript). The Rust backend continues to use `fastembed`/`ort` for ONNX embeddings. There is no Rust-side GGUF integration.
- Model download for GGUF models uses the same `ModelDownloader` domain service as ONNX models. The service is extended to handle both formats, not duplicated.
