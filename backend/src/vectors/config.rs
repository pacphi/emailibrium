//! Configuration for the vector intelligence layer.

use serde::{Deserialize, Serialize};

use super::error::VectorError;

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorConfig {
    /// Server host.
    #[serde(default = "default_host")]
    pub host: String,
    /// Server port.
    #[serde(default = "default_port")]
    pub port: u16,
    /// SQLite/PostgreSQL connection URL.
    #[serde(default = "default_database_url")]
    pub database_url: String,
    /// Vector store settings.
    #[serde(default)]
    pub store: StoreConfig,
    /// Embedding pipeline settings.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// HNSW index settings.
    #[serde(default)]
    pub index: IndexConfig,
    /// Search settings.
    #[serde(default)]
    pub search: SearchConfig,
    /// Encryption settings (ADR-008).
    #[serde(default)]
    pub encryption: EncryptionConfig,
    /// Categorizer settings.
    #[serde(default)]
    pub categorizer: CategorizerConfig,
    /// Backup settings (ADR-003).
    #[serde(default)]
    pub backup: BackupConfig,
    /// Clustering settings (ADR-009).
    #[serde(default)]
    pub clustering: super::clustering::ClusterConfig,
    /// SONA learning engine settings (ADR-004).
    #[serde(default)]
    pub learning: super::learning::LearningConfig,
    /// Quantization settings (ADR-007).
    #[serde(default)]
    pub quantization: super::quantization::QuantizationConfig,
    /// Generative AI settings (ADR-012).
    #[serde(default)]
    pub generative: GenerativeConfig,
    /// RAG (Retrieval-Augmented Generation) settings (ADR-022, DDD-010).
    #[serde(default)]
    pub rag: super::rag::RagConfig,
    /// OAuth provider settings (DDD-005).
    #[serde(default)]
    pub oauth: OAuthConfig,
    /// Redis cache settings.
    #[serde(default)]
    pub redis: RedisConfig,
    /// Security settings (CORS + CSP, audit items #6 / #13).
    #[serde(default)]
    pub security: SecurityConfig,
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            database_url: default_database_url(),
            store: StoreConfig::default(),
            embedding: EmbeddingConfig::default(),
            index: IndexConfig::default(),
            search: SearchConfig::default(),
            encryption: EncryptionConfig::default(),
            categorizer: CategorizerConfig::default(),
            backup: BackupConfig::default(),
            clustering: super::clustering::ClusterConfig::default(),
            learning: super::learning::LearningConfig::default(),
            quantization: super::quantization::QuantizationConfig::default(),
            generative: GenerativeConfig::default(),
            rag: super::rag::RagConfig::default(),
            oauth: OAuthConfig::default(),
            redis: RedisConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl VectorConfig {
    /// Load configuration from YAML file + env vars via figment.
    pub fn load() -> Result<Self, VectorError> {
        use figment::{
            providers::{Env, Format, Yaml},
            Figment,
        };

        let config: Self = Figment::new()
            .merge(Yaml::file("config.yaml"))
            .merge(Yaml::file("config.local.yaml"))
            .merge(Env::prefixed("EMAILIBRIUM_").split("_"))
            .extract()
            .map_err(|e| VectorError::ConfigError(e.to_string()))?;

        Ok(config)
    }

    /// Apply `app.yaml` path overrides as fallback defaults.
    ///
    /// When a Figment config field still has its compile-time default value,
    /// the corresponding `app.yaml` path is used instead.  This allows users
    /// to centralise path configuration in `config/app.yaml` without having
    /// to duplicate values in `config.yaml`.
    pub fn apply_yaml_path_defaults(&mut self, paths: &crate::vectors::yaml_config::PathsConfig) {
        // store.path  ←  paths.vector_data_dir
        if self.store.path == default_store_path() && paths.vector_data_dir != default_store_path()
        {
            tracing::debug!(
                "Overriding store.path with app.yaml paths.vector_data_dir: {}",
                paths.vector_data_dir
            );
            self.store.path = paths.vector_data_dir.clone();
        }

        // generative.builtin.cache_dir  ←  paths.llm_cache_dir
        if self.generative.builtin.cache_dir == default_builtin_cache_dir()
            && paths.llm_cache_dir != default_builtin_cache_dir()
        {
            tracing::debug!(
                "Overriding generative.builtin.cache_dir with app.yaml paths.llm_cache_dir: {}",
                paths.llm_cache_dir
            );
            self.generative.builtin.cache_dir = paths.llm_cache_dir.clone();
        }

        // embedding.onnx.cache_dir  ←  paths.embedding_cache_dir
        let yaml_embed = &paths.embedding_cache_dir;
        if self.embedding.onnx.cache_dir.is_none() && yaml_embed != ".fastembed_cache" {
            tracing::debug!(
                "Overriding embedding.onnx.cache_dir with app.yaml paths.embedding_cache_dir: {}",
                yaml_embed
            );
            self.embedding.onnx.cache_dir = Some(yaml_embed.clone());
        }

        // database_url  ←  paths.database_file
        let default_db = default_database_url();
        if self.database_url == default_db && paths.database_file != "emailibrium.db" {
            let new_url = format!("sqlite:{}?mode=rwc", paths.database_file);
            tracing::debug!(
                "Overriding database_url with app.yaml paths.database_file: {}",
                new_url
            );
            self.database_url = new_url;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// Path for vector data persistence.
    #[serde(default = "default_store_path")]
    pub path: String,
    /// Whether vectors are enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Backend selection: "ruvector" (HNSW, default), "qdrant", "sqlite", or "memory".
    #[serde(default = "default_store_backend")]
    pub backend: String,
    /// Qdrant REST API URL (used when backend = "qdrant").
    #[serde(default)]
    pub qdrant_url: Option<String>,
    /// Qdrant collection name prefix (used when backend = "qdrant").
    #[serde(default)]
    pub qdrant_collection_prefix: Option<String>,
    /// Qdrant API key for authenticated deployments (used when backend = "qdrant").
    #[serde(default)]
    pub qdrant_api_key: Option<String>,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: default_store_path(),
            enabled: true,
            backend: default_store_backend(),
            qdrant_url: None,
            qdrant_collection_prefix: None,
            qdrant_api_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding provider priority order.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Model name for text embeddings.
    #[serde(default = "default_model")]
    pub model: String,
    /// Embedding dimensions.
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    /// Batch size for bulk embedding.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Number of entries in the embedding cache.
    #[serde(default = "default_cache_size")]
    pub cache_size: u64,
    /// Ollama base URL (fallback provider).
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// Minimum token count before query augmentation kicks in.
    #[serde(default = "default_min_query_tokens")]
    pub min_query_tokens: usize,
    /// ONNX / fastembed configuration (ADR-011).
    #[serde(default)]
    pub onnx: OnnxConfig,
    /// Cloud embedding settings (OpenAI text-embedding-3-small, audit item #14).
    #[serde(default)]
    pub cloud: CloudEmbeddingConfig,
    /// Cohere embedding settings (audit item #30).
    #[serde(default)]
    pub cohere: CohereEmbeddingConfig,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            dimensions: default_dimensions(),
            batch_size: default_batch_size(),
            cache_size: default_cache_size(),
            ollama_url: default_ollama_url(),
            min_query_tokens: default_min_query_tokens(),
            onnx: OnnxConfig::default(),
            cloud: CloudEmbeddingConfig::default(),
            cohere: CohereEmbeddingConfig::default(),
        }
    }
}

/// Configuration for the ONNX embedding provider (fastembed, ADR-011).
///
/// Downloads the specified model from Hugging Face Hub on first use and
/// caches it locally. Runs entirely in-process via ONNX Runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnnxConfig {
    /// Model name. Supported: all-MiniLM-L6-v2, bge-small-en-v1.5, bge-base-en-v1.5.
    #[serde(default = "default_onnx_model")]
    pub model: String,
    /// Local cache directory for downloaded model files. `None` uses fastembed default.
    #[serde(default)]
    pub cache_dir: Option<String>,
    /// Show download progress on first model fetch.
    #[serde(default = "default_true")]
    pub show_download_progress: bool,
    /// Output embedding dimensions (must match the chosen model).
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
}

impl Default for OnnxConfig {
    fn default() -> Self {
        Self {
            model: default_onnx_model(),
            cache_dir: None,
            show_download_progress: true,
            dimensions: default_dimensions(),
        }
    }
}

/// Configuration for the cloud (OpenAI) embedding provider (audit item #14).
///
/// Uses OpenAI's embedding API (`text-embedding-3-small` by default).
/// The API key is read from the environment variable named in `api_key_env`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudEmbeddingConfig {
    /// Name of the environment variable holding the OpenAI API key.
    #[serde(default = "default_cloud_embedding_api_key_env")]
    pub api_key_env: String,
    /// Model identifier (e.g. "text-embedding-3-small", "text-embedding-3-large").
    #[serde(default = "default_cloud_embedding_model")]
    pub model: String,
    /// Base URL for the embedding API.
    #[serde(default = "default_cloud_embedding_base_url")]
    pub base_url: String,
    /// Output embedding dimensions.
    #[serde(default = "default_cloud_embedding_dimensions")]
    pub dimensions: usize,
}

impl Default for CloudEmbeddingConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_cloud_embedding_api_key_env(),
            model: default_cloud_embedding_model(),
            base_url: default_cloud_embedding_base_url(),
            dimensions: default_cloud_embedding_dimensions(),
        }
    }
}

/// Configuration for the Cohere embedding provider (audit item #30).
///
/// Uses Cohere's embed API v2 (`embed-english-v3.0` by default).
/// The API key is read from the environment variable named in `api_key_env`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CohereEmbeddingConfig {
    /// Name of the environment variable holding the Cohere API key.
    #[serde(default = "default_cohere_api_key_env")]
    pub api_key_env: String,
    /// Model identifier (e.g. "embed-english-v3.0", "embed-multilingual-v3.0").
    #[serde(default = "default_cohere_embedding_model")]
    pub model: String,
    /// Base URL for the Cohere API.
    #[serde(default = "default_cohere_base_url")]
    pub base_url: String,
    /// Output embedding dimensions.
    #[serde(default = "default_cohere_dimensions")]
    pub dimensions: usize,
    /// Input type hint for the Cohere API.
    #[serde(default = "default_cohere_input_type")]
    pub input_type: String,
}

impl Default for CohereEmbeddingConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_cohere_api_key_env(),
            model: default_cohere_embedding_model(),
            base_url: default_cohere_base_url(),
            dimensions: default_cohere_dimensions(),
            input_type: default_cohere_input_type(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// HNSW M parameter (connections per node).
    #[serde(default = "default_m")]
    pub m: usize,
    /// HNSW ef_construction parameter.
    #[serde(default = "default_ef_construction")]
    pub ef_construction: usize,
    /// HNSW ef_search parameter.
    #[serde(default = "default_ef_search")]
    pub ef_search: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            m: default_m(),
            ef_construction: default_ef_construction(),
            ef_search: default_ef_search(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Default result limit.
    #[serde(default = "default_search_limit")]
    pub default_limit: usize,
    /// Maximum result limit.
    #[serde(default = "default_max_limit")]
    pub max_limit: usize,
    /// Minimum cosine similarity to include in results.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f32,
    /// Whether SONA re-ranking is applied after RRF fusion (DDD-002, item #18).
    #[serde(default)]
    pub sona_reranking_enabled: bool,
    /// Blending weight for SONA re-ranking (0.0 = pure RRF, 1.0 = pure SONA).
    #[serde(default = "default_sona_weight")]
    pub sona_weight: f32,
    /// Collections to search in multi-collection mode (DDD-002, item #25).
    /// Defaults to `["email_text"]` for backward compatibility.
    #[serde(default = "default_search_collections")]
    pub collections: Vec<String>,
    /// Per-collection weight multipliers keyed by collection name.
    /// Missing collections default to 1.0.
    #[serde(default)]
    pub collection_weights: std::collections::HashMap<String, f32>,
    /// RRF k parameter (ADR-029 Phase C). Defaults to 60 for backward compat.
    #[serde(default = "default_rrf_k_search")]
    pub rrf_k: u32,
}

fn default_rrf_k_search() -> u32 {
    60
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: default_search_limit(),
            max_limit: default_max_limit(),
            similarity_threshold: default_similarity_threshold(),
            sona_reranking_enabled: false,
            sona_weight: default_sona_weight(),
            collections: default_search_collections(),
            collection_weights: std::collections::HashMap::new(),
            rrf_k: default_rrf_k_search(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Whether encryption at rest is enabled (ADR-008).
    #[serde(default)]
    pub enabled: bool,
    /// Master password for key derivation (set via env var, never in config file).
    #[serde(default)]
    pub master_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizerConfig {
    /// Minimum confidence for vector centroid classification.
    /// Below this, falls back to LLM (ADR-004).
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f32,
    /// Maximum centroid shift per feedback event.
    #[serde(default = "default_max_centroid_shift")]
    pub max_centroid_shift: f32,
    /// Minimum feedback events before centroid updates activate.
    #[serde(default = "default_min_feedback_events")]
    pub min_feedback_events: u32,
}

impl Default for CategorizerConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: default_confidence_threshold(),
            max_centroid_shift: default_max_centroid_shift(),
            min_feedback_events: default_min_feedback_events(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Whether automatic SQLite backup is enabled (ADR-003).
    #[serde(default)]
    pub enabled: bool,
    /// Backup interval in seconds.
    #[serde(default = "default_backup_interval")]
    pub interval_secs: u64,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_backup_interval(),
        }
    }
}

// Default value functions
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8080
}
fn default_database_url() -> String {
    "sqlite:emailibrium.db?mode=rwc".to_string()
}
fn default_store_path() -> String {
    "data/vectors".to_string()
}
fn default_store_backend() -> String {
    "ruvector".to_string()
}
fn default_true() -> bool {
    true
}
/// Default embedding provider. Set to "onnx" for zero-config local embedding via fastembed
/// (ADR-011). Other options: "mock" (development/testing), "ollama", "cloud".
fn default_provider() -> String {
    "onnx".to_string()
}
fn default_onnx_model() -> String {
    "all-MiniLM-L6-v2".to_string()
}
fn default_model() -> String {
    "all-MiniLM-L6-v2".to_string()
}
fn default_dimensions() -> usize {
    384
}
fn default_batch_size() -> usize {
    64
}
fn default_cache_size() -> u64 {
    10_000
}
fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}
fn default_min_query_tokens() -> usize {
    5
}
fn default_m() -> usize {
    16
}
fn default_ef_construction() -> usize {
    200
}
fn default_ef_search() -> usize {
    100
}
fn default_search_limit() -> usize {
    20
}
fn default_max_limit() -> usize {
    100
}
fn default_similarity_threshold() -> f32 {
    0.5
}
fn default_sona_weight() -> f32 {
    0.3
}
fn default_search_collections() -> Vec<String> {
    vec!["email_text".to_string()]
}
fn default_confidence_threshold() -> f32 {
    0.7
}
fn default_max_centroid_shift() -> f32 {
    0.1
}
fn default_min_feedback_events() -> u32 {
    10
}
fn default_backup_interval() -> u64 {
    3600
}

// Cloud embedding defaults (audit item #14)
fn default_cloud_embedding_api_key_env() -> String {
    "EMAILIBRIUM_OPENAI_API_KEY".to_string()
}
fn default_cloud_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}
fn default_cloud_embedding_base_url() -> String {
    "https://api.openai.com".to_string()
}
fn default_cloud_embedding_dimensions() -> usize {
    1536
}

// Cohere embedding defaults (audit item #30)
fn default_cohere_api_key_env() -> String {
    "EMAILIBRIUM_COHERE_API_KEY".to_string()
}
fn default_cohere_embedding_model() -> String {
    "embed-english-v3.0".to_string()
}
fn default_cohere_base_url() -> String {
    "https://api.cohere.com".to_string()
}
fn default_cohere_dimensions() -> usize {
    1024
}
fn default_cohere_input_type() -> String {
    "search_document".to_string()
}

// ---------------------------------------------------------------------------
// Redis config
// ---------------------------------------------------------------------------

/// Redis cache configuration.
///
/// The backend operates without Redis (graceful degradation). When `enabled`
/// is `true` and a connection can be established, embedding results and other
/// hot-path data are cached in Redis to avoid redundant computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Whether Redis caching is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Redis connection URL. Respects `REDIS_URL` env var via figment.
    #[serde(default = "default_redis_url")]
    pub url: String,
    /// Default TTL in seconds for cached entries.
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_redis_url(),
            cache_ttl_secs: default_cache_ttl_secs(),
        }
    }
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379".to_string()
}
fn default_cache_ttl_secs() -> u64 {
    3600
}

// ---------------------------------------------------------------------------
// Generative AI config (ADR-012)
// ---------------------------------------------------------------------------

/// Configuration for the generative AI subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerativeConfig {
    /// Provider selection: "builtin" | "none" | "ollama" | "cloud".
    #[serde(default = "default_gen_provider")]
    pub provider: String,
    /// Built-in local LLM settings (Tier 0.5, ADR-021).
    #[serde(default)]
    pub builtin: BuiltInLlmConfig,
    /// Ollama-specific settings (Tier 1).
    #[serde(default)]
    pub ollama: OllamaGenerativeConfig,
    /// Cloud provider settings (Tier 2).
    #[serde(default)]
    pub cloud: CloudGenerativeConfig,
}

impl Default for GenerativeConfig {
    fn default() -> Self {
        Self {
            provider: default_gen_provider(),
            builtin: BuiltInLlmConfig::default(),
            ollama: OllamaGenerativeConfig::default(),
            cloud: CloudGenerativeConfig::default(),
        }
    }
}

/// Built-in local LLM settings (Tier 0.5, ADR-021).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltInLlmConfig {
    /// Model identifier from the GGUF manifest.
    #[serde(default = "default_builtin_model")]
    pub model_id: String,
    /// Context window size in tokens.
    #[serde(default = "default_builtin_context_size")]
    pub context_size: u32,
    /// Number of layers to offload to GPU (0 = CPU only, 99 = all).
    #[serde(default = "default_builtin_gpu_layers")]
    pub gpu_layers: u32,
    /// Seconds of inactivity before unloading the model to free RAM.
    #[serde(default = "default_builtin_idle_timeout")]
    pub idle_timeout_secs: u64,
    /// Directory for cached GGUF model files.
    #[serde(default = "default_builtin_cache_dir")]
    pub cache_dir: String,
}

impl Default for BuiltInLlmConfig {
    fn default() -> Self {
        Self {
            model_id: default_builtin_model(),
            context_size: default_builtin_context_size(),
            gpu_layers: default_builtin_gpu_layers(),
            idle_timeout_secs: default_builtin_idle_timeout(),
            cache_dir: default_builtin_cache_dir(),
        }
    }
}

/// Ollama generative model settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaGenerativeConfig {
    /// Base URL for the Ollama API.
    #[serde(default = "default_ollama_gen_url")]
    pub base_url: String,
    /// Model to use for classification prompts.
    #[serde(default = "default_ollama_classification_model")]
    pub classification_model: String,
    /// Model to use for chat / free-form generation.
    #[serde(default = "default_ollama_chat_model")]
    pub chat_model: String,
}

impl Default for OllamaGenerativeConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_gen_url(),
            classification_model: default_ollama_classification_model(),
            chat_model: default_ollama_chat_model(),
        }
    }
}

/// Cloud generative model settings (OpenAI / Anthropic / Gemini).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudGenerativeConfig {
    /// Cloud provider: "openai" | "anthropic" | "gemini".
    #[serde(default = "default_cloud_provider")]
    pub provider: String,
    /// Name of the environment variable holding the API key.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Model identifier (e.g. "gpt-4o-mini", "claude-sonnet-4-6", "gemini-2.0-flash").
    #[serde(default = "default_cloud_model")]
    pub model: String,
    /// Base URL for the provider API.
    #[serde(default = "default_cloud_base_url")]
    pub base_url: String,
    /// Gemini-specific settings (audit item #29).
    #[serde(default)]
    pub gemini: GeminiGenerativeConfig,
}

impl Default for CloudGenerativeConfig {
    fn default() -> Self {
        Self {
            provider: default_cloud_provider(),
            api_key_env: default_api_key_env(),
            model: default_cloud_model(),
            base_url: default_cloud_base_url(),
            gemini: GeminiGenerativeConfig::default(),
        }
    }
}

/// Gemini (Google AI) generative model settings (audit item #29).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiGenerativeConfig {
    /// Name of the environment variable holding the Gemini API key.
    #[serde(default = "default_gemini_api_key_env")]
    pub api_key_env: String,
    /// Model identifier (e.g. "gemini-2.0-flash", "gemini-2.5-pro").
    #[serde(default = "default_gemini_model")]
    pub model: String,
    /// Base URL for the Gemini API.
    #[serde(default = "default_gemini_base_url")]
    pub base_url: String,
}

impl Default for GeminiGenerativeConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_gemini_api_key_env(),
            model: default_gemini_model(),
            base_url: default_gemini_base_url(),
        }
    }
}

fn default_gen_provider() -> String {
    "builtin".to_string()
}

fn default_builtin_model() -> String {
    "qwen3-1.7b-q4km".to_string()
}
fn default_builtin_context_size() -> u32 {
    2048
}
fn default_builtin_gpu_layers() -> u32 {
    99
}
fn default_builtin_idle_timeout() -> u64 {
    300
}
fn default_builtin_cache_dir() -> String {
    "~/.emailibrium/models/llm".to_string()
}
fn default_ollama_gen_url() -> String {
    "http://localhost:11434".to_string()
}
fn default_ollama_classification_model() -> String {
    "llama3.2:1b".to_string()
}
fn default_ollama_chat_model() -> String {
    "llama3.2:3b".to_string()
}
fn default_cloud_provider() -> String {
    "openai".to_string()
}
fn default_api_key_env() -> String {
    "EMAILIBRIUM_CLOUD_API_KEY".to_string()
}
fn default_cloud_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_cloud_base_url() -> String {
    "https://api.openai.com".to_string()
}

// Gemini generative defaults (audit item #29)
fn default_gemini_api_key_env() -> String {
    "EMAILIBRIUM_GEMINI_API_KEY".to_string()
}
fn default_gemini_model() -> String {
    "gemini-2.0-flash".to_string()
}
fn default_gemini_base_url() -> String {
    "https://generativelanguage.googleapis.com".to_string()
}

// ---------------------------------------------------------------------------
// OAuth config (DDD-005: Account Management)
// ---------------------------------------------------------------------------

/// OAuth provider configuration for Gmail and Outlook account connections.
///
/// Client credentials are loaded from environment variables (never from config
/// files) to prevent accidental secret exposure. The env var names are
/// configurable so deployments can use their own naming conventions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// Gmail OAuth settings.
    #[serde(default)]
    pub gmail: GmailOAuthConfig,
    /// Outlook/Microsoft OAuth settings.
    #[serde(default)]
    pub outlook: OutlookOAuthConfig,
    /// OAuth callback base URL (used to construct redirect URIs).
    #[serde(default = "default_oauth_redirect_base")]
    pub redirect_base_url: String,
    /// Frontend URL to redirect to after OAuth completes.
    #[serde(default = "default_oauth_frontend_url")]
    pub frontend_url: String,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            gmail: GmailOAuthConfig::default(),
            outlook: OutlookOAuthConfig::default(),
            redirect_base_url: default_oauth_redirect_base(),
            frontend_url: default_oauth_frontend_url(),
        }
    }
}

/// Gmail (Google) OAuth2 configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailOAuthConfig {
    /// Env var holding the Google OAuth Client ID.
    #[serde(default = "default_google_client_id_env")]
    pub client_id_env: String,
    /// Env var holding the Google OAuth Client Secret.
    #[serde(default = "default_google_client_secret_env")]
    pub client_secret_env: String,
    /// OAuth scopes requested from Google.
    #[serde(default = "default_gmail_scopes")]
    pub scopes: Vec<String>,
    /// Google OAuth authorization endpoint.
    #[serde(default = "default_google_auth_url")]
    pub auth_url: String,
    /// Google OAuth token endpoint.
    #[serde(default = "default_google_token_url")]
    pub token_url: String,
}

impl Default for GmailOAuthConfig {
    fn default() -> Self {
        Self {
            client_id_env: default_google_client_id_env(),
            client_secret_env: default_google_client_secret_env(),
            scopes: default_gmail_scopes(),
            auth_url: default_google_auth_url(),
            token_url: default_google_token_url(),
        }
    }
}

/// Outlook (Microsoft Entra) OAuth2 configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlookOAuthConfig {
    /// Env var holding the Microsoft OAuth Client ID (Application ID).
    #[serde(default = "default_microsoft_client_id_env")]
    pub client_id_env: String,
    /// Env var holding the Microsoft OAuth Client Secret.
    #[serde(default = "default_microsoft_client_secret_env")]
    pub client_secret_env: String,
    /// Tenant ID: "common" for multi-tenant, or a specific tenant UUID.
    #[serde(default = "default_microsoft_tenant")]
    pub tenant: String,
    /// OAuth scopes requested from Microsoft.
    #[serde(default = "default_outlook_scopes")]
    pub scopes: Vec<String>,
}

impl Default for OutlookOAuthConfig {
    fn default() -> Self {
        Self {
            client_id_env: default_microsoft_client_id_env(),
            client_secret_env: default_microsoft_client_secret_env(),
            tenant: default_microsoft_tenant(),
            scopes: default_outlook_scopes(),
        }
    }
}

impl OutlookOAuthConfig {
    /// Microsoft OAuth authorization endpoint for the configured tenant.
    pub fn auth_url(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
            self.tenant
        )
    }

    /// Microsoft OAuth token endpoint for the configured tenant.
    pub fn token_url(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            self.tenant
        )
    }
}

fn default_oauth_redirect_base() -> String {
    "http://localhost:8080".to_string()
}
fn default_oauth_frontend_url() -> String {
    "http://localhost:3000".to_string()
}
fn default_google_client_id_env() -> String {
    "EMAILIBRIUM_GOOGLE_CLIENT_ID".to_string()
}
fn default_google_client_secret_env() -> String {
    "EMAILIBRIUM_GOOGLE_CLIENT_SECRET".to_string()
}
fn default_gmail_scopes() -> Vec<String> {
    vec![
        "https://www.googleapis.com/auth/gmail.modify".to_string(),
        "https://www.googleapis.com/auth/gmail.labels".to_string(),
        "https://www.googleapis.com/auth/userinfo.email".to_string(),
    ]
}
fn default_google_auth_url() -> String {
    "https://accounts.google.com/o/oauth2/v2/auth".to_string()
}
fn default_google_token_url() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}
fn default_microsoft_client_id_env() -> String {
    "EMAILIBRIUM_MICROSOFT_CLIENT_ID".to_string()
}
fn default_microsoft_client_secret_env() -> String {
    "EMAILIBRIUM_MICROSOFT_CLIENT_SECRET".to_string()
}
fn default_microsoft_tenant() -> String {
    "common".to_string()
}
fn default_outlook_scopes() -> Vec<String> {
    vec![
        "Mail.ReadWrite".to_string(),
        "Mail.Send".to_string(),
        "offline_access".to_string(),
        "User.Read".to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Security config (audit items #6 CORS, #13 CSP)
// ---------------------------------------------------------------------------

/// HTTP security configuration: CORS allowed origins, CSP header toggle,
/// rate limiting, and HSTS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Origins allowed by the CORS middleware.
    #[serde(default = "default_allowed_origins")]
    pub allowed_origins: Vec<String>,
    /// Whether Content-Security-Policy and related headers are emitted.
    #[serde(default = "default_true")]
    pub csp_enabled: bool,
    /// Rate limiting settings (R-05).
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// HSTS settings (R-05).
    #[serde(default)]
    pub hsts: HstsConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_allowed_origins(),
            csp_enabled: true,
            rate_limit: RateLimitConfig::default(),
            hsts: HstsConfig::default(),
        }
    }
}

/// Rate-limiting configuration (R-05).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Sustained requests per second per IP.
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: u32,
    /// Maximum burst size (initial token count).
    #[serde(default = "default_burst_size")]
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_second: default_requests_per_second(),
            burst_size: default_burst_size(),
        }
    }
}

fn default_requests_per_second() -> u32 {
    10
}

fn default_burst_size() -> u32 {
    50
}

/// HSTS (Strict-Transport-Security) configuration (R-05).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HstsConfig {
    /// Whether the HSTS header is emitted.
    #[serde(default)]
    pub enabled: bool,
    /// `max-age` directive in seconds (default: 2 years).
    #[serde(default = "default_hsts_max_age")]
    pub max_age_secs: u64,
    /// Whether the `includeSubDomains` directive is added.
    #[serde(default = "default_true")]
    pub include_subdomains: bool,
}

impl Default for HstsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_age_secs: default_hsts_max_age(),
            include_subdomains: true,
        }
    }
}

fn default_hsts_max_age() -> u64 {
    63_072_000 // 2 years
}

fn default_allowed_origins() -> Vec<String> {
    vec![
        "http://localhost:3000".to_string(),
        "http://localhost:5173".to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generative_config_defaults() {
        let config = GenerativeConfig::default();
        assert_eq!(config.provider, "builtin");
        // Built-in LLM sub-config (ADR-021)
        assert_eq!(config.builtin.model_id, "qwen3-1.7b-q4km");
        assert_eq!(config.builtin.context_size, 2048);
        assert_eq!(config.builtin.gpu_layers, 99);
        assert_eq!(config.builtin.idle_timeout_secs, 300);
        assert_eq!(config.builtin.cache_dir, "~/.emailibrium/models/llm");
        // Ollama sub-config
        assert_eq!(config.ollama.base_url, "http://localhost:11434");
        assert_eq!(config.ollama.classification_model, "llama3.2:1b");
        assert_eq!(config.ollama.chat_model, "llama3.2:3b");
        assert_eq!(config.cloud.provider, "openai");
        assert_eq!(config.cloud.api_key_env, "EMAILIBRIUM_CLOUD_API_KEY");
        assert_eq!(config.cloud.model, "gpt-4o-mini");
        assert_eq!(config.cloud.base_url, "https://api.openai.com");
        // Gemini sub-config (audit item #29)
        assert_eq!(
            config.cloud.gemini.api_key_env,
            "EMAILIBRIUM_GEMINI_API_KEY"
        );
        assert_eq!(config.cloud.gemini.model, "gemini-2.0-flash");
        assert_eq!(
            config.cloud.gemini.base_url,
            "https://generativelanguage.googleapis.com"
        );
    }

    #[test]
    fn test_cloud_embedding_config_defaults() {
        let config = CloudEmbeddingConfig::default();
        assert_eq!(config.api_key_env, "EMAILIBRIUM_OPENAI_API_KEY");
        assert_eq!(config.model, "text-embedding-3-small");
        assert_eq!(config.base_url, "https://api.openai.com");
        assert_eq!(config.dimensions, 1536);
    }

    #[test]
    fn test_cohere_embedding_config_defaults() {
        let config = CohereEmbeddingConfig::default();
        assert_eq!(config.api_key_env, "EMAILIBRIUM_COHERE_API_KEY");
        assert_eq!(config.model, "embed-english-v3.0");
        assert_eq!(config.base_url, "https://api.cohere.com");
        assert_eq!(config.dimensions, 1024);
        assert_eq!(config.input_type, "search_document");
    }

    #[test]
    fn test_embedding_config_includes_cloud_and_cohere() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.cloud.model, "text-embedding-3-small");
        assert_eq!(config.cohere.model, "embed-english-v3.0");
    }

    #[test]
    fn test_vector_config_includes_generative() {
        let config = VectorConfig::default();
        assert_eq!(config.generative.provider, "builtin");
    }

    #[test]
    fn test_oauth_config_defaults() {
        let config = OAuthConfig::default();
        assert_eq!(config.redirect_base_url, "http://localhost:8080");
        assert_eq!(config.frontend_url, "http://localhost:3000");
        assert_eq!(config.gmail.client_id_env, "EMAILIBRIUM_GOOGLE_CLIENT_ID");
        assert_eq!(
            config.gmail.client_secret_env,
            "EMAILIBRIUM_GOOGLE_CLIENT_SECRET"
        );
        assert!(!config.gmail.scopes.is_empty());
        assert_eq!(
            config.outlook.client_id_env,
            "EMAILIBRIUM_MICROSOFT_CLIENT_ID"
        );
        assert_eq!(config.outlook.tenant, "common");
        assert!(config
            .outlook
            .scopes
            .contains(&"Mail.ReadWrite".to_string()));
        assert!(config
            .outlook
            .scopes
            .contains(&"offline_access".to_string()));
    }

    #[test]
    fn test_outlook_auth_urls() {
        let config = OutlookOAuthConfig::default();
        assert!(config.auth_url().contains("common"));
        assert!(config.token_url().contains("common"));

        let tenant_config = OutlookOAuthConfig {
            tenant: "my-tenant-id".to_string(),
            ..Default::default()
        };
        assert!(tenant_config.auth_url().contains("my-tenant-id"));
    }

    #[test]
    fn test_vector_config_includes_oauth() {
        let config = VectorConfig::default();
        assert_eq!(
            config.oauth.gmail.client_id_env,
            "EMAILIBRIUM_GOOGLE_CLIENT_ID"
        );
    }

    #[test]
    fn test_security_config_defaults() {
        let config = SecurityConfig::default();
        assert!(config.csp_enabled);
        assert_eq!(config.allowed_origins.len(), 2);
        assert!(config
            .allowed_origins
            .contains(&"http://localhost:3000".to_string()));
        assert!(config
            .allowed_origins
            .contains(&"http://localhost:5173".to_string()));
    }

    #[test]
    fn test_vector_config_includes_security() {
        let config = VectorConfig::default();
        assert!(config.security.csp_enabled);
        assert!(!config.security.allowed_origins.is_empty());
    }

    #[test]
    fn test_generative_config_deserialize_from_json() {
        let json = r#"{
            "provider": "ollama",
            "ollama": {
                "base_url": "http://my-ollama:11434",
                "classification_model": "custom-model",
                "chat_model": "custom-chat"
            }
        }"#;
        let config: GenerativeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.provider, "ollama");
        assert_eq!(config.ollama.base_url, "http://my-ollama:11434");
        assert_eq!(config.ollama.classification_model, "custom-model");
        // Cloud should use defaults since not specified.
        assert_eq!(config.cloud.provider, "openai");
        // Gemini sub-config should use defaults.
        assert_eq!(config.cloud.gemini.model, "gemini-2.0-flash");
    }

    #[test]
    fn test_redis_config_defaults() {
        let config = RedisConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.url, "redis://127.0.0.1:6379");
        assert_eq!(config.cache_ttl_secs, 3600);
    }

    #[test]
    fn test_vector_config_includes_redis() {
        let config = VectorConfig::default();
        assert!(!config.redis.enabled);
        assert_eq!(config.redis.url, "redis://127.0.0.1:6379");
    }

    // -- apply_yaml_path_defaults -------------------------------------------

    #[test]
    fn test_apply_yaml_path_defaults_overrides_when_different() {
        let mut config = VectorConfig::default();
        let paths = crate::vectors::yaml_config::PathsConfig {
            llm_cache_dir: "/custom/llm".to_string(),
            embedding_cache_dir: "/custom/embeddings".to_string(),
            vector_data_dir: "/custom/vectors".to_string(),
            database_file: "custom.db".to_string(),
        };
        config.apply_yaml_path_defaults(&paths);
        assert_eq!(config.generative.builtin.cache_dir, "/custom/llm");
        assert_eq!(
            config.embedding.onnx.cache_dir,
            Some("/custom/embeddings".to_string())
        );
        assert_eq!(config.store.path, "/custom/vectors");
        assert_eq!(config.database_url, "sqlite:custom.db?mode=rwc");
    }

    #[test]
    fn test_apply_yaml_path_defaults_noop_when_same_as_compile_defaults() {
        let mut config = VectorConfig::default();
        let paths = crate::vectors::yaml_config::PathsConfig::default();
        let orig_store_path = config.store.path.clone();
        let orig_cache_dir = config.generative.builtin.cache_dir.clone();
        let orig_db_url = config.database_url.clone();
        config.apply_yaml_path_defaults(&paths);
        assert_eq!(config.store.path, orig_store_path);
        assert_eq!(config.generative.builtin.cache_dir, orig_cache_dir);
        assert_eq!(config.database_url, orig_db_url);
        assert!(config.embedding.onnx.cache_dir.is_none());
    }
}
