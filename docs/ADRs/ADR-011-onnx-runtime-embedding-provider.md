# ADR-011: ONNX Runtime as Default Embedding Provider

- **Status**: Proposed
- **Date**: 2026-03-23
- **Extends**: ADR-002 (Embedding Model Selection Strategy)
- **Research References**: docs/research/llm-options.md -- Sections 3, 4, 5, 8

## Context

Emailibrium's current embedding subsystem has two providers: `MockEmbeddingModel` (hash-based, no semantic meaning) used as the default, and `OllamaEmbeddingModel` (requires separate Ollama installation and daemon) used for production. Neither satisfies the project's privacy-first, zero-configuration design principle:

1. **Mock provider (current default):** Produces deterministic pseudo-random vectors with no semantic meaning. Development-time testing of search quality, classification accuracy, and clustering behavior is impossible.

2. **Ollama provider (production):** Requires users to install Ollama separately, pull a ~274 MB model (`nomic-embed-text`), and keep a daemon running on localhost. This creates friction (installation steps, version mismatches, daemon management) and an unnecessary attack surface (HTTP API on localhost).

3. **Gap:** There is no provider that delivers real semantic embeddings with zero external dependencies.

The research in docs/research/llm-options.md evaluates the Rust ONNX ecosystem and identifies `fastembed` (backed by the `ort` crate's ONNX Runtime bindings) as a solution that delivers production-quality embeddings with zero external dependencies, zero network calls during inference, and automatic model management.

## Decision

Make ONNX-based embedding via the `fastembed` crate the DEFAULT provider (Tier 0). Change the config default from `provider: "mock"` to `provider: "onnx"`.

### Default Model

- **Model**: `all-MiniLM-L6-v2`
- **Dimensions**: 384 (matches existing HNSW index configuration and ADR-002's selection)
- **Parameters**: 22M
- **ONNX file size**: ~90 MB (FP32), ~23 MB (INT8 quantized)
- **MTEB average score**: ~56 (sufficient for email classification where input is subject + sender + truncated body)
- **Context length**: 256 tokens (adequate for `prepare_email_text()` which truncates body to ~80-100 tokens)

### Implementation Crate

- **Crate**: `fastembed` v5.12.0
- **Backend**: `ort` (ONNX Runtime Rust bindings, v2.0.0-rc.12)
- **License**: Apache-2.0
- **Async model**: Synchronous API; wrap calls in `tokio::task::spawn_blocking()`
- **Tokenization**: Built-in via the `tokenizers` crate (Hugging Face); no external tokenizer service

### Model Management

- Models auto-downloaded from Hugging Face Hub on first `embed()` call
- Cached to `~/.emailibrium/models/` (configurable via `ai.embedding.onnx.model_path`)
- SHA-256 checksum verification on download (checksums embedded in binary)
- Download progress bar displayed on first run (`show_download_progress: true`)

### Runtime Characteristics

- CPU-only by default; optional CoreML (macOS) / CUDA (Linux/Windows) via feature flags
- Single sentence latency: 5-40 ms depending on hardware
- Batch throughput: 80-600 sentences/sec depending on hardware
- Memory footprint: ~150-250 MB RSS (model + ONNX Runtime buffers)
- Model load time: 200-800 ms (one-time at startup)

### Provider Hierarchy

The embedding provider configuration supports four options, ordered by dependency requirements:

1. `"onnx"` **(NEW DEFAULT)** -- fastembed, zero external dependencies, auto-downloads model
2. `"ollama"` -- existing `OllamaEmbeddingModel`, requires Ollama installed and running
3. `"cloud"` -- OpenAI/Cohere/Voyage API, requires API key and explicit user consent
4. `"mock"` -- development/testing only, not for production use

### Fallback Behavior

- If ONNX model download fails (offline, no prior cache): return a clear error message with instructions ("Run `emailibrium --download-models` while connected to the internet, or switch to mock provider for development")
- If Ollama is configured but unreachable: fall back to ONNX with a logged warning
- If cloud is configured but consent not granted or API key missing: fall back to ONNX with a logged error

## Consequences

### Positive

- Zero-config semantic embeddings out of the box -- users get real search quality without installing anything
- Development and production use the same embedding provider by default, eliminating the mock/production gap
- Eliminates Ollama as a hard requirement for meaningful AI capabilities
- Faster than Ollama for embeddings: 5-40 ms vs. 50-200 ms per sentence (no HTTP overhead)
- Lower memory footprint than Ollama: ~200 MB vs. ~500-800 MB
- Full offline operation after initial model download
- No telemetry on macOS/Linux; explicit disable on Windows
- ONNX models are declarative computation graphs (no arbitrary code execution risk unlike pickle-based PyTorch models)

### Negative

- Adds ~15-25 MB to compiled binary size (ONNX Runtime shared library)
- First run requires ~90 MB model download (or ~23 MB for quantized variant); users without internet on first launch get an error
- Adds `fastembed` and `ort` to the dependency tree, increasing build time
- ONNX Runtime is a C library (via FFI); not pure Rust -- introduces a native dependency
- Model quality (MTEB ~56) is lower than larger models or cloud providers; sufficient for email triage but not state-of-the-art

### Neutral

- Model weights are ~90 MB on disk, loaded once at startup and held in memory for the process lifetime
- The `fastembed` API is synchronous; the `spawn_blocking` wrapper adds negligible overhead (~1 microsecond)
- Quantized (INT8) models are available for users who prefer smaller downloads at a minor quality cost

## Alternatives Considered

### Keep Ollama as Default Provider

- **Pros**: Simpler implementation (already exists), access to larger models, supports generative tasks too
- **Cons**: Requires separate installation (friction), requires daemon management, ~274 MB model pull, HTTP API attack surface on localhost, 50-200 ms latency per embedding (HTTP overhead), ~500-800 MB memory footprint
- **Verdict**: Rejected as default. Remains available as Tier 1 for users who want generative AI features.

### Embed Model in Binary via `include_bytes!()`

- **Pros**: Single self-contained executable, no first-run download
- **Cons**: Increases binary size by 90 MB (FP32) or 23 MB (INT8), makes binary too large for package managers and CI/CD, model updates require binary rebuild
- **Verdict**: Rejected. Auto-download on first run is the better trade-off.

### Use `tract` (Pure Rust ONNX Runtime)

- **Pros**: No C dependency, pure Rust, smaller binary
- **Cons**: Slower inference for transformer models, limited operator support for newer ONNX opsets, less community testing
- **Verdict**: Rejected. Performance and compatibility lag behind `ort`. Could be reconsidered if eliminating the C dependency becomes critical.

### Use `candle` (Hugging Face Rust ML Framework)

- **Pros**: Pure Rust, supports BERT/sentence-transformers, no C dependency, also supports generative models
- **Cons**: More complex API, slower than `ort` for inference, less mature embedding pipeline, would require building tokenization and model management that `fastembed` provides out of the box
- **Verdict**: Rejected for embeddings. May be relevant for future generative model integration without Ollama.

### Use `embed_anything` Crate

- **Pros**: Supports ONNX and Candle backends, multimodal (text, image, audio, PDF), streaming to vector databases
- **Cons**: Larger dependency tree, more complexity than needed for focused text embedding
- **Verdict**: Rejected for now. Could replace `fastembed` if multimodal embedding becomes a priority.

## Research References

- docs/research/llm-options.md -- Section 3 (ONNX Runtime Ecosystem), Section 4 (Embedding Model Catalog), Section 5 (fastembed Analysis), Section 8 (Configuration Design)
- ADR-002 (Embedding Model Selection Strategy) -- Establishes `all-MiniLM-L6-v2` as the primary model and defines the `EmbeddingModel` trait interface
- Wang, W., Wei, F., Dong, L., et al. (2020). "MiniLM: Deep Self-Attention Distillation for Task-Agnostic Compression of Pre-Trained Transformers." NeurIPS 2020.
