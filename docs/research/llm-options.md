# Embedded ONNX Model Options for Emailibrium

## Research Report: Local-First AI Inference via ONNX Runtime

**Date:** 2026-03-23
**Status:** Research Complete
**Applicability:** Emailibrium v0.x -- AI/ML subsystem design

---

## 1. Abstract

Emailibrium's current AI subsystem relies on an `EmbeddingModel` trait with two implementations: `MockEmbeddingModel` for development and `OllamaEmbeddingModel` for production. Both fall short of the project's privacy-first, zero-configuration design principle. The mock model produces meaningless vectors; Ollama requires users to install and maintain a separate service. This report evaluates the feasibility and implementation path for embedding ONNX-format machine learning models directly into the Emailibrium binary (or auto-downloading them on first run), eliminating all external dependencies for the default configuration. We survey the Rust ONNX ecosystem (`ort`, `fastembed`, `candle`), catalog viable embedding and generative models with their size/quality/performance trade-offs, propose a three-tier provider architecture (ONNX default, Ollama enhanced, Cloud opt-in), and present a complete YAML configuration schema. The analysis concludes that the `fastembed` crate, backed by the `ort` ONNX Runtime bindings, can deliver production-quality 384-dimensional text embeddings on CPU in under 50ms per sentence with approximately 80--130 MB of model storage, zero network dependency, and full privacy guarantees.

---

## 2. Introduction

### 2.1 The Problem

Emailibrium is a privacy-first, local-first email intelligence platform. Its value proposition depends on a hard guarantee: **user email data never leaves the machine unless the user explicitly opts in.** The current architecture undermines this guarantee at the AI layer:

1. **Ollama dependency (production):** Requires users to install Ollama separately, pull models (~4 GB for `nomic-embed-text`), and keep a daemon running. This creates friction, failure modes (Ollama not running, wrong model pulled, version mismatches), and a large attack surface (HTTP API on localhost).

2. **Mock model (development):** The `MockEmbeddingModel` uses deterministic hash-based pseudo-random vectors. These have no semantic meaning, making development-time testing of search quality, classification accuracy, and clustering behavior impossible.

3. **No generative capability:** The classification fallback (when vector centroid confidence is below 0.7) and the planned chat/rule-building features have no local implementation path without Ollama or a cloud API.

### 2.2 Design Principle

The default Emailibrium installation should provide meaningful AI capabilities with **zero external dependencies**:

- No Ollama installation required
- No cloud API keys required
- No network calls during inference
- No background services to manage
- Models either ship with the binary or auto-download on first run (with SHA-256 integrity verification)
- Users who want enhanced capabilities can opt into Ollama or cloud providers via YAML configuration

### 2.3 Scope

This report covers:

- ONNX Runtime integration for Rust via the `ort` and `fastembed` crates
- A catalog of embedding models available in ONNX format
- Small generative models suitable for email classification
- A tiered architecture proposal with complete configuration design
- Privacy and security analysis across all provider tiers
- An implementation roadmap with specific code changes required

---

## 3. ONNX Runtime Ecosystem

### 3.1 What is ONNX Runtime?

ONNX Runtime is Microsoft's cross-platform, hardware-accelerated inference engine for models in the Open Neural Network Exchange (ONNX) format. It supports CPU, CUDA (NVIDIA), DirectML (Windows), CoreML (macOS/iOS), and NNAPI (Android) execution providers. ONNX has become the de facto standard for portable model deployment, with export support from PyTorch, TensorFlow, JAX, and sentence-transformers.

### 3.2 The `ort` Crate (Rust Bindings)

The `ort` crate (maintained by pyke.io) is the primary Rust interface to ONNX Runtime. It wraps the ONNX Runtime C API in safe, idiomatic Rust.

| Property             | Value                                      |
| -------------------- | ------------------------------------------ |
| Crate                | `ort`                                      |
| Latest version       | 2.0.0-rc.12 (recommended for new projects) |
| ONNX Runtime version | 1.24.x                                     |
| Sys crate            | `ort-sys` (auto-generated FFI bindings)    |
| License              | MIT / Apache-2.0                           |
| Repository           | https://github.com/pykeio/ort              |
| Documentation        | https://ort.pyke.io/                       |

**Key features:**

- **Three linking strategies:** `download` (default, fetches prebuilt ONNX Runtime from Microsoft), `compile` (builds from source), `system` (links against a user-provided library via `ORT_LIB_LOCATION`).
- **Execution providers:** CPU (default), CUDA, TensorRT, CoreML, DirectML, OpenVINO, NNAPI, XNNPACK, QNN, CANN, ROCm, MIGraphX, ACL, and ArmNN.
- **Minimal build option:** The `minimal-build` feature strips RTTI, `.onnx` format support, and runtime optimizations, drastically reducing binary size for release builds.
- **Dynamic library management:** The `copy-dylibs` feature automatically copies ONNX Runtime shared libraries to the Cargo target directory, solving deployment issues on Windows and macOS.

**Binary size and memory impact:**

- ONNX Runtime shared library (CPU only): approximately 15--25 MB depending on platform
- Rust `ort` wrapper overhead: negligible (thin FFI layer)
- Peak memory during inference: model size + input tensor + output tensor + ONNX Runtime internal buffers (typically 2--3x model file size for small models)

**GPU acceleration on macOS (Apple Silicon):**

GPU acceleration on Apple Silicon uses the CoreML execution provider, which can also leverage the Neural Processing Unit (NPU). This is enabled via the `coreml` feature flag in `ort`. For Emailibrium's default CPU-only tier, this is unnecessary but available as an optimization for users who enable it.

### 3.3 Alternative Rust ONNX Crates

| Crate            | Status       | Notes                                                                                  |
| ---------------- | ------------ | -------------------------------------------------------------------------------------- |
| `onnxruntime`    | Inactive     | Original Rust bindings, superseded by `ort`                                            |
| `onnxruntime-ng` | Low activity | Fork of `onnxruntime`, limited updates                                                 |
| `wonnx`          | Active       | WebGPU-accelerated, pure Rust, but limited operator support                            |
| `tract`          | Active       | Pure Rust ONNX/NNEF runtime, no C dependency; slower than `ort` for transformer models |

**Recommendation:** Use `ort` via `fastembed` (see Section 5). The `ort` crate is the most mature, best-maintained, and highest-performance option. `tract` is a viable fallback if eliminating the C dependency is critical, but its transformer support and performance lag behind `ort`.

### 3.4 Bundling Models with the Application

There are three strategies for shipping ONNX models with Emailibrium:

1. **Embed in binary (compile-time):** Use `include_bytes!()` to bake the model file into the Rust binary. Produces a single self-contained executable but increases binary size by the model file size (80--130 MB). Not recommended for most models.

2. **Ship alongside binary:** Place model files in a `models/` directory next to the binary. Suitable for package managers (Homebrew, apt, Flatpak) and Docker images.

3. **Auto-download on first run (recommended):** On first launch, download model files from a known URL (Hugging Face Hub) to a local cache directory (`~/.emailibrium/models/`). Verify integrity via SHA-256 checksum. This is exactly what `fastembed` does by default.

The `fastembed` crate implements strategy 3 natively, downloading models from Hugging Face Hub on first use and caching them locally. This aligns perfectly with Emailibrium's design goals.

---

## 4. Embedding Model Catalog

### 4.1 Comparison Table

The following table catalogs ONNX-compatible embedding models suitable for Emailibrium, ordered by the trade-off between quality, size, and performance.

| Model                      | Params | Dims    | ONNX Size (FP32) | ONNX Size (INT8/Q) | MTEB Avg | Max Tokens | Languages | fastembed Support |
| -------------------------- | ------ | ------- | ---------------- | ------------------ | -------- | ---------- | --------- | ----------------- |
| `all-MiniLM-L6-v2`         | 22M    | 384     | ~90 MB           | ~23 MB             | ~56      | 256        | English   | Yes               |
| `all-MiniLM-L12-v2`        | 33M    | 384     | ~130 MB          | ~33 MB             | ~57      | 256        | English   | Yes               |
| `bge-small-en-v1.5`        | 33M    | 384     | ~127 MB          | ~32 MB             | ~59      | 512        | English   | Yes (default)     |
| `bge-base-en-v1.5`         | 109M   | 768     | ~420 MB          | ~105 MB            | ~61      | 512        | English   | Yes               |
| `bge-m3`                   | 568M   | 1024    | ~2.2 GB          | ~550 MB            | ~63      | 8192       | 100+      | Yes               |
| `snowflake-arctic-embed-s` | 22M    | 384     | ~90 MB           | ~23 MB             | ~57      | 512        | English   | Yes               |
| `nomic-embed-text-v1.5`    | 137M   | 64--768 | ~520 MB          | ~130 MB            | ~62      | 8192       | English   | Yes               |
| `e5-small-v2`              | 33M    | 384     | ~127 MB          | ~32 MB             | ~57      | 512        | English   | Partial           |
| `multilingual-e5-small`    | 118M   | 384     | ~449 MB          | ~113 MB            | ~58      | 512        | 100+      | Yes               |
| `multilingual-e5-large`    | 560M   | 1024    | ~2.1 GB          | ~530 MB            | ~62      | 512        | 100+      | Yes               |
| `mxbai-embed-large-v1`     | 335M   | 1024    | ~1.3 GB          | ~335 MB            | ~64      | 512        | English   | Yes               |
| `gte-base-en-v1.5`         | 109M   | 768     | ~420 MB          | ~105 MB            | ~62      | 8192       | English   | Yes               |
| `CLIP ViT-B-32` (text)     | 63M    | 512     | ~240 MB          | ~60 MB             | N/A      | 77         | English   | Yes (text only)   |
| `CLIP ViT-B-32` (vision)   | 88M    | 512     | ~340 MB          | ~85 MB             | N/A      | N/A        | N/A       | Separate model    |

**Notes on MTEB scores:** Scores are approximate averages across the MTEB English benchmark suite (retrieval, classification, clustering, STS, pair classification, reranking, summarization). Higher is better. For comparison, commercial models score higher: Cohere embed-v4 (~65), OpenAI text-embedding-3-large (~64.6), Qwen3-Embedding-8B (~70.6).

### 4.2 Recommended Default: `all-MiniLM-L6-v2`

For Emailibrium's default (Tier 0) configuration, `all-MiniLM-L6-v2` is the recommended model:

- **Size:** 22M parameters, ~90 MB ONNX FP32, ~23 MB quantized INT8
- **Dimensions:** 384 (matches the current `MockEmbeddingModel` default and HNSW index configuration)
- **Performance:** 40--100+ sentences/second on CPU (hardware dependent), sub-50ms single-sentence latency
- **Quality:** MTEB ~56 average. Sufficient for email classification (subject + sender + body snippet). The model was specifically trained for semantic similarity and retrieval tasks
- **Context length:** 256 tokens. Adequate for Emailibrium's `prepare_email_text()` which truncates body to 400 characters (~80--100 tokens)
- **Ecosystem:** The most widely deployed small embedding model globally. Battle-tested in production at scale by Qdrant, Pinecone, Weaviate, and others

**Why not `bge-small-en-v1.5`?** It scores slightly higher on MTEB (~59 vs ~56) and supports 512 tokens, but is 50% larger (33M vs 22M params). For email triage where the input is a short concatenation of subject + sender + body snippet, the quality difference is marginal. `all-MiniLM-L6-v2` offers the better size/quality trade-off for the zero-config default. Users who want higher quality can switch to `bge-small-en-v1.5` or `bge-base-en-v1.5` via configuration.

### 4.3 Recommended Multilingual Option: `multilingual-e5-small`

For users processing non-English email, `multilingual-e5-small` (118M params, 384 dims, ~113 MB quantized) covers 100+ languages with the same dimensionality as the default model, allowing a drop-in swap without re-indexing.

### 4.4 Image Embeddings: CLIP ViT-B-32

For the planned image embedding feature (email attachment thumbnails, inline images):

- **Text encoder:** `Qdrant/clip-ViT-B-32-text` -- supported by `fastembed`, produces 512-dimensional embeddings
- **Vision encoder:** `Qdrant/clip-ViT-B-32-vision` -- available as a separate ONNX model, also 512 dimensions
- **Input:** Images are preprocessed to 224x224x3 (RGB) tensors
- **Cross-modal search:** Text and image embeddings share the same 512-dimensional space, enabling "find emails with images similar to this description" queries

CLIP integration will require a separate `ImageEmbeddingModel` trait implementation. The `fastembed` crate supports the text half; the vision encoder will require direct `ort` usage or the `embed_anything` crate.

### 4.5 ONNX Model Export Process

For models not already available in ONNX format, the Hugging Face `optimum` library provides export tooling:

```bash
pip install optimum[onnx]
optimum-cli export onnx --model sentence-transformers/all-MiniLM-L6-v2 ./minilm-onnx/
```

This exports the model with tokenizer configuration. Dynamic quantization (INT8) can be applied without a calibration dataset:

```bash
optimum-cli export onnx --model sentence-transformers/all-MiniLM-L6-v2 \
  --task feature-extraction \
  --optimize O2 \
  ./minilm-onnx-optimized/
```

For Emailibrium's purposes, pre-exported ONNX models from Hugging Face Hub (used by `fastembed`) eliminate the need for this step entirely.

---

## 5. fastembed Analysis

### 5.1 Overview

`fastembed` is a Rust crate purpose-built for fast, lightweight embedding generation using ONNX Runtime. It is maintained by Anush008 (Qdrant contributor) and serves as the Rust counterpart to the Python `fastembed` library by Qdrant.

| Property       | Value                                             |
| -------------- | ------------------------------------------------- |
| Crate          | `fastembed`                                       |
| Latest version | 5.12.0                                            |
| License        | Apache-2.0                                        |
| Repository     | https://github.com/Anush008/fastembed-rs          |
| Dependencies   | `ort` (ONNX Runtime), `tokenizers` (Hugging Face) |
| Async support  | Synchronous (no Tokio dependency)                 |

### 5.2 Key Features

1. **Automatic model management:** Models are downloaded from Hugging Face Hub on first use and cached in `~/.cache/fastembed/` (configurable). Download progress can be displayed.

2. **Built-in tokenization:** Uses the `tokenizers` crate (Hugging Face's Rust tokenizer library) for fast, correct tokenization. No external tokenizer service needed.

3. **Quantized model support:** Many models have quantized variants (append `Q` to the enum, e.g., `EmbeddingModel::BGESmallENV15Q`). These use INT8 quantization for ~4x smaller files and faster inference with minimal quality loss.

4. **ONNX Runtime backend:** Leverages `ort` for hardware-accelerated inference. CPU by default; GPU acceleration available when `ort` is configured with the appropriate execution provider.

5. **Batch processing:** Supports configurable batch sizes for efficient bulk embedding.

6. **No async runtime required:** The API is synchronous, avoiding Tokio dependency conflicts. This is important for Emailibrium since the embedding calls can be wrapped in `tokio::task::spawn_blocking()`.

### 5.3 Supported Models (Text Embedding)

As of version 5.12.0, `fastembed` supports the following text embedding models via the `EmbeddingModel` enum:

- `AllMiniLML6V2` / `AllMiniLML6V2Q` -- sentence-transformers/all-MiniLM-L6-v2
- `AllMiniLML12V2` / `AllMiniLML12V2Q` -- sentence-transformers/all-MiniLM-L12-v2
- `AllMpnetBaseV2` -- sentence-transformers/all-mpnet-base-v2
- `BGESmallENV15` / `BGESmallENV15Q` -- BAAI/bge-small-en-v1.5 **(default)**
- `BGEBaseENV15` / `BGEBaseENV15Q` -- BAAI/bge-base-en-v1.5
- `BGELargeENV15` / `BGELargeENV15Q` -- BAAI/bge-large-en-v1.5
- `BGESmallZHV15` -- BAAI/bge-small-zh-v1.5
- `BGELargeZHV15` -- BAAI/bge-large-zh-v1.5
- `BGEM3` -- BAAI/bge-m3
- `NomicEmbedTextV1` -- nomic-ai/nomic-embed-text-v1
- `NomicEmbedTextV15` -- nomic-ai/nomic-embed-text-v1.5
- `ParaphraseMLMiniLML12V2` / `ParaphraseMLMpnetBaseV2` -- multilingual paraphrase models
- `MultilingualE5Small` / `MultilingualE5Base` / `MultilingualE5Large` -- intfloat/multilingual-e5-\*
- `MxbaiEmbedLargeV1` -- mixedbread-ai/mxbai-embed-large-v1
- `GTEBaseENV15` / `GTELargeENV15` -- Alibaba-NLP/gte-\*-en-v1.5
- `ModernBERTEmbedLarge` -- lightonai/ModernBERT-embed-large
- `CLIPViTB32Text` -- Qdrant/clip-ViT-B-32-text (text-side CLIP embeddings)
- Snowflake Arctic Embed variants

### 5.4 Code Example: Emailibrium Integration

The following demonstrates how a new `OnnxEmbeddingModel` implementation would use `fastembed`:

```rust
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub struct OnnxEmbeddingModel {
    model: TextEmbedding,
    model_name: String,
    dims: usize,
}

impl OnnxEmbeddingModel {
    pub fn new(
        model_enum: EmbeddingModel,
        cache_dir: Option<String>,
        show_download: bool,
    ) -> Result<Self, VectorError> {
        let mut options = InitOptions::new(model_enum.clone())
            .with_show_download_progress(show_download);

        if let Some(dir) = &cache_dir {
            options = options.with_cache_dir(dir.into());
        }

        let model = TextEmbedding::try_new(options)
            .map_err(|e| VectorError::EmbeddingFailed(
                format!("Failed to initialize ONNX model: {e}")
            ))?;

        // Dimensions are determined by the model choice
        let dims = match model_enum {
            EmbeddingModel::AllMiniLML6V2
            | EmbeddingModel::AllMiniLML6V2Q
            | EmbeddingModel::BGESmallENV15
            | EmbeddingModel::BGESmallENV15Q => 384,
            EmbeddingModel::BGEBaseENV15
            | EmbeddingModel::BGEBaseENV15Q => 768,
            _ => 384, // safe default
        };

        Ok(Self {
            model,
            model_name: format!("{:?}", model_enum),
            dims,
        })
    }
}

#[async_trait]
impl crate::vectors::embedding::EmbeddingModel for OnnxEmbeddingModel {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        let text = text.to_string();
        let model = self.model.clone(); // fastembed TextEmbedding is Clone
        tokio::task::spawn_blocking(move || {
            let results = model.embed(vec![text], None)
                .map_err(|e| VectorError::EmbeddingFailed(e.to_string()))?;
            results.into_iter().next()
                .ok_or(VectorError::EmbeddingFailed("empty result".into()))
        })
        .await
        .map_err(|e| VectorError::EmbeddingFailed(format!("join error: {e}")))?
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let texts = texts.to_vec();
        let model = self.model.clone();
        tokio::task::spawn_blocking(move || {
            model.embed(texts, None)
                .map_err(|e| VectorError::EmbeddingFailed(e.to_string()))
        })
        .await
        .map_err(|e| VectorError::EmbeddingFailed(format!("join error: {e}")))?
    }

    fn dimensions(&self) -> usize { self.dims }
    fn model_name(&self) -> &str { &self.model_name }
    async fn is_available(&self) -> bool { true } // always available once initialized
}
```

### 5.5 Performance Characteristics

Based on community benchmarks and the `ort` documentation:

| Metric                             | Value (CPU, Apple M-series)   | Value (CPU, Intel i7) |
| ---------------------------------- | ----------------------------- | --------------------- |
| Single sentence latency            | 5--15 ms                      | 15--40 ms             |
| Batch (32 sentences)               | 50--120 ms                    | 150--400 ms           |
| Throughput                         | 200--600 sentences/sec        | 80--200 sentences/sec |
| Model load time (first run)        | 200--500 ms                   | 300--800 ms           |
| Model download (first ever)        | 5--30 sec (network dependent) | 5--30 sec             |
| Memory (runtime, all-MiniLM-L6-v2) | ~150--250 MB RSS              | ~150--250 MB RSS      |

These figures represent the `all-MiniLM-L6-v2` model. Larger models (e.g., `bge-base-en-v1.5` at 109M params) will have proportionally higher latency and memory usage.

**Comparison to Ollama for embeddings:**

| Metric                         | fastembed (ONNX)     | Ollama (nomic-embed-text)           |
| ------------------------------ | -------------------- | ----------------------------------- |
| First-run setup                | Auto-download ~90 MB | Install Ollama + pull ~274 MB       |
| Single sentence latency        | 5--40 ms             | 50--200 ms (HTTP overhead)          |
| Batch throughput               | 200--600 sent/sec    | 50--150 sent/sec                    |
| Memory footprint               | ~150--250 MB         | ~500--800 MB (Ollama process)       |
| External dependency            | None (in-process)    | Ollama daemon on localhost          |
| Network calls during inference | Zero                 | Zero (local, but HTTP to localhost) |

### 5.6 Alternative: `embed_anything`

The `embed_anything` crate (by StarlightSearch) is a more feature-rich alternative that supports both ONNX and Candle backends, multimodal embeddings (text, image, audio, PDF), and streaming to vector databases. It could replace `fastembed` if multimodal support becomes a priority, but its larger dependency tree and complexity make `fastembed` the better choice for Emailibrium's focused text embedding needs.

---

## 6. Generative Model Options

### 6.1 Classification Without Generative Models

An important insight for Emailibrium: **most email classification does not require a generative model at all.** The current design uses vector centroid matching with a 0.7 confidence threshold. Based on typical email classification workloads:

- **85--95% of emails** can be classified by cosine similarity to category centroids alone (no LLM needed)
- **5--15% of ambiguous emails** (confidence 0.5--0.7) benefit from a generative model as a tie-breaker
- **<1% of truly novel emails** (no close centroid) require generative classification from scratch

For the default (Tier 0) configuration, Emailibrium should handle the ambiguous cases with a **fallback heuristic** (e.g., weighted keyword matching, sender domain rules) rather than requiring a generative model. This keeps the default zero-dependency.

### 6.2 Small Generative Models for Tier 1

When users opt into Tier 1 (Ollama), the following models are suitable for email classification and light chat:

| Model            | Params | RAM (Q4) | Classification Quality | Chat Quality | Format    |
| ---------------- | ------ | -------- | ---------------------- | ------------ | --------- |
| `gemma-3-1b`     | 1B     | ~1.5 GB  | Good                   | Basic        | GGUF      |
| `TinyLlama-1.1B` | 1.1B   | ~1.5 GB  | Good                   | Basic        | GGUF      |
| `SmolLM2-1.7B`   | 1.7B   | ~2 GB    | Good                   | Moderate     | GGUF      |
| `Phi-3.5-mini`   | 3.8B   | ~3 GB    | Excellent              | Good         | GGUF/ONNX |
| `Qwen2.5-3B`     | 3B     | ~2.5 GB  | Excellent              | Good         | GGUF      |
| `llama3.2:3b`    | 3.2B   | ~2.5 GB  | Excellent              | Good         | GGUF      |
| `Gemma-3-4B`     | 4B     | ~3 GB    | Excellent              | Good         | GGUF      |

**Recommended for Ollama Tier 1:** `llama3.2:3b` or `Phi-3.5-mini`. Both excel at instruction-following tasks like "Classify this email as one of: [promotions, updates, personal, finance, travel]" and require only 2.5--3 GB RAM.

**Ultra-small option for classification only:** `Gemma-3-270M` (529 MB) handles basic text classification and entity extraction on hardware as minimal as a Raspberry Pi 5. This could be a future option for an embedded GGUF/ONNX generative model that ships with the binary.

### 6.3 ONNX vs. GGUF for Generative Models

| Aspect                            | ONNX (via `ort`)                 | GGUF (via `llama-cpp-rs`)             |
| --------------------------------- | -------------------------------- | ------------------------------------- |
| Ecosystem maturity for generative | Moderate (Phi-3 official ONNX)   | Excellent (most models available)     |
| Quantization options              | INT4, INT8                       | Q2_K through Q8_0, many variants      |
| Rust crate quality                | `ort` (excellent)                | `llama-cpp-rs` / `llama-cpp-2` (good) |
| CPU performance                   | Good                             | Excellent (highly optimized)          |
| Apple Silicon Metal               | Via CoreML EP                    | Native Metal support                  |
| Model availability                | Limited (Phi-3, some BERT-class) | Nearly all open models                |
| Memory efficiency                 | Moderate                         | Excellent (memory-mapped)             |

**Recommendation:** For generative models, GGUF via `llama-cpp-rs` is superior to ONNX. The llama.cpp ecosystem has far more model availability, better quantization options, and more optimized CPU inference. However, this is only relevant for Tier 1+ (Ollama handles GGUF natively). For the Tier 0 default, no generative model is needed.

### 6.4 The `candle` Crate (Hugging Face Rust ML)

Candle is Hugging Face's minimalist ML framework for Rust, now at version 0.9.2. It supports:

- **Generative models:** LLaMA (v1/v2/v3), Phi (1/1.5/2/3), Gemma (v1/v2), Falcon, StarCoder, Qwen, Mistral
- **Embedding models:** BERT, Sentence-BERT
- **Vision models:** Stable Diffusion, CLIP
- **Quantization:** GGUF and GGML format support
- **Hardware:** CPU, CUDA, Metal

Candle eliminates the ONNX Runtime C dependency entirely (pure Rust), but has slower inference for transformer models compared to `ort`. It is a viable path for future generative model integration without Ollama, especially the `fastembed` crate's optional `qwen3` feature flag which uses Candle as a backend for Qwen3 embedding models.

---

## 7. Tiered Architecture Proposal

### 7.1 Overview

```text
+-----------------------------------------------------------------------+
|                         Emailibrium AI Layer                          |
+-----------------------------------------------------------------------+
|                                                                       |
|  Tier 0 (Default, Zero Config)          Tier 1 (Local Enhanced)       |
|  +---------------------------------+    +---------------------------+ |
|  | Embedding: fastembed (ONNX)     |    | Embedding: fastembed      | |
|  |   all-MiniLM-L6-v2             |    |   (still ONNX, faster)    | |
|  |   ~90 MB, auto-downloads       |    |                           | |
|  |                                 |    | Generative: Ollama        | |
|  | Classification: centroid only   |    |   llama3.2:3b / phi-3.5   | |
|  |   + keyword/rule fallback       |    |   for classification      | |
|  |                                 |    |   fallback + chat         | |
|  | Chat: disabled / template-based |    |                           | |
|  |                                 |    | Image: CLIP via ONNX      | |
|  | Image: deferred                 |    |   + Ollama vision models  | |
|  +---------------------------------+    +---------------------------+ |
|                                                                       |
|  Tier 2 (Cloud Opt-in)                                                |
|  +---------------------------------+                                  |
|  | Embedding: OpenAI / Cohere      |                                  |
|  |   text-embedding-3-small        |                                  |
|  |   or embed-v4                   |                                  |
|  |                                 |                                  |
|  | Generative: Claude / GPT-4o     |                                  |
|  |   for classification + chat     |                                  |
|  |                                 |                                  |
|  | REQUIRES: explicit consent      |                                  |
|  |   + API key via env var         |                                  |
|  +---------------------------------+                                  |
+-----------------------------------------------------------------------+
```

### 7.2 Tier 0: Default (Zero Configuration)

**Target user:** Anyone who installs Emailibrium. No AI knowledge required.

| Component             | Implementation                      | Details                                                              |
| --------------------- | ----------------------------------- | -------------------------------------------------------------------- |
| Text embedding        | `fastembed` with `all-MiniLM-L6-v2` | 384D, ~90 MB ONNX, auto-downloads to `~/.emailibrium/models/`        |
| Classification        | Vector centroid matching            | Cosine similarity to learned category centroids                      |
| Ambiguous fallback    | Rule-based heuristic                | Sender domain, keyword matching, header analysis                     |
| Chat                  | Disabled                            | "Chat features require Tier 1 or higher. Enable Ollama in settings." |
| Image embedding       | Deferred                            | "Image search requires Tier 1 or higher."                            |
| Network calls         | Zero                                | All inference runs in-process                                        |
| External dependencies | Zero                                | No Ollama, no cloud APIs, no Docker                                  |

### 7.3 Tier 1: Local Enhanced (Ollama)

**Target user:** Power users who want generative AI features while keeping all data local.

| Component             | Implementation                         | Details                                                     |
| --------------------- | -------------------------------------- | ----------------------------------------------------------- |
| Text embedding        | `fastembed` (ONNX)                     | Same as Tier 0 -- ONNX is faster than Ollama for embeddings |
| Classification        | Vector centroid + Ollama fallback      | Ollama called only when centroid confidence < 0.7           |
| Chat                  | Ollama generative model                | User's choice of model (default: `llama3.2:3b`)             |
| Image embedding       | CLIP ViT-B-32 via ONNX + Ollama vision | ONNX for CLIP, Ollama for advanced vision models            |
| Network calls         | Zero (all localhost)                   | Ollama runs as a local process                              |
| External dependencies | Ollama installed and running           | User must `ollama pull` the desired model                   |

### 7.4 Tier 2: Cloud Opt-in

**Target user:** Users who want maximum quality and accept the privacy trade-off.

| Component             | Implementation                        | Details                                                               |
| --------------------- | ------------------------------------- | --------------------------------------------------------------------- |
| Text embedding        | Cloud API (OpenAI, Cohere)            | Higher quality, but data leaves machine                               |
| Classification        | Cloud LLM (Claude Haiku, GPT-4o-mini) | Best accuracy for ambiguous emails                                    |
| Chat                  | Cloud LLM (Claude Sonnet, GPT-4o)     | Full conversational AI capabilities                                   |
| Network calls         | Per-inference API calls               | All email text sent to cloud provider                                 |
| External dependencies | API key required                      | Set via env var, never stored in config file                          |
| Consent               | **Mandatory**                         | User must explicitly acknowledge data sharing before first cloud call |

---

## 8. Configuration Design

### 8.1 Complete YAML Schema

```yaml
# ~/.emailibrium/config.yaml (or /etc/emailibrium/config.yaml)

ai:
  # --- Embedding Configuration ---
  embedding:
    # Provider selection: "onnx" (default) | "ollama" | "cloud"
    provider: 'onnx'

    # ONNX provider settings (used when provider = "onnx")
    onnx:
      # Model identifier (fastembed EmbeddingModel enum variant)
      # Options: "all-MiniLM-L6-v2", "all-MiniLM-L6-v2-q", "bge-small-en-v1.5",
      #          "bge-small-en-v1.5-q", "bge-base-en-v1.5", "multilingual-e5-small",
      #          "nomic-embed-text-v1.5", "snowflake-arctic-embed-s"
      model: 'all-MiniLM-L6-v2'

      # Local cache directory for downloaded model files
      # Models are auto-downloaded from Hugging Face on first run
      model_path: '~/.emailibrium/models/'

      # Show download progress bar on first model download
      show_download_progress: true

      # Use GPU acceleration (CoreML on macOS, CUDA on Linux/Windows)
      # false = CPU only (default, works everywhere)
      use_gpu: false

      # Number of threads for ONNX Runtime intra-op parallelism
      # 0 = auto-detect (recommended)
      num_threads: 0

    # Ollama provider settings (used when provider = "ollama")
    ollama:
      url: 'http://localhost:11434'
      model: 'nomic-embed-text'
      # Dimensions must match the Ollama model's output
      dimensions: 768

    # Cloud provider settings (used when provider = "cloud")
    cloud:
      # Cloud provider: "openai" | "cohere" | "voyage"
      provider: 'openai'
      # API key is NEVER stored in this file. Use environment variable.
      api_key_env: 'OPENAI_API_KEY'
      model: 'text-embedding-3-small'
      dimensions: 1536
      # Maximum requests per minute (rate limiting)
      rate_limit_rpm: 500

  # --- Generative Model Configuration ---
  generative:
    # Provider selection: "none" (default) | "ollama" | "cloud"
    # "none" = classification uses centroid-only + rule-based fallback
    provider: 'none'

    # Ollama generative settings (used when provider = "ollama")
    ollama:
      url: 'http://localhost:11434'
      # Model for classification fallback
      classification_model: 'llama3.2:3b'
      # Model for chat/conversational AI (can be same or different)
      chat_model: 'llama3.2:3b'
      # Maximum tokens for classification responses
      classification_max_tokens: 50
      # Maximum tokens for chat responses
      chat_max_tokens: 1024
      # Temperature for classification (low = deterministic)
      classification_temperature: 0.1
      # Temperature for chat (higher = creative)
      chat_temperature: 0.7

    # Cloud generative settings (used when provider = "cloud")
    cloud:
      # Cloud provider: "anthropic" | "openai" | "google"
      provider: 'anthropic'
      api_key_env: 'ANTHROPIC_API_KEY'
      classification_model: 'claude-haiku-4-5-20251001'
      chat_model: 'claude-sonnet-4-6'
      # Maximum requests per minute
      rate_limit_rpm: 60

  # --- Image Embedding Configuration ---
  image_embedding:
    # Enable image embedding: true | false
    enabled: false
    # Provider: "onnx" | "ollama"
    provider: 'onnx'
    onnx:
      # CLIP model for image embeddings
      model: 'clip-vit-b-32'
      model_path: '~/.emailibrium/models/'
      use_gpu: false

  # --- Classification Tuning ---
  classification:
    # Minimum cosine similarity for centroid-based classification
    confidence_threshold: 0.7
    # Below this threshold, email is marked "uncategorized" even with LLM fallback
    minimum_threshold: 0.3
    # Maximum number of categories to consider
    max_candidates: 5
    # Enable generative fallback when confidence < confidence_threshold
    use_generative_fallback: true

  # --- Privacy & Consent ---
  consent:
    # Show a consent dialog before first cloud API call
    require_cloud_consent: true
    # Log all cloud API calls (request metadata, not content) to audit table
    audit_cloud_calls: true
    # Display warning when switching from local to cloud provider
    show_cloud_data_warning: true
    # Allow telemetry (anonymous usage stats) -- always default to false
    allow_telemetry: false

  # --- Caching ---
  cache:
    # Maximum number of embeddings to cache in memory
    embedding_cache_size: 10000
    # Minimum word count to skip query augmentation
    min_query_tokens: 5

  # --- Model Integrity ---
  integrity:
    # Verify SHA-256 checksums of downloaded model files
    verify_checksums: true
    # Path to checksum manifest file
    checksum_file: '~/.emailibrium/models/checksums.sha256'
```

### 8.2 Environment Variable Overrides

Every YAML key can be overridden via environment variable using the `EMAILIBRIUM_` prefix with double-underscore nesting:

```bash
# Override embedding provider
export EMAILIBRIUM_AI__EMBEDDING__PROVIDER="ollama"

# Override Ollama URL
export EMAILIBRIUM_AI__EMBEDDING__OLLAMA__URL="http://192.168.1.100:11434"

# Set API key (always via env var, never in YAML)
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
```

### 8.3 Provider Resolution Logic

```text
1. Read ai.embedding.provider from config (default: "onnx")
2. If "onnx":
   a. Check if model files exist in model_path
   b. If not, download from Hugging Face Hub (with progress bar)
   c. Verify SHA-256 checksum
   d. Initialize fastembed TextEmbedding
   e. Return OnnxEmbeddingModel
3. If "ollama":
   a. Check if Ollama is reachable at configured URL
   b. If not, log warning and fall back to ONNX (graceful degradation)
   c. Return OllamaEmbeddingModel
4. If "cloud":
   a. Check if consent has been granted (consent.require_cloud_consent)
   b. Check if API key env var is set
   c. If either fails, log error and fall back to ONNX
   d. Return CloudEmbeddingModel
```

---

## 9. Privacy and Security Analysis

### 9.1 Comparative Privacy Posture

| Aspect                          | Tier 0 (ONNX)                                | Tier 1 (Ollama)              | Tier 2 (Cloud)             |
| ------------------------------- | -------------------------------------------- | ---------------------------- | -------------------------- |
| Data leaves process             | No                                           | No (localhost HTTP)          | **Yes**                    |
| Data leaves machine             | No                                           | No                           | **Yes**                    |
| Network calls during inference  | Zero                                         | Zero                         | Per-request                |
| Third-party can observe content | No                                           | No                           | **Yes (provider)**         |
| Model telemetry                 | None (see 9.2)                               | None (Ollama is open source) | Varies by provider         |
| Attack surface                  | Model file integrity                         | Ollama HTTP API on localhost | Internet-facing API calls  |
| Offline operation               | Full                                         | Full                         | None                       |
| GDPR compliance                 | Inherent (no data processing by third party) | Inherent                     | Requires DPA with provider |

### 9.2 ONNX Runtime Telemetry

ONNX Runtime's telemetry behavior depends on the build:

- **Windows official builds:** Telemetry is ON by default, using the Windows TraceLogging API. It can be disabled via `DisableTelemetryEvents()` in the C API.
- **Linux and macOS:** **No telemetry is implemented.** There is no telemetry code in non-Windows builds.
- **Builds from source:** No telemetry regardless of platform.
- **The `ort` crate:** Downloads pre-built ONNX Runtime binaries. On macOS and Linux, these have zero telemetry. On Windows, `ort` should call `DisableTelemetryEvents()` during session initialization.

**Recommendation:** Emailibrium should explicitly call `DisableTelemetryEvents()` on Windows during `ort` session initialization, and document in the privacy policy that no telemetry is collected. On macOS and Linux (the primary targets), this is a non-issue.

### 9.3 Model File Integrity

ONNX model files are static binary artifacts. They should be treated as untrusted input until verified:

1. **SHA-256 checksums:** Maintain a `checksums.sha256` manifest file (shipped with Emailibrium or hosted at a known URL). Verify every model file against this manifest before loading.
2. **Source pinning:** Only download models from specific Hugging Face Hub repositories (e.g., `sentence-transformers/all-MiniLM-L6-v2`, `Qdrant/clip-ViT-B-32-vision`). Reject models from unknown sources.
3. **No code execution:** ONNX models are declarative computation graphs. Unlike PyTorch models (which can contain arbitrary Python code via pickle), ONNX models cannot execute arbitrary code. This is a significant security advantage.
4. **Model sandboxing:** ONNX Runtime uses bounded memory allocation. A malformed model file cannot cause unbounded memory allocation or buffer overflows (assuming ONNX Runtime itself is free of vulnerabilities).

### 9.4 Cloud Provider Risk Mitigation

For Tier 2, Emailibrium should implement:

1. **Explicit consent gate:** A dialog/prompt that clearly states: "Your email content will be sent to [provider] for processing. This data will leave your machine."
2. **Audit logging:** Every cloud API call is logged to an audit table with timestamp, provider, model, token count, and a truncated hash of the input (not the full content).
3. **Content minimization:** Send only the minimum text needed (subject + sender + truncated body), never full email bodies or attachment contents, to cloud APIs.
4. **API key hygiene:** API keys are only read from environment variables, never stored in YAML or SQLite.

---

## 10. Implementation Roadmap

### 10.1 Phase 1: ONNX Embedding (Tier 0 Default)

**Estimated effort:** 3--5 days

1. **Add `fastembed` dependency to `backend/Cargo.toml`:**

   ```toml
   [dependencies]
   fastembed = "5.12"
   ```

2. **Implement `OnnxEmbeddingModel`** in `backend/src/vectors/embedding.rs`:
   - New struct wrapping `fastembed::TextEmbedding`
   - Implement the existing `EmbeddingModel` trait
   - Use `tokio::task::spawn_blocking()` for the synchronous `fastembed` API
   - Handle model download on first run with progress reporting

3. **Update `EmbeddingConfig`** in `backend/src/vectors/config.rs`:
   - Add `onnx` section with model selection, cache path, GPU toggle
   - Change default provider from `"mock"` to `"onnx"`

4. **Update `EmbeddingPipeline::new()`** to handle `"onnx"` provider:
   - Add match arm for `"onnx"` that constructs `OnnxEmbeddingModel`
   - Implement graceful error handling if model download fails

5. **Add integration tests:**
   - Test ONNX model initialization and embedding generation
   - Test dimension consistency (384 for all-MiniLM-L6-v2)
   - Test batch embedding
   - Test model caching (second init should not re-download)

6. **Update YAML config** with ONNX section and new defaults.

### 10.2 Phase 2: Configuration and Provider Switching

**Estimated effort:** 2--3 days

1. Implement the full YAML configuration schema from Section 8.
2. Add environment variable override support.
3. Implement provider resolution logic with graceful fallback.
4. Add a CLI command: `emailibrium config show-ai` to display current AI configuration.
5. Add a CLI command: `emailibrium models list` to show available/downloaded models.

### 10.3 Phase 3: Generative Fallback (Tier 1)

**Estimated effort:** 3--5 days

1. Define a `GenerativeModel` trait (similar to `EmbeddingModel`).
2. Implement `OllamaGenerativeModel` using the existing Ollama HTTP client pattern.
3. Integrate with the classification pipeline: call generative model when centroid confidence is below threshold.
4. Implement the chat API endpoint backed by Ollama.

### 10.4 Phase 4: Cloud Providers (Tier 2)

**Estimated effort:** 5--7 days

1. Implement `CloudEmbeddingModel` for OpenAI and Cohere.
2. Implement `CloudGenerativeModel` for Anthropic and OpenAI.
3. Implement consent gate and audit logging.
4. Add rate limiting and retry logic.

### 10.5 Phase 5: Image Embeddings

**Estimated effort:** 3--5 days

1. Define `ImageEmbeddingModel` trait.
2. Implement CLIP ViT-B-32 vision encoder via direct `ort` usage.
3. Image preprocessing pipeline (resize, normalize, tensor conversion).
4. Cross-modal search integration.

---

## 11. References

### Crates and Libraries

- [ort crate on crates.io](https://crates.io/crates/ort) -- Rust bindings for ONNX Runtime
- [ort documentation](https://ort.pyke.io/) -- Official ort documentation
- [ort GitHub repository](https://github.com/pykeio/ort) -- Source code and issues
- [fastembed crate on crates.io](https://crates.io/crates/fastembed) -- Rust embedding library
- [fastembed-rs GitHub repository](https://github.com/Anush008/fastembed-rs) -- Source code, model list, examples
- [fastembed API documentation](https://docs.rs/fastembed/latest/fastembed/) -- Rust API docs
- [embed_anything crate](https://crates.io/crates/embed_anything) -- Multimodal embedding library
- [EmbedAnything GitHub](https://github.com/StarlightSearch/EmbedAnything) -- Source and documentation
- [candle GitHub repository](https://github.com/huggingface/candle) -- Hugging Face Rust ML framework
- [llama-cpp-rs GitHub](https://github.com/edgenai/llama_cpp-rs) -- Rust bindings for llama.cpp
- [llama-cpp-2 crate](https://github.com/utilityai/llama-cpp-rs) -- Alternative Rust bindings

### Models

- [sentence-transformers/all-MiniLM-L6-v2 on Hugging Face](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) -- Default embedding model
- [BAAI/bge-small-en-v1.5 on Hugging Face](https://huggingface.co/BAAI/bge-small-en-v1.5) -- Alternative embedding model
- [nomic-ai/nomic-embed-text-v1.5 on Hugging Face](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5) -- Long-context embedding model
- [intfloat/multilingual-e5-large on Hugging Face](https://huggingface.co/intfloat/multilingual-e5-large) -- Multilingual embedding model
- [Qdrant/clip-ViT-B-32-vision on Hugging Face](https://huggingface.co/Qdrant/clip-ViT-B-32-vision) -- CLIP vision encoder
- [microsoft/Phi-3-mini-4k-instruct-onnx on Hugging Face](https://huggingface.co/microsoft/Phi-3-mini-4k-instruct-onnx) -- Small generative model in ONNX
- [microsoft/Phi-3.5-mini-instruct-onnx on Hugging Face](https://huggingface.co/microsoft/Phi-3.5-mini-instruct-onnx) -- Updated Phi-3.5 ONNX

### Benchmarks and Leaderboards

- [MTEB Leaderboard on Hugging Face](https://huggingface.co/spaces/mteb/leaderboard) -- Massive Text Embedding Benchmark
- [MTEB GitHub repository](https://github.com/embeddings-benchmark/mteb) -- Benchmark source code and methodology
- [Best Embedding Models 2025: MTEB Scores](https://app.ailog.fr/en/blog/guides/choosing-embedding-models) -- Comparative guide

### ONNX Runtime

- [ONNX Runtime official site](https://onnxruntime.ai/) -- Microsoft's ONNX Runtime
- [ONNX Runtime Privacy documentation](https://github.com/microsoft/onnxruntime/blob/main/docs/Privacy.md) -- Telemetry and data collection policies
- [ONNX Runtime Phi-3 acceleration blog](https://onnxruntime.ai/blogs/accelerating-phi-3) -- Performance benchmarks for Phi-3 on ONNX

### Tooling

- [Hugging Face Optimum ONNX export](https://huggingface.co/docs/optimum-onnx/en/onnx/usage_guides/export_a_model) -- Model export to ONNX format
- [Sentence Transformers ONNX efficiency guide](https://sbert.net/docs/sentence_transformer/usage/efficiency.html) -- Optimization techniques
- [Bundling ONNX Runtime in Rust with Nix](https://blog.stark.pub/posts/bundling-onnxruntime-rust-nix/) -- Practical deployment guide

### Small Language Models

- [Best Small Language Models for 2026 (DataCamp)](https://www.datacamp.com/blog/top-small-language-models) -- Comprehensive survey
- [Small Language Models Guide 2026 (LocalAI Master)](https://localaimaster.com/blog/small-language-models-guide-2026) -- RAM requirements and benchmarks
- [10 AI Models Under 7B (LocalAI Master)](https://localaimaster.com/blog/top-lightweight-models) -- Laptop-friendly models
- [Best Sub-3B GGUF Models Guide](https://ggufloader.github.io/2025-07-07-top-10-gguf-models-i5-16gb/) -- CPU-optimized models

### Community and Tutorials

- [Local Embeddings with Fastembed, Rig and Rust (DEV Community)](https://dev.to/joshmo_dev/local-embeddings-with-fastembed-rig-rust-3581) -- Practical tutorial
- [Building Sentence Transformers in Rust (DEV Community)](https://dev.to/mayu2008/building-sentence-transformers-in-rust-a-practical-guide-with-burn-onnx-runtime-and-candle-281k) -- Comparative guide
- [Building LLM Applications with Rust: candle and llm Crates](https://dasroot.net/posts/2026/01/building-llm-applications-rust-candle-llm-crates/) -- 2026 Rust LLM guide
- [SurrealDB fastembed integration](https://surrealdb.com/docs/integrations/embeddings/fastembed) -- Production usage example
