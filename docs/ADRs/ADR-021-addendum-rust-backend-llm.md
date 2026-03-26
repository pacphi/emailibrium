# ADR-021 Addendum: Rust Backend Built-in LLM via llama-cpp-2

- **Status**: Proposed
- **Date**: 2026-03-26
- **Extends**: ADR-021 (Built-in Local LLM), ADR-012 (Tiered AI Provider Architecture)
- **Research**: Rust LLM crate research (2026-03-26)

## Context

ADR-021 introduced Tier 0.5 (built-in local LLM) via `node-llama-cpp` in the frontend/Electron layer. However, the Emailibrium frontend runs as a Vite-bundled browser app in development, and `node-llama-cpp` requires a Node.js runtime (native C++ bindings). Vite's bundler resolves `ipull` (a node-llama-cpp dependency) to its browser entry point, which lacks the required exports, causing build failures.

The Rust backend at `:8080` already has:

1. A `GenerativeModel` trait with `generate()`, `classify()`, `model_name()`, `is_available()`
2. A `GenerativeRouter` with priority-based failover across providers
3. A `ProviderType` enum (Onnx, Ollama, OpenAi, Anthropic, Gemini, RuleBased, None)
4. A `ModelRegistry` with full lifecycle state machine
5. A `ChatService` with session management and SSE streaming
6. Existing API endpoints: `POST /api/v1/ai/chat`, `POST /api/v1/vectors/classify`
7. ONNX embedding via `fastembed` using the same `tokio::task::block_in_place()` pattern

Adding the built-in LLM to the Rust backend instead of the frontend is a natural extension — the frontend simply calls the existing REST API.

## Decision

### 1. Use `llama-cpp-2` Crate in the Rust Backend

Add `llama-cpp-2` (Rust bindings for llama.cpp) as an optional dependency behind a `builtin-llm` Cargo feature flag. This implements `GenerativeModel` as a new provider type (`BuiltIn`) that slots into the existing `GenerativeRouter`.

### 2. Why `llama-cpp-2` Over Alternatives

| Crate         | Grammar            | Library-friendly     | Dependencies         | Verdict            |
| ------------- | ------------------ | -------------------- | -------------------- | ------------------ |
| `llama-cpp-2` | GBNF built-in      | Yes                  | Minimal (C++ build)  | **Selected**       |
| `candle`      | None               | Yes                  | Minimal (pure Rust)  | No grammar support |
| `mistral.rs`  | GBNF + JSON Schema | No (server-oriented) | Heavy                | Overkill           |
| `kalosm`      | Yes                | Yes                  | Heavy (voice, image) | Too many deps      |

### 3. Architecture

```
Frontend (browser)                    Rust Backend (:8080)
  POST /api/v1/vectors/classify  →  GenerativeRouter
  POST /api/v1/ai/chat           →    ├─ BuiltInGenerativeModel  [NEW — Tier 0.5]
                                       ├─ OllamaGenerativeModel   [Tier 1]
                                       ├─ CloudGenerativeModel    [Tier 2]
                                       └─ RuleBasedClassifier     [Tier 0 fallback]
```

The frontend makes the same API calls it already does. No browser-side LLM inference needed.

### 4. Configuration

```yaml
generative:
  provider: 'builtin' # NEW value alongside "none", "ollama", "cloud"
  builtin:
    model_id: 'qwen2.5-0.5b-q4km'
    context_size: 2048
    gpu_layers: 99 # offload all to Metal/CUDA; 0 = CPU only
    idle_timeout_secs: 300
    cache_dir: '~/.emailibrium/models/llm'
```

### 5. Feature-Gated Build

```toml
[features]
builtin-llm = ["dep:llama-cpp-2", "dep:hf-hub"]
builtin-llm-metal = ["builtin-llm", "llama-cpp-2/metal"]
builtin-llm-cuda = ["builtin-llm", "llama-cpp-2/cuda"]
```

- `cargo build` — no built-in LLM (default, CI-friendly)
- `cargo build --features builtin-llm` — CPU-only inference
- `cargo build --features builtin-llm-metal` — Metal acceleration on macOS

### 6. Thread Safety

- `LlamaModel` is `Send + Sync` → shared via `Arc<LlamaModel>` in `AppState`
- `LlamaContext` is NOT `Sync` → created per-request inside `spawn_blocking`
- Pattern matches existing `OnnxEmbeddingModel` which wraps `fastembed::TextEmbedding` in `Mutex` with `block_in_place()`

### 7. GBNF Grammar for Classification

```
root   ::= "{" ws q-cat ws ":" ws cat-val "," ws q-conf ws ":" ws number "}" ws
q-cat  ::= "\"category\""
q-conf ::= "\"confidence\""
cat-val ::= "\"" [a-zA-Z]+ "\""
number ::= "0." [0-9] [0-9]?
ws     ::= [ \t\n]*
```

Categories are injected dynamically into the `cat-val` rule at classification time.

## Consequences

### Positive

- **Works in browser dev flow** — no native Node.js modules in frontend bundle
- **50-80 MB less RAM** — no Node.js VM overhead (~370 MB vs ~430 MB)
- **Consistent architecture** — follows existing `GenerativeModel` trait pattern exactly
- **Feature-gated** — does not affect build times for developers who don't enable it
- **Same engine, same models** — llama.cpp underneath, identical GGUF compatibility

### Negative

- **30-90s longer clean build** — llama.cpp compiles from source via cmake (cached after first build)
- **+5-8 MB binary size** — llama.cpp static library linked into the backend binary
- **GBNF grammar must be hand-written** — no auto JSON Schema → GBNF converter (simple for classification schema)

### Relationship to Frontend Implementation

The frontend `node-llama-cpp` code from BL-1 through BL-4 remains intact for future Electron packaging. It serves as:

1. Reference implementation for the Rust-side port
2. The actual runtime when the app ships as Electron (direct in-process inference)
3. A working test bed for model selection, download UX, and progress tracking

## References

- ADR-021: Built-in Local LLM via node-llama-cpp
- ADR-012: Tiered AI Provider Architecture
- DDD-006: AI Providers Domain
- [llama-cpp-2 crate](https://crates.io/crates/llama-cpp-2)
- [hf-hub crate](https://crates.io/crates/hf-hub)
