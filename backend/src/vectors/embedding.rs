//! Pluggable embedding pipeline (ADR-002: Pluggable Embedding Model).
//!
//! Provides a trait-based embedding abstraction with a fallback provider chain
//! and transparent caching via `moka`.
//!
//! # Cargo.toml dependencies used
//! - `async-trait`
//! - `moka` (feature = "future")
//! - `reqwest` (features = ["json", "rustls-tls"])
//! - `tokio`
//! - `tracing`

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use moka::future::Cache;
use tracing::{debug, warn};

use super::config::{EmbeddingConfig, OnnxConfig};
use super::error::VectorError;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Abstraction over embedding backends so the rest of the system stays
/// provider-agnostic.
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    /// Embed a single text string.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError>;

    /// Batch embed multiple texts.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError>;

    /// Get the model's output dimensions.
    fn dimensions(&self) -> usize;

    /// Get the model name.
    fn model_name(&self) -> &str;

    /// Check if the model is available.
    async fn is_available(&self) -> bool;
}

// ---------------------------------------------------------------------------
// MockEmbeddingModel
// ---------------------------------------------------------------------------

/// Deterministic mock embedding model that derives vectors from a text hash.
///
/// Intended for development and testing only. Activated by setting
/// `embedding.provider = "mock"` in configuration.
pub struct MockEmbeddingModel {
    dims: usize,
}

impl MockEmbeddingModel {
    pub fn new(dims: usize) -> Self {
        Self { dims }
    }

    /// Produce a deterministic pseudo-random vector from `text`.
    fn deterministic_vector(&self, text: &str) -> Vec<f32> {
        let char_sum: u64 = text.chars().map(|c| c as u64).sum();

        // A small set of primes used to seed each dimension.
        const PRIMES: [u64; 8] = [7, 13, 31, 61, 127, 251, 509, 1021];

        let raw: Vec<f32> = (0..self.dims)
            .map(|i| {
                let prime = PRIMES[i % PRIMES.len()];
                let hash_val = (char_sum.wrapping_mul(prime).wrapping_add(i as u64)) % 10_000;
                (hash_val as f32 / 10_000.0) * 2.0 - 1.0 // map to [-1, 1]
            })
            .collect();

        // Normalise to unit length.
        let magnitude = raw.iter().map(|v| v * v).sum::<f32>().sqrt();
        if magnitude == 0.0 {
            return raw;
        }
        raw.into_iter().map(|v| v / magnitude).collect()
    }
}

#[async_trait]
impl EmbeddingModel for MockEmbeddingModel {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        Ok(self.deterministic_vector(text))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        Ok(texts.iter().map(|t| self.deterministic_vector(t)).collect())
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn model_name(&self) -> &str {
        "mock-embedding"
    }

    async fn is_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// OllamaEmbeddingModel
// ---------------------------------------------------------------------------

/// Ollama-backed embedding model using the `/api/embed` HTTP endpoint.
///
/// Requires a running Ollama instance. Configure via `embedding.ollama_url`
/// and `embedding.model` in config.yaml or the corresponding env vars.
pub struct OllamaEmbeddingModel {
    base_url: String,
    model: String,
    dims: usize,
    client: reqwest::Client,
}

impl OllamaEmbeddingModel {
    pub fn new(base_url: String, model: String, dims: usize) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            base_url,
            model,
            dims,
            client,
        }
    }
}

/// JSON request body for `POST /api/embed`.
#[derive(serde::Serialize)]
struct OllamaEmbedRequest<'a, T: serde::Serialize> {
    model: &'a str,
    input: T,
}

/// JSON response from `POST /api/embed`.
#[derive(serde::Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl EmbeddingModel for OllamaEmbeddingModel {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        let url = format!("{}/api/embed", self.base_url);
        let body = OllamaEmbedRequest {
            model: &self.model,
            input: text,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    VectorError::EmbeddingFailed(format!(
                        "Cannot connect to Ollama at {}",
                        self.base_url
                    ))
                } else if e.is_timeout() {
                    VectorError::EmbeddingFailed("Ollama request timed out".to_string())
                } else {
                    VectorError::EmbeddingFailed(format!("Ollama HTTP error: {e}"))
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(VectorError::EmbeddingFailed(format!(
                "Ollama returned status {status}: {body_text}"
            )));
        }

        let parsed: OllamaEmbedResponse = response.json().await.map_err(|e| {
            VectorError::EmbeddingFailed(format!("Failed to parse Ollama response: {e}"))
        })?;

        parsed.embeddings.into_iter().next().ok_or_else(|| {
            VectorError::EmbeddingFailed("Ollama returned empty embeddings array".to_string())
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let url = format!("{}/api/embed", self.base_url);
        let body = OllamaEmbedRequest {
            model: &self.model,
            input: texts,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    VectorError::EmbeddingFailed(format!(
                        "Cannot connect to Ollama at {}",
                        self.base_url
                    ))
                } else if e.is_timeout() {
                    VectorError::EmbeddingFailed("Ollama request timed out".to_string())
                } else {
                    VectorError::EmbeddingFailed(format!("Ollama HTTP error: {e}"))
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(VectorError::EmbeddingFailed(format!(
                "Ollama returned status {status}: {body_text}"
            )));
        }

        let parsed: OllamaEmbedResponse = response.json().await.map_err(|e| {
            VectorError::EmbeddingFailed(format!("Failed to parse Ollama response: {e}"))
        })?;

        if parsed.embeddings.len() != texts.len() {
            return Err(VectorError::EmbeddingFailed(format!(
                "Ollama returned {} embeddings for {} inputs",
                parsed.embeddings.len(),
                texts.len()
            )));
        }

        Ok(parsed.embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// OnnxEmbeddingModel (fastembed — ADR-011)
// ---------------------------------------------------------------------------

/// ONNX-based embedding model via fastembed (Tier 0: zero-config default).
///
/// Downloads the model from Hugging Face Hub on first use and caches it
/// locally.  Runs entirely in-process via ONNX Runtime — no external
/// services needed.
///
/// `fastembed::TextEmbedding::embed` requires `&mut self`, so the inner
/// model is wrapped in a [`Mutex`] to satisfy the `Send + Sync` bound
/// required by [`EmbeddingModel`].
pub struct OnnxEmbeddingModel {
    model: Mutex<fastembed::TextEmbedding>,
    name: String,
    dims: usize,
}

impl OnnxEmbeddingModel {
    /// Initialise a new ONNX embedding model from [`OnnxConfig`].
    pub fn new(config: &OnnxConfig) -> Result<Self, VectorError> {
        use fastembed::{EmbeddingModel as FEModel, TextEmbedding, TextInitOptions};

        let fe_model = match config.model.as_str() {
            "all-MiniLM-L6-v2" => FEModel::AllMiniLML6V2,
            "bge-small-en-v1.5" => FEModel::BGESmallENV15,
            "bge-base-en-v1.5" => FEModel::BGEBaseENV15,
            other => {
                return Err(VectorError::ConfigError(format!(
                    "Unknown ONNX model: {other}. Supported: all-MiniLM-L6-v2, \
                     bge-small-en-v1.5, bge-base-en-v1.5"
                )));
            }
        };

        let mut init = TextInitOptions::new(fe_model);
        init.show_download_progress = config.show_download_progress;
        if let Some(ref path) = config.cache_dir {
            init = init.with_cache_dir(std::path::PathBuf::from(path));
        }

        let model = TextEmbedding::try_new(init).map_err(|e| {
            VectorError::EmbeddingFailed(format!("Failed to initialize ONNX model: {e}"))
        })?;

        Ok(Self {
            model: Mutex::new(model),
            name: config.model.clone(),
            dims: config.dimensions,
        })
    }
}

#[async_trait]
impl EmbeddingModel for OnnxEmbeddingModel {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        let text = text.to_string();
        // fastembed is synchronous; use block_in_place to avoid starving the
        // tokio runtime while holding the Mutex.
        tokio::task::block_in_place(|| {
            let mut model = self.model.lock().map_err(|e| {
                VectorError::EmbeddingFailed(format!("ONNX model lock poisoned: {e}"))
            })?;
            let results = model
                .embed(vec![text], None)
                .map_err(|e| VectorError::EmbeddingFailed(format!("ONNX embed failed: {e}")))?;
            results
                .into_iter()
                .next()
                .ok_or_else(|| VectorError::EmbeddingFailed("ONNX returned empty results".into()))
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let texts: Vec<String> = texts.to_vec();
        tokio::task::block_in_place(|| {
            let mut model = self.model.lock().map_err(|e| {
                VectorError::EmbeddingFailed(format!("ONNX model lock poisoned: {e}"))
            })?;
            model
                .embed(texts, None)
                .map_err(|e| VectorError::EmbeddingFailed(format!("ONNX batch embed failed: {e}")))
        })
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    async fn is_available(&self) -> bool {
        true // always available once initialised
    }
}

// ---------------------------------------------------------------------------
// EmbeddingPipeline
// ---------------------------------------------------------------------------

/// Orchestrates embedding generation with caching, query augmentation, and
/// provider fallback (ADR-002).
pub struct EmbeddingPipeline {
    providers: Vec<Arc<dyn EmbeddingModel>>,
    cache: Cache<u64, Vec<f32>>,
    min_query_tokens: usize,
}

impl EmbeddingPipeline {
    /// Build a new pipeline from [`EmbeddingConfig`].
    ///
    /// The `provider` field in the config determines the provider chain:
    /// - `"onnx"` -> in-process ONNX via fastembed (zero-config default, ADR-011).
    /// - `"mock"` -> deterministic hash-based embeddings (development/testing).
    /// - `"ollama"` -> Ollama HTTP API. Fails explicitly if Ollama is down.
    /// - `"cloud"` -> cloud-hosted provider (reserved for future use).
    ///
    /// Unknown providers return [`VectorError::ConfigError`].
    pub fn new(config: &EmbeddingConfig) -> Result<Self, VectorError> {
        let mut providers: Vec<Arc<dyn EmbeddingModel>> = Vec::new();

        match config.provider.as_str() {
            "onnx" => match OnnxEmbeddingModel::new(&config.onnx) {
                Ok(model) => providers.push(Arc::new(model)),
                Err(e) => {
                    warn!(
                        "Failed to initialize ONNX embedding model: {e}. \
                         Falling back to mock for development."
                    );
                    providers.push(Arc::new(MockEmbeddingModel::new(config.dimensions)));
                }
            },
            "mock" => {
                providers.push(Arc::new(MockEmbeddingModel::new(config.dimensions)));
            }
            "ollama" => {
                providers.push(Arc::new(OllamaEmbeddingModel::new(
                    config.ollama_url.clone(),
                    config.model.clone(),
                    config.dimensions,
                )));
            }
            "cloud" => {
                return Err(VectorError::ConfigError(
                    "Cloud embedding provider is unsupported; use \"ollama\" or \"mock\""
                        .to_string(),
                ));
            }
            other => {
                return Err(VectorError::ConfigError(format!(
                    "Unknown embedding provider: {other}"
                )));
            }
        }

        let cache = Cache::new(config.cache_size);

        Ok(Self {
            providers,
            cache,
            min_query_tokens: config.min_query_tokens,
        })
    }

    /// Build a pipeline from an explicit list of providers (useful for tests).
    pub fn from_providers(
        providers: Vec<Arc<dyn EmbeddingModel>>,
        cache_size: u64,
        min_query_tokens: usize,
    ) -> Self {
        Self {
            providers,
            cache: Cache::new(cache_size),
            min_query_tokens,
        }
    }

    // -- public API ----------------------------------------------------------

    /// Embed a single piece of text with caching and fallback.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        let cache_key = Self::hash_text(text);

        // 1. Cache check.
        if let Some(cached) = self.cache.get(&cache_key).await {
            debug!("embedding cache hit for key={cache_key}");
            return Ok(cached);
        }

        // 2. Query augmentation for short texts.
        let augmented = self.maybe_augment(text);
        let input = augmented.as_deref().unwrap_or(text);

        // 3. Walk the provider chain.
        let mut last_err = String::new();
        for provider in &self.providers {
            match provider.embed(input).await {
                Ok(vec) => {
                    self.cache.insert(cache_key, vec.clone()).await;
                    return Ok(vec);
                }
                Err(e) => {
                    warn!(
                        provider = provider.model_name(),
                        "embedding provider failed: {e}"
                    );
                    last_err = e.to_string();
                }
            }
        }

        Err(VectorError::AllProvidersUnavailable(last_err))
    }

    /// Batch embed with per-text caching and fallback.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut miss_indices: Vec<usize> = Vec::new();
        let mut miss_texts: Vec<String> = Vec::new();

        // 1. Collect cache hits and misses.
        for (i, text) in texts.iter().enumerate() {
            let key = Self::hash_text(text);
            if let Some(cached) = self.cache.get(&key).await {
                results[i] = Some(cached);
            } else {
                miss_indices.push(i);
                let augmented = self.maybe_augment(text);
                miss_texts.push(augmented.unwrap_or_else(|| text.clone()));
            }
        }

        // 2. Batch-embed the misses.
        if !miss_texts.is_empty() {
            let new_vecs = self.batch_with_fallback(&miss_texts).await?;

            for (j, idx) in miss_indices.iter().enumerate() {
                let key = Self::hash_text(&texts[*idx]);
                self.cache.insert(key, new_vecs[j].clone()).await;
                results[*idx] = Some(new_vecs[j].clone());
            }
        }

        // 3. Unwrap -- every slot should be filled.
        Ok(results.into_iter().map(|r| r.unwrap()).collect())
    }

    /// Returns `true` if at least one provider is available.
    pub async fn is_available(&self) -> bool {
        for provider in &self.providers {
            if provider.is_available().await {
                return true;
            }
        }
        false
    }

    /// Format email fields into a single text suitable for embedding.
    pub fn prepare_email_text(subject: &str, from_addr: &str, body_text: &str) -> String {
        let truncated_body: String = body_text.chars().take(400).collect();
        format!("{subject}\nFrom: {from_addr}\n{truncated_body}")
    }

    // -- helpers -------------------------------------------------------------

    /// Produce a deterministic `u64` hash for cache keying.
    fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// Prepend context to short queries to improve embedding quality.
    fn maybe_augment(&self, text: &str) -> Option<String> {
        let word_count = text.split_whitespace().count();
        if word_count < self.min_query_tokens {
            Some(format!("email search: {text}"))
        } else {
            None
        }
    }

    /// Try each provider in order for batch embedding.
    async fn batch_with_fallback(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let mut last_err = String::new();
        for provider in &self.providers {
            match provider.embed_batch(texts).await {
                Ok(vecs) => return Ok(vecs),
                Err(e) => {
                    warn!(
                        provider = provider.model_name(),
                        "batch embedding provider failed: {e}"
                    );
                    last_err = e.to_string();
                }
            }
        }
        Err(VectorError::AllProvidersUnavailable(last_err))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A model that always fails -- used to test fallback behaviour.
    struct FailingEmbeddingModel;

    #[async_trait]
    impl EmbeddingModel for FailingEmbeddingModel {
        async fn embed(&self, _text: &str) -> Result<Vec<f32>, VectorError> {
            Err(VectorError::EmbeddingFailed("always fails".into()))
        }

        async fn embed_batch(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
            Err(VectorError::EmbeddingFailed("always fails".into()))
        }

        fn dimensions(&self) -> usize {
            384
        }

        fn model_name(&self) -> &str {
            "failing-model"
        }

        async fn is_available(&self) -> bool {
            false
        }
    }

    fn default_pipeline() -> EmbeddingPipeline {
        let config = EmbeddingConfig {
            provider: "mock".to_string(),
            ..EmbeddingConfig::default()
        };
        EmbeddingPipeline::new(&config).unwrap()
    }

    // -- MockEmbeddingModel tests -------------------------------------------

    #[tokio::test]
    async fn test_mock_embedding_deterministic() {
        let model = MockEmbeddingModel::new(384);
        let v1 = model.embed("hello world").await.unwrap();
        let v2 = model.embed("hello world").await.unwrap();
        assert_eq!(v1, v2, "same text must produce the same embedding");
    }

    #[tokio::test]
    async fn test_mock_embedding_different_texts() {
        let model = MockEmbeddingModel::new(384);
        let v1 = model.embed("hello world").await.unwrap();
        let v2 = model.embed("goodbye world").await.unwrap();
        assert_ne!(v1, v2, "different texts must produce different embeddings");
    }

    #[tokio::test]
    async fn test_mock_embedding_dimensions() {
        let model = MockEmbeddingModel::new(384);
        let v = model.embed("test input").await.unwrap();
        assert_eq!(v.len(), 384);
        assert_eq!(model.dimensions(), 384);
    }

    #[tokio::test]
    async fn test_mock_embedding_normalized() {
        let model = MockEmbeddingModel::new(384);
        let v = model
            .embed("some text for normalization check")
            .await
            .unwrap();
        let magnitude: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (magnitude - 1.0).abs() < 0.01,
            "vector should be unit length, got magnitude={magnitude}"
        );
    }

    // -- EmbeddingPipeline tests --------------------------------------------

    #[tokio::test]
    async fn test_pipeline_embed_caches() {
        let pipeline = default_pipeline();

        let v1 = pipeline.embed("cache me").await.unwrap();
        // Second call should hit the cache and still return the same vector.
        let v2 = pipeline.embed("cache me").await.unwrap();
        assert_eq!(v1, v2);

        // Verify it is really in the cache.
        let key = EmbeddingPipeline::hash_text("cache me");
        assert!(pipeline.cache.get(&key).await.is_some());
    }

    #[tokio::test]
    async fn test_pipeline_embed_batch() {
        let pipeline = default_pipeline();
        let texts: Vec<String> = vec![
            "first email about meetings".into(),
            "second email about invoices".into(),
            "third email about travel plans".into(),
        ];

        let vecs = pipeline.embed_batch(&texts).await.unwrap();
        assert_eq!(vecs.len(), 3);
        for v in &vecs {
            assert_eq!(v.len(), 384);
        }

        // Each vector should be different.
        assert_ne!(vecs[0], vecs[1]);
        assert_ne!(vecs[1], vecs[2]);

        // They should now be cached.
        for text in &texts {
            let key = EmbeddingPipeline::hash_text(text);
            assert!(pipeline.cache.get(&key).await.is_some());
        }
    }

    #[tokio::test]
    async fn test_pipeline_short_query_augmentation() {
        // With min_query_tokens = 5 (default), a short query gets augmented.
        let pipeline = default_pipeline();

        // "hi" is 1 word -> augmented to "email search: hi"
        // So embedding("hi") and embedding("email search: hi") should produce
        // the same result via the pipeline (since the pipeline always augments
        // the short query before calling the provider).
        let v_short = pipeline.embed("hi").await.unwrap();

        // Directly embed the augmented form via the model to compare.
        let model = MockEmbeddingModel::new(384);
        let v_augmented = model.embed("email search: hi").await.unwrap();

        assert_eq!(v_short, v_augmented);
    }

    #[tokio::test]
    async fn test_prepare_email_text_format() {
        let result = EmbeddingPipeline::prepare_email_text(
            "Meeting Tomorrow",
            "alice@example.com",
            "Please join the meeting at 10am.",
        );
        assert_eq!(
            result,
            "Meeting Tomorrow\nFrom: alice@example.com\nPlease join the meeting at 10am."
        );
    }

    #[tokio::test]
    async fn test_prepare_email_text_truncation() {
        let long_body = "a".repeat(600);
        let result = EmbeddingPipeline::prepare_email_text("Subj", "bob@example.com", &long_body);

        // The body portion should be truncated to 400 chars.
        let expected = format!("Subj\nFrom: bob@example.com\n{}", "a".repeat(400));
        assert_eq!(result, expected);
        // Verify the full 600-char body was NOT included.
        assert!(result.len() < 450);
    }

    #[tokio::test]
    async fn test_pipeline_fallback_chain() {
        // First provider always fails, second (mock) succeeds.
        let failing: Arc<dyn EmbeddingModel> = Arc::new(FailingEmbeddingModel);
        let mock: Arc<dyn EmbeddingModel> = Arc::new(MockEmbeddingModel::new(384));

        let pipeline = EmbeddingPipeline::from_providers(vec![failing, mock.clone()], 100, 5);

        let v = pipeline
            .embed("fallback test text with enough words")
            .await
            .unwrap();
        assert_eq!(v.len(), 384);

        // Result should match the mock model directly.
        let expected = mock
            .embed("fallback test text with enough words")
            .await
            .unwrap();
        assert_eq!(v, expected);
    }

    // -- OnnxEmbeddingModel tests (require model download) ------------------

    #[tokio::test]
    #[ignore] // requires ONNX model download (~80 MB)
    async fn test_onnx_embedding_dimensions() {
        let config = super::super::config::OnnxConfig::default();
        let model = OnnxEmbeddingModel::new(&config).expect("ONNX init should succeed");
        let v = model.embed("hello world").await.unwrap();
        assert_eq!(
            v.len(),
            384,
            "all-MiniLM-L6-v2 produces 384-dim embeddings"
        );
        assert_eq!(model.dimensions(), 384);
    }

    #[tokio::test]
    #[ignore] // requires ONNX model download
    async fn test_onnx_embedding_deterministic() {
        let config = super::super::config::OnnxConfig::default();
        let model = OnnxEmbeddingModel::new(&config).unwrap();
        let v1 = model.embed("determinism check").await.unwrap();
        let v2 = model.embed("determinism check").await.unwrap();
        assert_eq!(v1, v2, "same text must produce the same ONNX embedding");
    }

    #[tokio::test]
    #[ignore] // requires ONNX model download
    async fn test_onnx_embedding_semantic_similarity() {
        let config = super::super::config::OnnxConfig::default();
        let model = OnnxEmbeddingModel::new(&config).unwrap();

        let v_cat = model.embed("The cat sat on the mat").await.unwrap();
        let v_dog = model.embed("The dog lay on the rug").await.unwrap();
        let v_stock = model.embed("Stock prices rose sharply today").await.unwrap();

        fn cosine(a: &[f32], b: &[f32]) -> f32 {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (na * nb)
        }

        let sim_animals = cosine(&v_cat, &v_dog);
        let sim_unrelated = cosine(&v_cat, &v_stock);

        assert!(
            sim_animals > sim_unrelated,
            "semantically similar sentences should be closer: \
             animals={sim_animals:.4}, unrelated={sim_unrelated:.4}"
        );
    }
}
