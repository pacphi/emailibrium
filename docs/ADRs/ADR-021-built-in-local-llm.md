# ADR-021: Built-in Local LLM via node-llama-cpp

- **Status**: Proposed
- **Date**: 2026-03-26
- **Extends**: ADR-012 (Tiered AI Provider Architecture), DDD-006 (AI Providers Domain)
- **Research References**: Conversation research on local LLM runtimes (2026-03-26)

## Context

The tiered AI provider architecture (ADR-012) defines three tiers:

| Tier | Embedding        | Generative        | External Deps |
| ---- | ---------------- | ----------------- | ------------- |
| 0    | ONNX (fastembed) | None (rule-based) | None          |
| 1    | ONNX             | Ollama            | Ollama daemon |
| 2    | Cloud            | Cloud LLM         | API key       |

Embedding has a zero-config, built-in option (ONNX via fastembed) that runs entirely in-process with no external dependencies. Generative does not — the jump from "None (rule-based)" to "Ollama" requires users to install a separate application, pull a multi-GB model, and keep a daemon running. This creates a gap:

1. **High friction for basic classification.** Users who want AI-powered email classification (beyond keyword heuristics) must install Ollama, a non-trivial prerequisite that many users will not complete.
2. **No generative fallback at Tier 0.** When Ollama is unavailable (not installed, daemon stopped, model not pulled), generative falls back to rule-based heuristics. There is no intermediate option.
3. **Inconsistent UX.** Embedding has "Built-in (ONNX) Recommended" as the default. The LLM provider section has "None (Rule-based) Default" — implying the app works best with an external service.

The RuVector frontend layer (Node.js/TypeScript) already uses ONNX Runtime for embeddings. The question is whether a similar built-in option is feasible for generative inference.

### Why Not ONNX for LLM?

ONNX Runtime can technically run generative models, but:

- `onnxruntime-genai` (which handles the generation loop, KV cache, and sampling) has **no JavaScript/Node.js package** — it is Python-only.
- Raw `onnxruntime-node` provides tensor inference but no generation loop, no tokenization, and no chat template support. Building this from scratch would be substantial effort.
- Model availability in ONNX format for generation is limited compared to GGUF.

### Why node-llama-cpp?

| Criterion          | node-llama-cpp               | Transformers.js         | WebLLM                     |
| ------------------ | ---------------------------- | ----------------------- | -------------------------- |
| Performance        | Native C++ (llama.cpp)       | WASM (slower)           | WebGPU only                |
| Model format       | GGUF (broadest availability) | ONNX (limited for gen)  | MLC (requires compilation) |
| Hardware accel     | Metal, CUDA, Vulkan (auto)   | WebGPU only             | WebGPU only                |
| Electron support   | First-class + @electron/llm  | Works                   | Renderer only              |
| Structured JSON    | Built-in grammar enforcement | Manual parsing          | Manual                     |
| Chat templates     | Automatic Jinja2             | Manual                  | Automatic                  |
| Model download API | Built-in CLI + programmatic  | First-use auto-download | Manual                     |

## Decision

### 1. Add Tier 0.5: Built-in Local LLM

Insert a new tier between Tier 0 and Tier 1 in the provider hierarchy:

| Tier    | Name             | Embedding            | Generative                | External Deps                  |
| ------- | ---------------- | -------------------- | ------------------------- | ------------------------------ |
| 0       | Zero-config      | ONNX (fastembed)     | None (rule-based)         | None                           |
| **0.5** | **Built-in LLM** | **ONNX (fastembed)** | **node-llama-cpp (GGUF)** | **None (model auto-download)** |
| 1       | Local Enhanced   | ONNX                 | Ollama                    | Ollama daemon                  |
| 2       | Cloud Opt-in     | Cloud                | Cloud LLM                 | API key                        |

### 2. Use node-llama-cpp as the Runtime

Add `node-llama-cpp` (v3.x) as an optional dependency in the frontend/Electron layer. It provides:

- In-process GGUF model loading via native C++ bindings (N-API)
- Automatic hardware detection (Metal on macOS, CUDA on Linux/Windows, CPU fallback)
- Structured JSON output via grammar-constrained generation
- Built-in model downloading from Hugging Face Hub

### 3. Default Model: Qwen2.5-0.5B-Instruct (GGUF Q4_K_M)

| Property      | Value                                         |
| ------------- | --------------------------------------------- |
| Model         | Qwen2.5-0.5B-Instruct                         |
| Parameters    | 0.5B                                          |
| Quantization  | Q4_K_M                                        |
| Disk size     | ~350 MB                                       |
| RAM usage     | ~500 MB                                       |
| CPU speed     | 20-40 tok/s                                   |
| Metal speed   | 40-80 tok/s                                   |
| Primary use   | Email classification (structured JSON output) |
| Secondary use | Basic chat (acceptable quality for email Q&A) |

Alternative models available via Settings:

| Model                        | Disk    | RAM     | Use Case                              |
| ---------------------------- | ------- | ------- | ------------------------------------- |
| SmolLM2-360M-Instruct        | ~250 MB | ~400 MB | Ultra-lightweight classification only |
| SmolLM2-1.7B-Instruct        | ~1 GB   | ~1.5 GB | Better classification + basic chat    |
| Llama-3.2-3B-Instruct        | ~1.8 GB | ~2.5 GB | High-quality chat + classification    |
| Phi-3.5-mini-instruct (3.8B) | ~2.3 GB | ~3 GB   | Best quality, higher resource usage   |

### 4. Model Lifecycle: Pre-download and Cache

Models follow the same lifecycle as ONNX embedding models (DDD-006 ModelRegistryAggregate):

**Download triggers (in priority order):**

1. **CLI pre-download** — `emailibrium models download --model qwen2.5-0.5b` before first launch
2. **Settings UI** — "Download Model" button when user selects "Built-in (Local)" in AI/LLM settings
3. **Onboarding flow** — model download step during first-run setup (AISetup.tsx)
4. **Lazy download** — automatic download on first classification request (with progress indicator)

**Cache location:** `~/.emailibrium/models/llm/` (sibling to `~/.emailibrium/models/embedding/`)

**Integrity:** SHA-256 verification against model manifest, consistent with DDD-006 ModelRegistryAggregate invariants.

### 5. UI Changes

**Settings → AI/LLM → LLM Provider** adds a new option:

```text
○ None (Rule-based)     [Default]
○ Built-in (Local)      [NEW — auto-downloads ~350 MB model]
○ Local (Ollama)
○ OpenAI
○ Anthropic
```

When "Built-in (Local)" is selected:

- Show model selector dropdown (Qwen2.5-0.5B default, with alternatives)
- Show download status/progress if model not yet cached
- Show model size and estimated RAM usage
- Show "Download Now" button if model not cached

### 6. Provider Integration

The `BuiltInLlmAdapter` implements the same `GenerativeModel` trait from DDD-006:

| Domain Method      | node-llama-cpp Operation                               |
| ------------------ | ------------------------------------------------------ |
| `classify(prompt)` | `session.prompt()` with JSON schema grammar constraint |
| `chat(messages)`   | `LlamaChatSession` with message history                |
| `is_available()`   | Model file exists + verified + loadable                |

**Fallback chain update:**

```text
Cloud → Ollama → Built-in LLM → Rule-based heuristics
```

### 7. Structured Classification Output

node-llama-cpp's grammar enforcement guarantees valid JSON:

```json
{
  "category": "promotions",
  "confidence": 0.87,
  "reasoning": "Subject mentions 'sale' and sender is a retail brand"
}
```

The JSON schema is enforced at the token generation level — the model physically cannot produce invalid JSON. This eliminates the need for output parsing and retry logic.

## Consequences

### Positive

- **Zero-config generative AI.** Users get classification and basic chat without installing Ollama or paying for a cloud API.
- **Consistent with ONNX pattern.** Same UX as "Built-in (ONNX)" for embeddings — download once, runs locally forever.
- **Privacy preserved.** No data leaves the machine. No daemon running. No localhost HTTP API.
- **Graceful degradation.** Adds a fallback tier between Ollama and rule-based, reducing how often users hit the heuristic floor.
- **Structured output.** Grammar-constrained generation eliminates classification output parsing failures.

### Negative

- **~350 MB download.** Larger than ONNX embedding models (~23-90 MB). Mitigated by pre-download options and progress indicators.
- **~500 MB RAM.** Significant on memory-constrained systems. Model is loaded on-demand and can be unloaded when not in use.
- **Native compilation.** `node-llama-cpp` compiles C++ at install time, which can fail on systems without build tools. Mitigated by prebuilt binaries that the package ships for common platforms.
- **Quality ceiling.** A 0.5B model will not match GPT-4o or Claude Sonnet quality. This is Tier 0.5, not Tier 2. Users wanting higher quality are directed to Ollama (Tier 1) or cloud (Tier 2).
- **New dependency.** Adds `node-llama-cpp` (~5 MB npm + native binary) to the frontend dependency tree.

### Alternatives Considered

| Alternative                               | Why Rejected                                                                                                                             |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Transformers.js for generation            | WASM-based, 3-5x slower on CPU than native llama.cpp. Limited ONNX model availability for generation.                                    |
| WebLLM / MLC-LLM                          | WebGPU-only, no Node.js main process support, requires model compilation to MLC format.                                                  |
| Raw onnxruntime-node for generation       | No generation loop, tokenizer, or chat template support in JavaScript. Would require building onnxruntime-genai equivalent from scratch. |
| Ollama as default (lower Tier 1 friction) | Still requires separate install + daemon. Cannot be fully in-process. Does not solve the zero-config gap.                                |
| Wait for @electron/llm to mature          | Experimental, not production-ready. Built on node-llama-cpp anyway — adopting the base library now provides an upgrade path.             |

## References

- ADR-011: ONNX Runtime as Default Embedding Provider
- ADR-012: Tiered AI Provider Architecture
- DDD-006: AI Providers Domain
- [node-llama-cpp documentation](https://node-llama-cpp.withcat.ai/)
- [node-llama-cpp Electron guide](https://node-llama-cpp.withcat.ai/guide/electron)
- [@electron/llm](https://github.com/electron/llm)
- [Qwen2.5-0.5B-Instruct on Hugging Face](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct)
