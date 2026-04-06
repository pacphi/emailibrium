//! YAML configuration loader for project-level config files.
//!
//! Loads the 6 YAML config files from `config/` at startup and provides
//! typed access to prompts, tuning, classification, app settings, and
//! model catalogs.  All fields use `#[serde(default)]` so the app works
//! even if a YAML file is missing or incomplete.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Top-level aggregate
// ---------------------------------------------------------------------------

/// Holds all parsed YAML configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct YamlConfig {
    #[serde(default)]
    pub prompts: PromptsConfig,
    #[serde(default)]
    pub tuning: TuningConfig,
    #[serde(default)]
    pub classification: ClassificationConfig,
    #[serde(default)]
    pub app: AppConfig,
    #[serde(default)]
    pub llm_catalog: LlmCatalog,
    #[serde(default)]
    pub embedding_catalog: EmbeddingCatalog,
}

// ---------------------------------------------------------------------------
// prompts.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsConfig {
    #[serde(default = "default_chat_assistant")]
    pub chat_assistant: String,
    #[serde(default = "default_email_classification")]
    pub email_classification: String,
    #[serde(default = "default_email_classification_user")]
    pub email_classification_user: String,
    #[serde(default = "default_email_classification_json")]
    pub email_classification_json: String,
    #[serde(default = "default_email_classification_batch")]
    pub email_classification_batch: String,
}

fn default_chat_assistant() -> String {
    "You are an email assistant with full access to the user's inbox. \
     The [Email Context] section contains REAL emails from the user's inbox that match their query. \
     CRITICAL RULES:\n\
     1. Base ALL answers ONLY on the emails provided in [Email Context].\n\
     2. For EVERY factual claim, cite the email by subject line or sender name.\n\
     3. If emails are shown in the context, list them with sender, subject, and date.\n\
     4. NEVER fabricate email content, senders, dates, or subjects.\n\
     5. If no emails match or context is insufficient, say: \"I could not find this in your emails.\"\n\
     6. Be specific — quote subjects and senders from the provided emails.\n\
     7. Do NOT include internal reasoning or thinking in your response. Answer directly.\n\
     8. Use the current date provided in this prompt to answer time-relative questions."
        .to_string()
}

fn default_email_classification() -> String {
    "You are an email classifier. Respond with ONLY the category name, nothing else.".to_string()
}

fn default_email_classification_user() -> String {
    "Classify this email into one of: {{categories}}\n\nEmail: {{email_text}}".to_string()
}

fn default_email_classification_json() -> String {
    "You are an email classification assistant.\n\
     Classify the following email into exactly one of the provided categories.\n\
     Respond ONLY with valid JSON matching the schema."
        .to_string()
}

fn default_email_classification_batch() -> String {
    "Classify each of the following emails into exactly one of: {{categories}}\n\
     Respond with exactly {{count}} category names, one per line, nothing else."
        .to_string()
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            chat_assistant: default_chat_assistant(),
            email_classification: default_email_classification(),
            email_classification_user: default_email_classification_user(),
            email_classification_json: default_email_classification_json(),
            email_classification_batch: default_email_classification_batch(),
        }
    }
}

// ---------------------------------------------------------------------------
// tuning.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuningConfig {
    #[serde(default)]
    pub llm: LlmTuning,
    #[serde(default)]
    pub rag: RagTuning,
    #[serde(default)]
    pub chat: ChatTuning,
    #[serde(default)]
    pub ingestion: IngestionTuning,
    #[serde(default)]
    pub clustering: ClusteringTuning,
    #[serde(default)]
    pub error_recovery: ErrorRecoveryTuning,
    #[serde(default)]
    pub repetition: RepetitionTuning,
    #[serde(default)]
    pub rrf: RrfConfig,
    #[serde(default)]
    pub reranking: RerankingConfig,
    /// FTS5 scoring configuration (ADR-029).
    #[serde(default)]
    pub fts5: Fts5Config,
    /// Context building configuration (ADR-029).
    #[serde(default)]
    pub context: ContextConfig,
    /// Extractive passage configuration (ADR-029).
    #[serde(default)]
    pub extractive: super::extractive::ExtractiveConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmTuning {
    #[serde(default = "default_0_7")]
    pub default_temperature: f32,
    #[serde(default = "default_0_1")]
    pub classification_temperature: f32,
    #[serde(default = "default_512_usize")]
    pub default_max_tokens: usize,
    #[serde(default = "default_200_usize")]
    pub classification_max_tokens: usize,
    #[serde(default = "default_0_9")]
    pub top_p: f32,
    #[serde(default = "default_1_1")]
    pub repeat_penalty: f32,
    #[serde(default = "default_2048_usize")]
    pub default_context_size: usize,
    #[serde(default = "default_2048_usize")]
    pub chat_max_tokens: usize,
    #[serde(default = "default_300_u64")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_120_u64")]
    pub low_ram_idle_timeout_secs: u64,
    #[serde(default = "default_8_usize")]
    pub low_ram_threshold_gb: usize,
    #[serde(default = "default_1_2")]
    pub memory_safety_margin: f32,
    #[serde(default = "default_0_8")]
    pub memory_warning_threshold: f32,
    #[serde(default = "default_30_u64")]
    pub memory_monitor_interval_secs: u64,
}

impl Default for LlmTuning {
    fn default() -> Self {
        Self {
            default_temperature: 0.7,
            classification_temperature: 0.1,
            default_max_tokens: 512,
            classification_max_tokens: 200,
            top_p: 0.9,
            repeat_penalty: 1.1,
            default_context_size: 2048,
            chat_max_tokens: 2048,
            idle_timeout_secs: 300,
            low_ram_idle_timeout_secs: 120,
            low_ram_threshold_gb: 8,
            memory_safety_margin: 1.2,
            memory_warning_threshold: 0.8,
            memory_monitor_interval_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagTuning {
    #[serde(default = "default_3_usize")]
    pub top_k: usize,
    #[serde(default = "default_0_005")]
    pub min_relevance_score: f32,
    #[serde(default = "default_500_usize")]
    pub max_context_tokens: usize,
    #[serde(default = "default_true")]
    pub include_body: bool,
    #[serde(default = "default_200_usize")]
    pub max_body_chars: usize,
    #[serde(default = "default_656_usize")]
    pub overhead_tokens: usize,
}

impl Default for RagTuning {
    fn default() -> Self {
        Self {
            top_k: 3,
            min_relevance_score: 0.005,
            max_context_tokens: 500,
            include_body: true,
            max_body_chars: 200,
            overhead_tokens: 656,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTuning {
    #[serde(default = "default_3600_u64")]
    pub session_ttl_secs: u64,
    #[serde(default = "default_20_usize")]
    pub max_history_messages: usize,
}

impl Default for ChatTuning {
    fn default() -> Self {
        Self {
            session_ttl_secs: 3600,
            max_history_messages: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionTuning {
    #[serde(default = "default_64_usize")]
    pub embedding_batch_size: usize,
    #[serde(default = "default_50_usize")]
    pub min_cluster_emails: usize,
    #[serde(default = "default_10_usize")]
    pub sidecar_write_interval: usize,
    #[serde(default = "default_32_usize")]
    pub pipeline_channel_buffer: usize,
    #[serde(default = "default_true")]
    pub onboarding_mode: bool,
    #[serde(default = "default_10_usize")]
    pub backfill_batch_size: usize,
    #[serde(default = "default_8_usize")]
    pub backfill_concurrency: usize,
    #[serde(default = "default_50_u64")]
    pub backfill_delay_between_ms: u64,
}

impl Default for IngestionTuning {
    fn default() -> Self {
        Self {
            embedding_batch_size: 64,
            min_cluster_emails: 50,
            sidecar_write_interval: 10,
            pipeline_channel_buffer: 32,
            onboarding_mode: true,
            backfill_batch_size: 10,
            backfill_concurrency: 8,
            backfill_delay_between_ms: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringTuning {
    #[serde(default = "default_20_usize")]
    pub tfidf_max_terms: usize,
    #[serde(default = "default_5_usize")]
    pub representative_emails: usize,
    #[serde(default = "default_5_usize")]
    pub min_k: usize,
    #[serde(default = "default_10_usize")]
    pub max_k: usize,
    #[serde(default = "default_50_usize")]
    pub recluster_threshold: usize,
    /// Max points sampled for silhouette score estimation (ADR-021).
    /// Reduced from 3000 to 500: O(n²) → O(500²) regardless of dataset size.
    #[serde(default = "default_500_usize")]
    pub silhouette_sample_size: usize,
    /// KMeans iterations during K-detection probing (ADR-021).
    /// Reduced from 50 to 15: Yale research shows convergence in 2-4 iterations.
    #[serde(default = "default_15_usize")]
    pub kmeans_probe_iters: usize,
    /// KMeans iterations for final clustering (ADR-021).
    /// Reduced from 100 to 30: sufficient with KMeans++ initialization.
    #[serde(default = "default_30_usize")]
    pub kmeans_final_iters: usize,
}

impl Default for ClusteringTuning {
    fn default() -> Self {
        Self {
            tfidf_max_terms: 20,
            representative_emails: 5,
            min_k: 5,
            max_k: 10,
            recluster_threshold: 50,
            silhouette_sample_size: 500,
            kmeans_probe_iters: 15,
            kmeans_final_iters: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecoveryTuning {
    #[serde(default = "default_2_usize")]
    pub max_retries: usize,
    #[serde(default = "default_1000_u64")]
    pub retry_delay_ms: u64,
}

impl Default for ErrorRecoveryTuning {
    fn default() -> Self {
        Self {
            max_retries: 2,
            retry_delay_ms: 1000,
        }
    }
}

// ---------------------------------------------------------------------------
// RRF fusion config (ADR-029)
// ---------------------------------------------------------------------------

/// Configuration for Reciprocal Rank Fusion weights and k parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RrfConfig {
    /// Default k parameter for RRF scoring.
    #[serde(default = "default_rrf_k")]
    pub default_k: u32,
    /// Per-query-type adaptive weights.
    #[serde(default)]
    pub adaptive_weights: AdaptiveWeights,
}

fn default_rrf_k() -> u32 {
    40
}

impl Default for RrfConfig {
    fn default() -> Self {
        Self {
            default_k: 40,
            adaptive_weights: AdaptiveWeights::default(),
        }
    }
}

/// Per-query-type fusion weights for RRF (ADR-029 section 3.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveWeights {
    #[serde(default = "default_factual_weights")]
    pub factual: FusionWeights,
    #[serde(default = "default_needle_weights")]
    pub needle: FusionWeights,
    #[serde(default = "default_temporal_weights")]
    pub temporal: FusionWeights,
    #[serde(default = "default_semantic_weights")]
    pub semantic: FusionWeights,
    #[serde(default = "default_boolean_weights")]
    pub boolean: FusionWeights,
}

impl Default for AdaptiveWeights {
    fn default() -> Self {
        Self {
            factual: default_factual_weights(),
            needle: default_needle_weights(),
            temporal: default_temporal_weights(),
            semantic: default_semantic_weights(),
            boolean: default_boolean_weights(),
        }
    }
}

/// FTS and vector weight pair for a single query type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionWeights {
    pub fts: f32,
    pub vector: f32,
}

fn default_factual_weights() -> FusionWeights {
    FusionWeights {
        fts: 1.0,
        vector: 1.0,
    }
}
fn default_needle_weights() -> FusionWeights {
    FusionWeights {
        fts: 1.5,
        vector: 0.5,
    }
}
fn default_temporal_weights() -> FusionWeights {
    FusionWeights {
        fts: 0.8,
        vector: 1.2,
    }
}
fn default_semantic_weights() -> FusionWeights {
    FusionWeights {
        fts: 0.7,
        vector: 1.3,
    }
}
fn default_boolean_weights() -> FusionWeights {
    FusionWeights {
        fts: 1.3,
        vector: 0.7,
    }
}

// ---------------------------------------------------------------------------
// Re-ranking config (ADR-029)
// ---------------------------------------------------------------------------

/// Configuration for the cross-encoder re-ranking stage (tuning.yaml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankingConfig {
    /// Whether re-ranking is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Model identifier.
    #[serde(default = "default_reranker_model")]
    pub model: String,
    /// Number of RRF candidates to feed into the re-ranker.
    #[serde(default = "default_50_usize")]
    pub candidates: usize,
    /// Number of results to return after re-ranking.
    #[serde(default = "default_10_usize")]
    pub top_k: usize,
    /// Timeout in milliseconds; skip re-ranking if exceeded.
    #[serde(default = "default_100_u64")]
    pub timeout_ms: u64,
}

fn default_reranker_model() -> String {
    "BAAI/bge-reranker-base".to_string()
}

fn default_100_u64() -> u64 {
    100
}

impl Default for RerankingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_reranker_model(),
            candidates: 50,
            top_k: 10,
            timeout_ms: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// FTS5 scoring config (ADR-029)
// ---------------------------------------------------------------------------

/// FTS5 scoring weights for BM25 ranking (ADR-029).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fts5Config {
    /// Positional BM25 column weights: id, subject, from_name, from_addr, body_text, labels.
    #[serde(default = "default_fts5_column_weights")]
    pub column_weights: Vec<f32>,
}

fn default_fts5_column_weights() -> Vec<f32> {
    vec![0.0, 10.0, 5.0, 3.0, 1.0, 2.0]
}

impl Default for Fts5Config {
    fn default() -> Self {
        Self {
            column_weights: default_fts5_column_weights(),
        }
    }
}

// ---------------------------------------------------------------------------
// Context building config (ADR-029)
// ---------------------------------------------------------------------------

/// Context building configuration (ADR-029).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Characters of email body to include in embedding text.
    #[serde(default = "default_1500_usize")]
    pub embedding_body_budget: usize,
    /// Minimum top-result RRF score to consider context sufficient.
    #[serde(default = "default_context_threshold")]
    pub context_sufficiency_threshold: f32,
}

fn default_1500_usize() -> usize {
    1500
}

fn default_context_threshold() -> f32 {
    0.01
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            embedding_body_budget: 1500,
            context_sufficiency_threshold: 0.01,
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepetitionTuning {
    #[serde(default = "default_8_usize")]
    pub token_window: usize,
    #[serde(default = "default_4_usize")]
    pub token_repeat_threshold: usize,
    #[serde(default = "default_100_usize")]
    pub phrase_check_length: usize,
    #[serde(default = "default_200_usize")]
    pub phrase_check_after: usize,
}

impl Default for RepetitionTuning {
    fn default() -> Self {
        Self {
            token_window: 8,
            token_repeat_threshold: 4,
            phrase_check_length: 100,
            phrase_check_after: 200,
        }
    }
}

// ---------------------------------------------------------------------------
// classification.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationConfig {
    #[serde(default = "default_categories")]
    pub categories: Vec<String>,
    #[serde(default)]
    pub domain_rules: Vec<DomainRule>,
    #[serde(default)]
    pub keyword_rules: Vec<KeywordRule>,
}

fn default_categories() -> Vec<String> {
    vec![
        "Work".to_string(),
        "Personal".to_string(),
        "Finance".to_string(),
        "Shopping".to_string(),
        "Social".to_string(),
        "Newsletter".to_string(),
        "Marketing".to_string(),
        "Notification".to_string(),
        "Alerts".to_string(),
        "Promotions".to_string(),
        "Travel".to_string(),
        "Uncategorized".to_string(),
    ]
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            categories: default_categories(),
            domain_rules: default_domain_rules(),
            keyword_rules: default_keyword_rules(),
        }
    }
}

fn default_domain_rules() -> Vec<DomainRule> {
    vec![
        DomainRule {
            domains: vec![
                "github.com".into(),
                "gitlab.com".into(),
                "bitbucket.org".into(),
            ],
            category: "Notification".into(),
        },
        DomainRule {
            domains: vec![
                "slack.com".into(),
                "discord.com".into(),
                "teams.microsoft.com".into(),
            ],
            category: "Notification".into(),
        },
        DomainRule {
            domains: vec![
                "linkedin.com".into(),
                "facebook.com".into(),
                "twitter.com".into(),
                "instagram.com".into(),
            ],
            category: "Social".into(),
        },
        DomainRule {
            domains: vec![
                "paypal.com".into(),
                "venmo.com".into(),
                "stripe.com".into(),
                "square.com".into(),
            ],
            category: "Finance".into(),
        },
        DomainRule {
            domains: vec![
                "amazon.com".into(),
                "ebay.com".into(),
                "shopify.com".into(),
                "etsy.com".into(),
            ],
            category: "Shopping".into(),
        },
        DomainRule {
            domains: vec![
                "booking.com".into(),
                "airbnb.com".into(),
                "expedia.com".into(),
                "kayak.com".into(),
            ],
            category: "Travel".into(),
        },
    ]
}

fn default_keyword_rules() -> Vec<KeywordRule> {
    vec![
        KeywordRule {
            keywords: vec![
                "invoice".into(),
                "receipt".into(),
                "payment".into(),
                "billing".into(),
                "statement".into(),
            ],
            category: "Finance".into(),
        },
        KeywordRule {
            keywords: vec![
                "sale".into(),
                "discount".into(),
                "offer".into(),
                "promo".into(),
                "coupon".into(),
                "deal".into(),
            ],
            category: "Marketing".into(),
        },
        KeywordRule {
            keywords: vec![
                "meeting".into(),
                "calendar".into(),
                "schedule".into(),
                "appointment".into(),
            ],
            category: "Work".into(),
        },
        KeywordRule {
            keywords: vec![
                "newsletter".into(),
                "digest".into(),
                "weekly update".into(),
                "monthly roundup".into(),
            ],
            category: "Newsletter".into(),
        },
        KeywordRule {
            keywords: vec![
                "shipped".into(),
                "delivery".into(),
                "tracking".into(),
                "order confirmed".into(),
            ],
            category: "Shopping".into(),
        },
        KeywordRule {
            keywords: vec![
                "flight".into(),
                "hotel".into(),
                "reservation".into(),
                "itinerary".into(),
                "boarding pass".into(),
            ],
            category: "Travel".into(),
        },
        KeywordRule {
            keywords: vec!["unsubscribe".into()],
            category: "Marketing".into(),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainRule {
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordRule {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub category: String,
}

// ---------------------------------------------------------------------------
// app.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub security: AppSecurityConfig,
    #[serde(default)]
    pub hardware: HardwareConfig,
    #[serde(default)]
    pub email: EmailConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    #[serde(default = "default_30_u32")]
    pub trash_retention_days: u32,
    #[serde(default = "default_30_u32")]
    pub spam_retention_days: u32,
    #[serde(default = "default_true")]
    pub skip_trash_embedding: bool,
    #[serde(default = "default_true")]
    pub skip_spam_embedding: bool,
    #[serde(default = "default_inbox_str")]
    pub default_folder_filter: String,
    /// How often (in hours) the background label repair job runs.
    /// Set to 0 to disable. Default: 6 hours.
    #[serde(default = "default_6_u32")]
    pub label_repair_interval_hours: u32,
}

fn default_30_u32() -> u32 {
    30
}
fn default_6_u32() -> u32 {
    6
}
fn default_inbox_str() -> String {
    "INBOX".to_string()
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            trash_retention_days: 30,
            spam_retention_days: 30,
            skip_trash_embedding: true,
            skip_spam_embedding: true,
            default_folder_filter: "INBOX".to_string(),
            label_repair_interval_hours: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_15_u64")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_5_u64")]
    pub default_sync_frequency_minutes: u64,
    #[serde(default = "default_2_usize")]
    pub sync_completion_stable_checks: usize,
    #[serde(default = "default_3000_u64")]
    pub sync_completion_check_interval_ms: u64,
    #[serde(default = "default_120_usize")]
    pub max_sync_wait_polls: usize,
    /// Delay in milliseconds between successive page fetches during email sync.
    /// Throttles requests to stay within provider rate limits (e.g. Gmail ~250 queries/min).
    /// Default: 200ms.
    #[serde(default = "default_200_u64")]
    pub fetch_page_delay_ms: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 15,
            default_sync_frequency_minutes: 5,
            sync_completion_stable_checks: 2,
            sync_completion_check_interval_ms: 3000,
            max_sync_wait_polls: 120,
            fetch_page_delay_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_30000_u64")]
    pub default_stale_time_ms: u64,
    #[serde(default = "default_1_usize")]
    pub default_retry_count: usize,

    // Per-feature overrides (consumed by frontend via /ai/config/app)
    #[serde(default = "default_10000_u64")]
    pub email_counts_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub email_counts_refetch_interval_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub categories_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub labels_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub chat_sessions_stale_time_ms: u64,
    #[serde(default = "default_60000_u64")]
    pub subscriptions_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub ollama_models_stale_time_ms: u64,
    #[serde(default = "default_60000_u64")]
    pub model_catalog_stale_time_ms: u64,
    #[serde(default = "default_10000_u64")]
    pub clusters_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub clusters_refetch_interval_ms: u64,
    #[serde(default = "default_3000_u64")]
    pub clusters_active_stale_time_ms: u64,
    #[serde(default = "default_5000_u64")]
    pub clusters_active_refetch_interval_ms: u64,
    #[serde(default = "default_5000_u64")]
    pub clustering_status_stale_time_ms: u64,
    #[serde(default = "default_10000_u64")]
    pub clustering_status_refetch_interval_ms: u64,
    #[serde(default = "default_10000_u64")]
    pub dashboard_accounts_refetch_interval_ms: u64,
    #[serde(default = "default_10000_u64")]
    pub dashboard_embedding_refetch_interval_ms: u64,
    #[serde(default = "default_5000_u64")]
    pub embedding_active_refetch_interval_ms: u64,
    #[serde(default = "default_3000_u64")]
    pub ingestion_active_refetch_interval_ms: u64,
    #[serde(default = "default_2000_u64")]
    pub ingestion_active_stale_time_ms: u64,
    #[serde(default = "default_30000_u64")]
    pub stats_refetch_interval_ms: u64,
    #[serde(default = "default_5000_u64")]
    pub stats_active_refetch_interval_ms: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default_stale_time_ms: 30000,
            default_retry_count: 1,
            email_counts_stale_time_ms: 10000,
            email_counts_refetch_interval_ms: 30000,
            categories_stale_time_ms: 30000,
            labels_stale_time_ms: 30000,
            chat_sessions_stale_time_ms: 30000,
            subscriptions_stale_time_ms: 60000,
            ollama_models_stale_time_ms: 30000,
            model_catalog_stale_time_ms: 60000,
            clusters_stale_time_ms: 10000,
            clusters_refetch_interval_ms: 30000,
            clusters_active_stale_time_ms: 3000,
            clusters_active_refetch_interval_ms: 5000,
            clustering_status_stale_time_ms: 5000,
            clustering_status_refetch_interval_ms: 10000,
            dashboard_accounts_refetch_interval_ms: 10000,
            dashboard_embedding_refetch_interval_ms: 10000,
            embedding_active_refetch_interval_ms: 5000,
            ingestion_active_refetch_interval_ms: 3000,
            ingestion_active_stale_time_ms: 2000,
            stats_refetch_interval_ms: 30000,
            stats_active_refetch_interval_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_3000_u64")]
    pub ollama_fetch_timeout_ms: u64,
    #[serde(default = "default_3000_u64")]
    pub model_catalog_fetch_timeout_ms: u64,
    #[serde(default = "default_300000_u64")]
    pub ingestion_start_timeout_ms: u64,
    #[serde(default = "default_300000_u64")]
    pub recluster_timeout_ms: u64,
    #[serde(default = "default_60000_u64")]
    pub reembed_timeout_ms: u64,
    #[serde(default = "default_2000_u64")]
    pub model_switch_poll_interval_ms: u64,
    #[serde(default = "default_150_usize")]
    pub model_switch_max_polls: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            ollama_fetch_timeout_ms: 3000,
            model_catalog_fetch_timeout_ms: 3000,
            ingestion_start_timeout_ms: 300000,
            recluster_timeout_ms: 300000,
            reembed_timeout_ms: 60000,
            model_switch_poll_interval_ms: 2000,
            model_switch_max_polls: 150,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_system_str")]
    pub theme: String,
    #[serde(default = "default_left_str")]
    pub sidebar_position: String,
    #[serde(default = "default_14_usize")]
    pub font_size_px: usize,
    #[serde(default = "default_comfortable_str")]
    pub email_density: String,
    #[serde(default = "default_90_usize")]
    pub data_retention_days: usize,
    #[serde(default = "default_true")]
    pub sona_learning_enabled: bool,
    #[serde(default = "default_0_5")]
    pub learning_rate_sensitivity: f32,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            sidebar_position: "left".to_string(),
            font_size_px: 14,
            email_density: "comfortable".to_string(),
            data_retention_days: 90,
            sona_learning_enabled: true,
            learning_rate_sensitivity: 0.5,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub ollama: OllamaProviderConfig,
    #[serde(default)]
    pub openai: ApiKeyProviderConfig,
    #[serde(default)]
    pub anthropic: ApiKeyProviderConfig,
    #[serde(default)]
    pub openrouter: OpenRouterProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaProviderConfig {
    #[serde(default = "default_ollama_url")]
    pub base_url: String,
}

impl Default for OllamaProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeyProviderConfig {
    #[serde(default)]
    pub api_key_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterProviderConfig {
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default = "default_openrouter_url")]
    pub base_url: String,
    #[serde(default)]
    pub required_headers: HashMap<String, String>,
}

fn default_openrouter_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

impl Default for OpenRouterProviderConfig {
    fn default() -> Self {
        Self {
            api_key_env: String::new(),
            base_url: default_openrouter_url(),
            required_headers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_llm_cache_dir")]
    pub llm_cache_dir: String,
    #[serde(default = "default_embedding_cache_dir")]
    pub embedding_cache_dir: String,
    #[serde(default = "default_vector_data_dir")]
    pub vector_data_dir: String,
    #[serde(default = "default_database_file")]
    pub database_file: String,
}

fn default_llm_cache_dir() -> String {
    "~/.emailibrium/models/llm".to_string()
}

fn default_embedding_cache_dir() -> String {
    ".fastembed_cache".to_string()
}

fn default_vector_data_dir() -> String {
    "data/vectors".to_string()
}

fn default_database_file() -> String {
    "emailibrium.db".to_string()
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            llm_cache_dir: default_llm_cache_dir(),
            embedding_cache_dir: default_embedding_cache_dir(),
            vector_data_dir: default_vector_data_dir(),
            database_file: default_database_file(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSecurityConfig {
    #[serde(default)]
    pub jwt_secret_env: String,
    #[serde(default)]
    pub encryption_key_env: String,
    #[serde(default = "default_60_usize")]
    pub rate_limit_capacity: usize,
    #[serde(default = "default_1_0")]
    pub rate_limit_refill_per_sec: f64,
    #[serde(default = "default_63072000_u64")]
    pub hsts_max_age_secs: u64,
}

impl Default for AppSecurityConfig {
    fn default() -> Self {
        Self {
            jwt_secret_env: String::new(),
            encryption_key_env: String::new(),
            rate_limit_capacity: 500,
            rate_limit_refill_per_sec: 20.0,
            hsts_max_age_secs: 63072000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareConfig {
    #[serde(default)]
    pub backend_priority: Vec<String>,
    #[serde(default = "default_4096_usize")]
    pub os_overhead_mb: usize,
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            backend_priority: vec![
                "metal".to_string(),
                "cuda".to_string(),
                "vulkan".to_string(),
                "cpu".to_string(),
            ],
            os_overhead_mb: 4096,
        }
    }
}

// ---------------------------------------------------------------------------
// models-llm.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmCatalog {
    #[serde(default)]
    pub providers: HashMap<String, LlmProviderCatalog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderCatalog {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub download_required: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api_format: Option<String>,
    #[serde(default)]
    pub required_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub models: Vec<LlmModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModelEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub params: Option<String>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub context_size: u32,
    #[serde(default)]
    pub disk_mb: Option<u32>,
    #[serde(default)]
    pub min_ram_mb: Option<u32>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub chat_template: Option<String>,
    #[serde(default)]
    pub rag_capable: bool,
    #[serde(default)]
    pub default_for_ram_mb: Option<u32>,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub ollama_tag: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub cost_per_1m_input: Option<f64>,
    #[serde(default)]
    pub cost_per_1m_output: Option<f64>,
    #[serde(default)]
    pub tuning: Option<ModelTuning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTuning {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub repeat_penalty: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// models-embedding.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingCatalog {
    #[serde(default)]
    pub providers: HashMap<String, EmbeddingProviderCatalog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingProviderCatalog {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub download_required: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub models: Vec<EmbeddingModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fastembed_variant: Option<String>,
    #[serde(default)]
    pub fastembed_quantized: Option<String>,
    #[serde(default)]
    pub dimensions: u32,
    #[serde(default)]
    pub max_tokens: u32,
    #[serde(default)]
    pub disk_mb: Option<u32>,
    #[serde(default)]
    pub min_ram_mb: Option<u32>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "default", default)]
    pub is_default: bool,
    #[serde(default)]
    pub ollama_tag: Option<String>,
    #[serde(default)]
    pub cost_per_1m_tokens: Option<f64>,
}

// ---------------------------------------------------------------------------
// Default value functions
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

fn default_0_7() -> f32 {
    0.7
}
fn default_0_1() -> f32 {
    0.1
}
fn default_0_9() -> f32 {
    0.9
}
fn default_1_1() -> f32 {
    1.1
}
fn default_0_8() -> f32 {
    0.8
}
fn default_1_2() -> f32 {
    1.2
}
fn default_0_5() -> f32 {
    0.5
}
fn default_0_005() -> f32 {
    0.005
}
fn default_1_0() -> f64 {
    1.0
}

fn default_1_usize() -> usize {
    1
}
fn default_2_usize() -> usize {
    2
}
fn default_3_usize() -> usize {
    3
}
fn default_4_usize() -> usize {
    4
}
fn default_5_usize() -> usize {
    5
}
fn default_8_usize() -> usize {
    8
}
fn default_10_usize() -> usize {
    10
}
fn default_14_usize() -> usize {
    14
}
fn default_20_usize() -> usize {
    20
}
fn default_50_usize() -> usize {
    50
}
fn default_60_usize() -> usize {
    60
}
fn default_64_usize() -> usize {
    64
}
fn default_90_usize() -> usize {
    90
}
fn default_100_usize() -> usize {
    100
}
fn default_120_usize() -> usize {
    120
}
fn default_15_usize() -> usize {
    15
}
fn default_30_usize() -> usize {
    30
}
fn default_32_usize() -> usize {
    32
}
fn default_150_usize() -> usize {
    150
}
fn default_200_usize() -> usize {
    200
}
fn default_500_usize() -> usize {
    500
}
fn default_512_usize() -> usize {
    512
}
fn default_656_usize() -> usize {
    656
}
fn default_2048_usize() -> usize {
    2048
}
fn default_4096_usize() -> usize {
    4096
}

fn default_5_u64() -> u64 {
    5
}
fn default_15_u64() -> u64 {
    15
}
fn default_30_u64() -> u64 {
    30
}
fn default_50_u64() -> u64 {
    50
}
fn default_120_u64() -> u64 {
    120
}
fn default_200_u64() -> u64 {
    200
}
fn default_300_u64() -> u64 {
    300
}
fn default_1000_u64() -> u64 {
    1000
}
fn default_2000_u64() -> u64 {
    2000
}
fn default_3000_u64() -> u64 {
    3000
}
fn default_3600_u64() -> u64 {
    3600
}
fn default_5000_u64() -> u64 {
    5000
}
fn default_10000_u64() -> u64 {
    10000
}
fn default_30000_u64() -> u64 {
    30000
}
fn default_60000_u64() -> u64 {
    60000
}
fn default_300000_u64() -> u64 {
    300000
}
fn default_63072000_u64() -> u64 {
    63072000
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_system_str() -> String {
    "system".to_string()
}
fn default_left_str() -> String {
    "left".to_string()
}
fn default_comfortable_str() -> String {
    "comfortable".to_string()
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load all 6 YAML config files from `config_dir` and return a merged `YamlConfig`.
///
/// Missing files are silently skipped (defaults are used).
/// Parse errors produce warnings but do not crash the app.
pub fn load_yaml_config(config_dir: &str) -> Result<YamlConfig, anyhow::Error> {
    let dir = Path::new(config_dir);

    let prompts = load_file::<PromptsConfig>(dir, "prompts.yaml");
    let tuning = load_file::<TuningConfig>(dir, "tuning.yaml");
    let classification = load_file::<ClassificationConfig>(dir, "classification.yaml");
    let app = load_file::<AppConfig>(dir, "app.yaml");
    let llm_catalog = load_file::<LlmCatalog>(dir, "models-llm.yaml");
    let embedding_catalog = load_file::<EmbeddingCatalog>(dir, "models-embedding.yaml");

    Ok(YamlConfig {
        prompts,
        tuning,
        classification,
        app,
        llm_catalog,
        embedding_catalog,
    })
}

/// Load a single YAML file, returning `Default` on missing/invalid files.
fn load_file<T: Default + serde::de::DeserializeOwned>(dir: &Path, filename: &str) -> T {
    let path = dir.join(filename);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_yaml::from_str::<T>(&contents) {
            Ok(parsed) => {
                tracing::info!("Loaded YAML config: {}", path.display());
                parsed
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {e} — using defaults", path.display());
                T::default()
            }
        },
        Err(_) => {
            tracing::debug!("YAML config not found: {} — using defaults", path.display());
            T::default()
        }
    }
}
