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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// Path for vector data persistence.
    #[serde(default = "default_store_path")]
    pub path: String,
    /// Whether vectors are enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: default_store_path(),
            enabled: true,
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
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: default_search_limit(),
            max_limit: default_max_limit(),
            similarity_threshold: default_similarity_threshold(),
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
fn default_true() -> bool {
    true
}
/// Default embedding provider. Set to "mock" for development (deterministic hash-based).
/// Production should use "ollama" or "cloud" via config.yaml or EMAILIBRIUM_EMBEDDING_PROVIDER.
fn default_provider() -> String {
    "mock".to_string()
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
