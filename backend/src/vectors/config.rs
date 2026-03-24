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
    /// OAuth provider settings (DDD-005).
    #[serde(default)]
    pub oauth: OAuthConfig,
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
            oauth: OAuthConfig::default(),
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
    /// ONNX / fastembed configuration (ADR-011).
    #[serde(default)]
    pub onnx: OnnxConfig,
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

// ---------------------------------------------------------------------------
// Generative AI config (ADR-012)
// ---------------------------------------------------------------------------

/// Configuration for the generative AI subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerativeConfig {
    /// Provider selection: "none" | "ollama" | "cloud".
    #[serde(default = "default_gen_provider")]
    pub provider: String,
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
            ollama: OllamaGenerativeConfig::default(),
            cloud: CloudGenerativeConfig::default(),
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

/// Cloud generative model settings (OpenAI / Anthropic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudGenerativeConfig {
    /// Cloud provider: "openai" | "anthropic".
    #[serde(default = "default_cloud_provider")]
    pub provider: String,
    /// Name of the environment variable holding the API key.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Model identifier (e.g. "gpt-4o-mini", "claude-sonnet-4-20250514").
    #[serde(default = "default_cloud_model")]
    pub model: String,
    /// Base URL for the provider API.
    #[serde(default = "default_cloud_base_url")]
    pub base_url: String,
}

impl Default for CloudGenerativeConfig {
    fn default() -> Self {
        Self {
            provider: default_cloud_provider(),
            api_key_env: default_api_key_env(),
            model: default_cloud_model(),
            base_url: default_cloud_base_url(),
        }
    }
}

fn default_gen_provider() -> String {
    "none".to_string()
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
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            gmail: GmailOAuthConfig::default(),
            outlook: OutlookOAuthConfig::default(),
            redirect_base_url: default_oauth_redirect_base(),
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generative_config_defaults() {
        let config = GenerativeConfig::default();
        assert_eq!(config.provider, "none");
        assert_eq!(config.ollama.base_url, "http://localhost:11434");
        assert_eq!(config.ollama.classification_model, "llama3.2:1b");
        assert_eq!(config.ollama.chat_model, "llama3.2:3b");
        assert_eq!(config.cloud.provider, "openai");
        assert_eq!(config.cloud.api_key_env, "EMAILIBRIUM_CLOUD_API_KEY");
        assert_eq!(config.cloud.model, "gpt-4o-mini");
        assert_eq!(config.cloud.base_url, "https://api.openai.com");
    }

    #[test]
    fn test_vector_config_includes_generative() {
        let config = VectorConfig::default();
        assert_eq!(config.generative.provider, "none");
    }

    #[test]
    fn test_oauth_config_defaults() {
        let config = OAuthConfig::default();
        assert_eq!(config.redirect_base_url, "http://localhost:8080");
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
    }
}
