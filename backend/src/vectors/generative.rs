//! Generative AI models for classification fallback and chat (ADR-012).
//!
//! Provides a trait-based generative model abstraction with tiered implementations:
//! - **Tier 0**: `RuleBasedClassifier` — keyword/domain heuristics, no LLM needed
//! - **Tier 1**: `OllamaGenerativeModel` — local Ollama inference
//! - **Tier 2**: `CloudGenerativeModel` — cloud provider (OpenAI / Anthropic)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::config::{CloudGenerativeConfig, OllamaGenerativeConfig};
use super::error::VectorError;
use super::yaml_config::{ClassificationConfig, LlmTuning, ModelTuning, PromptsConfig};

// ---------------------------------------------------------------------------
// Generation parameters (resolved from YAML config)
// ---------------------------------------------------------------------------

/// Resolved generation parameters for a model instance.
///
/// Built by merging per-model tuning (from `models-llm.yaml`) over global
/// defaults (from `tuning.yaml`).  Every generative model stores one of these
/// at construction time so that no hardcoded values remain in hot paths.
#[derive(Debug, Clone)]
pub struct GenerationParams {
    /// Temperature for free-form / chat generation.
    pub temperature: f32,
    /// Temperature for classification calls (low for accuracy).
    pub classification_temperature: f32,
    /// Max tokens for classification output.
    pub classification_max_tokens: u32,
    /// Nucleus sampling threshold.
    pub top_p: f32,
    /// Repetition penalty (1.0 = none).
    pub repeat_penalty: f32,
}

impl GenerationParams {
    /// Resolve generation parameters by overlaying per-model tuning on top of
    /// global `LlmTuning` defaults.
    pub fn resolve(global: &LlmTuning, per_model: Option<&ModelTuning>) -> Self {
        let temperature = per_model
            .and_then(|t| t.temperature)
            .unwrap_or(global.default_temperature);
        let top_p = per_model
            .and_then(|t| t.top_p)
            .unwrap_or(global.top_p);
        let repeat_penalty = per_model
            .and_then(|t| t.repeat_penalty)
            .unwrap_or(global.repeat_penalty);

        Self {
            temperature,
            classification_temperature: global.classification_temperature,
            classification_max_tokens: global.classification_max_tokens as u32,
            top_p,
            repeat_penalty,
        }
    }
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self::resolve(&LlmTuning::default(), None)
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Abstraction over generative text models so the classification and chat
/// subsystems stay provider-agnostic.
#[async_trait]
pub trait GenerativeModel: Send + Sync {
    /// Generate free-form text from a prompt.
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError>;

    /// Classify text into exactly one of the given categories.
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError>;

    /// Return the underlying model name.
    fn model_name(&self) -> &str;

    /// Check whether the model backend is reachable.
    async fn is_available(&self) -> bool;

    /// Classify multiple texts in a single LLM call, returning one category per text.
    ///
    /// The default implementation falls back to individual `classify()` calls.
    /// Providers override this to send a single batched prompt for efficiency.
    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.classify(text, categories).await?);
        }
        Ok(results)
    }

    /// Return the model's configured max response tokens (from per-model tuning),
    /// or `None` to fall back to the global `chat_max_tokens` default.
    fn configured_max_tokens(&self) -> Option<u32> {
        None
    }
}

// ---------------------------------------------------------------------------
// Tier 0 — Rule-based classifier (no LLM)
// ---------------------------------------------------------------------------

/// Rule-based classification fallback for Tier 0 (no generative model).
/// Uses domain and keyword rules loaded from `config/classification.yaml`.
pub struct RuleBasedClassifier;

impl RuleBasedClassifier {
    /// Attempt to classify an email using config-driven domain and keyword rules.
    ///
    /// Rules are read from `ClassificationConfig` (loaded from `config/classification.yaml`).
    /// Returns `None` when no rule matches.
    pub fn classify_by_rules_with_config(
        text: &str,
        from_addr: &str,
        config: &ClassificationConfig,
    ) -> Option<String> {
        let domain = from_addr.split('@').next_back().unwrap_or("");
        let lower = text.to_lowercase();

        // --- Config-driven domain rules ---
        for rule in &config.domain_rules {
            for rule_domain in &rule.domains {
                if domain.contains(rule_domain.as_str()) {
                    return Some(rule.category.clone());
                }
            }
        }

        // --- Sender prefix rules (built-in, not configurable) ---
        let local_part = from_addr.split('@').next().unwrap_or("");
        if local_part == "noreply" || local_part == "no-reply" {
            return Some("Notification".into());
        }
        if local_part.contains("newsletter") || local_part.contains("digest") {
            return Some("Newsletter".into());
        }

        // --- Config-driven keyword rules ---
        for rule in &config.keyword_rules {
            for keyword in &rule.keywords {
                if lower.contains(&keyword.to_lowercase()) {
                    return Some(rule.category.clone());
                }
            }
        }

        None
    }

    /// Legacy entry point that uses default `ClassificationConfig`.
    ///
    /// Prefer `classify_by_rules_with_config` when config is available.
    #[allow(dead_code)]
    pub fn classify_by_rules(text: &str, from_addr: &str) -> Option<String> {
        Self::classify_by_rules_with_config(text, from_addr, &ClassificationConfig::default())
    }

    /// Map Gmail built-in category labels to EmailCategory names.
    /// Called from the labels aggregation endpoint; will also be wired into sync.
    #[allow(dead_code)]
    pub fn category_from_gmail_label(label: &str) -> Option<String> {
        match label {
            "CATEGORY_SOCIAL" => Some("Social".into()),
            "CATEGORY_PROMOTIONS" => Some("Promotions".into()),
            "CATEGORY_UPDATES" => Some("Notification".into()),
            "CATEGORY_FORUMS" => Some("Social".into()),
            "CATEGORY_PERSONAL" => Some("Personal".into()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tier 1 — Ollama (local)
// ---------------------------------------------------------------------------

/// Local generative model backed by Ollama.
pub struct OllamaGenerativeModel {
    client: reqwest::Client,
    base_url: String,
    classification_model: String,
    chat_model: String,
    /// Resolved generation parameters from YAML config.
    params: GenerationParams,
    /// Classification prompts loaded from `config/prompts.yaml`.
    prompts: PromptsConfig,
}

impl OllamaGenerativeModel {
    #[allow(dead_code)]
    pub fn new(config: &OllamaGenerativeConfig) -> Self {
        Self::with_params(config, GenerationParams::default())
    }

    /// Create a new Ollama model with resolved generation parameters.
    #[allow(dead_code)]
    pub fn with_params(config: &OllamaGenerativeConfig, params: GenerationParams) -> Self {
        Self::with_params_and_prompts(config, params, PromptsConfig::default())
    }

    /// Create with explicit generation parameters and prompts configuration from YAML.
    pub fn with_params_and_prompts(
        config: &OllamaGenerativeConfig,
        params: GenerationParams,
        prompts: PromptsConfig,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: config.base_url.clone(),
            classification_model: config.classification_model.clone(),
            chat_model: config.chat_model.clone(),
            params,
            prompts,
        }
    }
}

#[derive(Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f32,
    top_p: f32,
    repeat_penalty: f32,
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

impl OllamaGenerativeModel {
    /// Internal generation helper that accepts an explicit temperature override.
    async fn generate_internal(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = OllamaGenerateRequest {
            model: &self.chat_model,
            prompt,
            stream: false,
            options: OllamaOptions {
                num_predict: max_tokens,
                temperature,
                top_p: self.params.top_p,
                repeat_penalty: self.params.repeat_penalty,
            },
        };

        debug!(model = %self.chat_model, "Ollama generate request");

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                VectorError::CategorizationFailed(format!("Ollama request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::CategorizationFailed(format!(
                "Ollama returned {status}: {text}"
            )));
        }

        let parsed: OllamaGenerateResponse = resp
            .json()
            .await
            .map_err(|e| VectorError::CategorizationFailed(format!("Ollama parse error: {e}")))?;

        Ok(parsed.response)
    }
}

#[async_trait]
impl GenerativeModel for OllamaGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        self.generate_internal(prompt, max_tokens, self.params.temperature)
            .await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        // Build prompt from YAML config: system + user prompt with variable substitution.
        let system = self.prompts.email_classification.trim();
        let user = self
            .prompts
            .email_classification_user
            .replace("{{categories}}", &cats)
            .replace("{{email_text}}", text);
        let prompt = format!("{system}\n\n{user}");

        // Use classification-specific temperature and max tokens from config
        let response = self
            .generate_internal(
                &prompt,
                self.params.classification_max_tokens,
                self.params.classification_temperature,
            )
            .await?;
        validate_classification(&response, categories, &cats)
    }

    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        if texts.len() <= 1 {
            // Not worth batching a single email.
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.classify(text, categories).await?);
            }
            return Ok(results);
        }

        let prompt = build_batch_prompt(&self.prompts, texts, categories);
        let max_tokens = self.params.classification_max_tokens * texts.len() as u32;

        let response = self
            .generate_internal(&prompt, max_tokens, self.params.classification_temperature)
            .await?;

        let parsed = parse_batch_response(&response, texts.len(), categories);
        // Collect results; for any parse failures, fall back to individual classify.
        let mut results = Vec::with_capacity(texts.len());
        for (i, r) in parsed.into_iter().enumerate() {
            match r {
                Ok(cat) => results.push(cat),
                Err(_) => {
                    debug!(index = i, "Batch parse failed, falling back to individual classify");
                    results.push(self.classify(texts[i], categories).await?);
                }
            }
        }
        Ok(results)
    }

    fn model_name(&self) -> &str {
        &self.classification_model
    }

    async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }
}

// ---------------------------------------------------------------------------
// Tier 2 — Cloud provider (OpenAI / Anthropic / Gemini)
// ---------------------------------------------------------------------------

/// Cloud generative model backed by OpenAI, Anthropic, or Gemini APIs.
#[derive(Debug)]
pub struct CloudGenerativeModel {
    client: reqwest::Client,
    provider: String,
    api_key: String,
    model: String,
    base_url: String,
    /// Gemini-specific config for when provider == "gemini".
    gemini_config: Option<GeminiResolvedConfig>,
    /// Resolved generation parameters from YAML config.
    params: GenerationParams,
    /// Classification prompts loaded from `config/prompts.yaml`.
    prompts: PromptsConfig,
}

/// Resolved Gemini configuration (API key already read from env).
#[derive(Debug)]
struct GeminiResolvedConfig {
    api_key: String,
    model: String,
    base_url: String,
}

impl CloudGenerativeModel {
    /// Create a new cloud generative model.
    ///
    /// Reads the API key from the environment variable named in `config.api_key_env`.
    /// For Gemini, reads from `config.gemini.api_key_env` instead.
    #[allow(dead_code)]
    pub fn new(config: &CloudGenerativeConfig) -> Result<Self, VectorError> {
        Self::with_params(config, GenerationParams::default())
    }

    /// Create a new cloud generative model with resolved generation parameters.
    #[allow(dead_code)]
    pub fn with_params(
        config: &CloudGenerativeConfig,
        params: GenerationParams,
    ) -> Result<Self, VectorError> {
        Self::with_params_and_prompts(config, params, PromptsConfig::default())
    }

    /// Create with explicit generation parameters and prompts configuration from YAML.
    pub fn with_params_and_prompts(
        config: &CloudGenerativeConfig,
        params: GenerationParams,
        prompts: PromptsConfig,
    ) -> Result<Self, VectorError> {
        let (api_key, gemini_config) = if config.provider == "gemini" {
            let key = std::env::var(&config.gemini.api_key_env).map_err(|_| {
                VectorError::ConfigError(format!(
                    "Gemini API key env var '{}' not set",
                    config.gemini.api_key_env
                ))
            })?;
            let gc = GeminiResolvedConfig {
                api_key: key.clone(),
                model: config.gemini.model.clone(),
                base_url: config.gemini.base_url.clone(),
            };
            (key, Some(gc))
        } else {
            let key = std::env::var(&config.api_key_env).map_err(|_| {
                VectorError::ConfigError(format!(
                    "API key env var '{}' not set",
                    config.api_key_env
                ))
            })?;
            (key, None)
        };

        Ok(Self {
            client: reqwest::Client::new(),
            provider: config.provider.clone(),
            api_key,
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            gemini_config,
            params,
            prompts,
        })
    }
}

#[async_trait]
impl GenerativeModel for CloudGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        self.generate_internal(prompt, max_tokens, self.params.temperature)
            .await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        let system = self.prompts.email_classification.trim();
        let user = self
            .prompts
            .email_classification_user
            .replace("{{categories}}", &cats)
            .replace("{{email_text}}", text);
        let prompt = format!("{system}\n\n{user}");

        // Use classification-specific temperature and max tokens from config
        let response = self
            .generate_internal(
                &prompt,
                self.params.classification_max_tokens,
                self.params.classification_temperature,
            )
            .await?;
        validate_classification(&response, categories, &cats)
    }

    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        if texts.len() <= 1 {
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.classify(text, categories).await?);
            }
            return Ok(results);
        }

        let prompt = build_batch_prompt(&self.prompts, texts, categories);
        let max_tokens = self.params.classification_max_tokens * texts.len() as u32;

        let response = self
            .generate_internal(&prompt, max_tokens, self.params.classification_temperature)
            .await?;

        let parsed = parse_batch_response(&response, texts.len(), categories);
        let mut results = Vec::with_capacity(texts.len());
        for (i, r) in parsed.into_iter().enumerate() {
            match r {
                Ok(cat) => results.push(cat),
                Err(_) => {
                    debug!(index = i, "Batch parse failed, falling back to individual classify");
                    results.push(self.classify(texts[i], categories).await?);
                }
            }
        }
        Ok(results)
    }

    fn model_name(&self) -> &str {
        if let Some(ref gc) = self.gemini_config {
            &gc.model
        } else {
            &self.model
        }
    }

    async fn is_available(&self) -> bool {
        // Cloud providers are assumed available if configured.
        !self.api_key.is_empty()
    }
}

impl CloudGenerativeModel {
    /// Internal dispatch that accepts an explicit temperature for the request.
    async fn generate_internal(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        match self.provider.as_str() {
            "openai" => self.generate_openai(prompt, max_tokens, temperature).await,
            "anthropic" => self.generate_anthropic(prompt, max_tokens, temperature).await,
            "gemini" => self.generate_gemini(prompt, max_tokens, temperature).await,
            other => Err(VectorError::ConfigError(format!(
                "Unknown cloud provider: {other}"
            ))),
        }
    }

    async fn generate_openai(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "top_p": self.params.top_p,
        });

        debug!(model = %self.model, provider = "openai", "Cloud generate request");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                VectorError::CategorizationFailed(format!("OpenAI request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::CategorizationFailed(format!(
                "OpenAI returned {status}: {text}"
            )));
        }

        let parsed: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VectorError::CategorizationFailed(format!("OpenAI parse error: {e}")))?;

        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                VectorError::CategorizationFailed("OpenAI response missing content".into())
            })
    }

    async fn generate_anthropic(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let url = format!("{}/v1/messages", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": temperature,
            "top_p": self.params.top_p,
        });

        debug!(model = %self.model, provider = "anthropic", "Cloud generate request");

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                VectorError::CategorizationFailed(format!("Anthropic request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::CategorizationFailed(format!(
                "Anthropic returned {status}: {text}"
            )));
        }

        let parsed: serde_json::Value = resp.json().await.map_err(|e| {
            VectorError::CategorizationFailed(format!("Anthropic parse error: {e}"))
        })?;

        parsed["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                VectorError::CategorizationFailed("Anthropic response missing content".into())
            })
    }

    /// Generate text via the Google Gemini REST API (audit item #29).
    ///
    /// Uses the `generateContent` endpoint with the model from `gemini_config`.
    /// Supports `gemini-2.0-flash`, `gemini-2.5-pro`, and other Gemini model IDs.
    async fn generate_gemini(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let gc = self
            .gemini_config
            .as_ref()
            .ok_or_else(|| VectorError::ConfigError("Gemini config not initialised".to_string()))?;

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            gc.base_url, gc.model, gc.api_key
        );

        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "maxOutputTokens": max_tokens,
                "temperature": temperature,
                "topP": self.params.top_p,
            }
        });

        debug!(model = %gc.model, provider = "gemini", "Cloud generate request");

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                VectorError::CategorizationFailed(format!("Gemini request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::CategorizationFailed(format!(
                "Gemini returned {status}: {text}"
            )));
        }

        let parsed: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VectorError::CategorizationFailed(format!("Gemini parse error: {e}")))?;

        parsed["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                VectorError::CategorizationFailed("Gemini response missing content".into())
            })
    }
}

// ---------------------------------------------------------------------------
// Tier 2b — OpenRouter (OpenAI-compatible cloud proxy)
// ---------------------------------------------------------------------------

/// Cloud generative model backed by OpenRouter (OpenAI-compatible API with extra headers).
///
/// OpenRouter provides access to 300+ models through a single endpoint using the
/// OpenAI chat completions format. It requires an API key and additional headers
/// (`HTTP-Referer`, `X-Title`) for attribution.
#[derive(Debug)]
pub struct OpenRouterGenerativeModel {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    /// Extra headers required by OpenRouter (HTTP-Referer, X-Title).
    extra_headers: std::collections::HashMap<String, String>,
    /// Resolved generation parameters from YAML config.
    params: GenerationParams,
    /// Classification prompts loaded from `config/prompts.yaml`.
    prompts: PromptsConfig,
}

impl OpenRouterGenerativeModel {
    /// Create a new OpenRouter generative model.
    ///
    /// Reads the API key from `OPENROUTER_API_KEY` env var (or the name specified
    /// in `api_key_env`). The `base_url` defaults to `https://openrouter.ai/api/v1`.
    #[allow(dead_code)]
    pub fn new(
        api_key_env: &str,
        model: &str,
        base_url: &str,
        extra_headers: std::collections::HashMap<String, String>,
    ) -> Result<Self, VectorError> {
        Self::with_params(api_key_env, model, base_url, extra_headers, GenerationParams::default())
    }

    /// Create a new OpenRouter generative model with resolved generation parameters.
    #[allow(dead_code)]
    pub fn with_params(
        api_key_env: &str,
        model: &str,
        base_url: &str,
        extra_headers: std::collections::HashMap<String, String>,
        params: GenerationParams,
    ) -> Result<Self, VectorError> {
        Self::with_params_and_prompts(api_key_env, model, base_url, extra_headers, params, PromptsConfig::default())
    }

    /// Create with explicit generation parameters and prompts configuration from YAML.
    pub fn with_params_and_prompts(
        api_key_env: &str,
        model: &str,
        base_url: &str,
        extra_headers: std::collections::HashMap<String, String>,
        params: GenerationParams,
        prompts: PromptsConfig,
    ) -> Result<Self, VectorError> {
        let env_name = if api_key_env.is_empty() {
            "OPENROUTER_API_KEY"
        } else {
            api_key_env
        };
        let api_key = std::env::var(env_name).map_err(|_| {
            VectorError::ConfigError(format!("OpenRouter API key env var '{env_name}' not set"))
        })?;

        let resolved_base = if base_url.is_empty() {
            "https://openrouter.ai/api/v1"
        } else {
            base_url
        };

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.to_string(),
            base_url: resolved_base.to_string(),
            extra_headers,
            params,
            prompts,
        })
    }

    /// Send a chat completion request using the OpenAI-compatible format.
    async fn generate_openai_compat(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "top_p": self.params.top_p,
        });

        debug!(model = %self.model, provider = "openrouter", "OpenRouter generate request");

        let mut request = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        // Add OpenRouter-specific headers (HTTP-Referer, X-Title)
        for (key, value) in &self.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let resp = request.json(&body).send().await.map_err(|e| {
            VectorError::CategorizationFailed(format!("OpenRouter request failed: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(VectorError::CategorizationFailed(format!(
                "OpenRouter returned {status}: {text}"
            )));
        }

        let parsed: serde_json::Value = resp.json().await.map_err(|e| {
            VectorError::CategorizationFailed(format!("OpenRouter parse error: {e}"))
        })?;

        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                VectorError::CategorizationFailed("OpenRouter response missing content".into())
            })
    }
}

#[async_trait]
impl GenerativeModel for OpenRouterGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        self.generate_openai_compat(prompt, max_tokens, self.params.temperature)
            .await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        let system = self.prompts.email_classification.trim();
        let user = self
            .prompts
            .email_classification_user
            .replace("{{categories}}", &cats)
            .replace("{{email_text}}", text);
        let prompt = format!("{system}\n\n{user}");

        // Use classification-specific temperature and max tokens from config
        let response = self
            .generate_openai_compat(
                &prompt,
                self.params.classification_max_tokens,
                self.params.classification_temperature,
            )
            .await?;
        validate_classification(&response, categories, &cats)
    }

    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        if texts.len() <= 1 {
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.classify(text, categories).await?);
            }
            return Ok(results);
        }

        let prompt = build_batch_prompt(&self.prompts, texts, categories);
        let max_tokens = self.params.classification_max_tokens * texts.len() as u32;

        let response = self
            .generate_openai_compat(&prompt, max_tokens, self.params.classification_temperature)
            .await?;

        let parsed = parse_batch_response(&response, texts.len(), categories);
        let mut results = Vec::with_capacity(texts.len());
        for (i, r) in parsed.into_iter().enumerate() {
            match r {
                Ok(cat) => results.push(cat),
                Err(_) => {
                    debug!(index = i, "Batch parse failed, falling back to individual classify");
                    results.push(self.classify(texts[i], categories).await?);
                }
            }
        }
        Ok(results)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a batch classification prompt from the prompt template, texts, and categories.
///
/// The prompt includes the system classification instruction followed by the
/// batch user template with numbered emails separated by "---".
pub(crate) fn build_batch_prompt(prompts: &PromptsConfig, texts: &[&str], categories: &[&str]) -> String {
    let cats = categories.join(", ");
    let system = prompts.email_classification.trim();
    let user = prompts
        .email_classification_batch
        .replace("{{categories}}", &cats)
        .replace("{{count}}", &texts.len().to_string());

    let mut prompt = format!("{system}\n\n{user}\n\n");
    for (i, text) in texts.iter().enumerate() {
        if i > 0 {
            prompt.push_str("---\n");
        }
        prompt.push_str(&format!("Email {}:\n{}\n", i + 1, text));
    }
    prompt
}

/// Parse a batch classification response into individual category results.
///
/// Expects one category per line. Lines that don't match any known category
/// are returned as errors for that position; valid lines are returned as `Ok`.
pub(crate) fn parse_batch_response(
    response: &str,
    expected_count: usize,
    categories: &[&str],
) -> Vec<Result<String, VectorError>> {
    let cats_display = categories.join(", ");
    let lines: Vec<&str> = response
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut results = Vec::with_capacity(expected_count);
    for i in 0..expected_count {
        if let Some(line) = lines.get(i) {
            // Try exact match (case-insensitive)
            let mut matched = false;
            for cat in categories {
                if line.eq_ignore_ascii_case(cat) {
                    results.push(Ok(cat.to_string()));
                    matched = true;
                    break;
                }
            }
            if !matched {
                // Try fuzzy: check if line contains a category name
                let mut fuzzy_matched = false;
                for cat in categories {
                    if line.to_lowercase().contains(&cat.to_lowercase()) {
                        results.push(Ok(cat.to_string()));
                        fuzzy_matched = true;
                        break;
                    }
                }
                if !fuzzy_matched {
                    warn!(
                        line = *line,
                        index = i,
                        "Batch classification line didn't match any category"
                    );
                    results.push(Err(VectorError::CategorizationFailed(format!(
                        "Batch line {}: '{}' not one of: {}",
                        i + 1,
                        line,
                        cats_display
                    ))));
                }
            }
        } else {
            results.push(Err(VectorError::CategorizationFailed(format!(
                "Batch response missing line {} (expected {})",
                i + 1,
                expected_count
            ))));
        }
    }
    results
}

/// Validate that an LLM response matches one of the expected categories.
fn validate_classification(
    response: &str,
    categories: &[&str],
    cats_display: &str,
) -> Result<String, VectorError> {
    let trimmed = response.trim();
    for cat in categories {
        if trimmed.eq_ignore_ascii_case(cat) {
            return Ok(cat.to_string());
        }
    }
    warn!(
        response = trimmed,
        categories = cats_display,
        "LLM returned unexpected category"
    );
    Err(VectorError::CategorizationFailed(format!(
        "LLM returned '{trimmed}', not one of: {cats_display}"
    )))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_based_classifier_github() {
        let result =
            RuleBasedClassifier::classify_by_rules("New pull request opened", "noreply@github.com");
        assert_eq!(result, Some("Notification".to_string()));
    }

    #[test]
    fn test_rule_based_classifier_invoice() {
        let result = RuleBasedClassifier::classify_by_rules(
            "Your invoice for March is ready",
            "billing@example.com",
        );
        assert_eq!(result, Some("Finance".to_string()));
    }

    #[test]
    fn test_rule_based_classifier_unknown() {
        let result = RuleBasedClassifier::classify_by_rules(
            "Hello, how are you doing today?",
            "friend@personal.com",
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_rule_based_classifier_marketing() {
        let result = RuleBasedClassifier::classify_by_rules(
            "Click here to unsubscribe from our mailing list",
            "news@company.com",
        );
        assert_eq!(result, Some("Marketing".to_string()));
    }

    #[test]
    fn test_rule_based_classifier_work() {
        let result = RuleBasedClassifier::classify_by_rules(
            "Team meeting scheduled for tomorrow at 2pm",
            "boss@company.com",
        );
        assert_eq!(result, Some("Work".to_string()));
    }

    #[test]
    fn test_rule_based_classifier_social_domain() {
        let result = RuleBasedClassifier::classify_by_rules(
            "You have a new connection",
            "noreply@linkedin.com",
        );
        assert_eq!(result, Some("Social".to_string()));
    }

    #[test]
    fn test_rule_based_classifier_shopping_domain() {
        let result = RuleBasedClassifier::classify_by_rules(
            "Your order has been placed",
            "order@amazon.com",
        );
        assert_eq!(result, Some("Shopping".to_string()));
    }

    #[test]
    fn test_validate_classification_match() {
        let result = validate_classification(
            "Finance",
            &["Work", "Finance", "Personal"],
            "Work, Finance, Personal",
        );
        assert_eq!(result.unwrap(), "Finance");
    }

    #[test]
    fn test_validate_classification_case_insensitive() {
        let result = validate_classification("  finance  ", &["Work", "Finance"], "Work, Finance");
        assert_eq!(result.unwrap(), "Finance");
    }

    #[test]
    fn test_validate_classification_no_match() {
        let result = validate_classification("Unknown", &["Work", "Finance"], "Work, Finance");
        assert!(result.is_err());
    }

    // -- CloudGenerativeModel tests (Gemini, audit item #29) ----------------

    #[test]
    fn test_gemini_provider_requires_api_key() {
        std::env::remove_var("EMAILIBRIUM_GEMINI_API_KEY");
        let config = crate::vectors::config::CloudGenerativeConfig {
            provider: "gemini".to_string(),
            ..Default::default()
        };
        let result = CloudGenerativeModel::new(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("EMAILIBRIUM_GEMINI_API_KEY"),
            "expected Gemini env var name in error, got: {err_msg}"
        );
    }

    #[test]
    fn test_gemini_provider_creates_with_valid_key() {
        std::env::set_var("__TEST_GEMINI_KEY", "AIza-test-key");
        let config = crate::vectors::config::CloudGenerativeConfig {
            provider: "gemini".to_string(),
            gemini: crate::vectors::config::GeminiGenerativeConfig {
                api_key_env: "__TEST_GEMINI_KEY".to_string(),
                model: "gemini-2.0-flash".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let model = CloudGenerativeModel::new(&config).unwrap();
        assert_eq!(model.model_name(), "gemini-2.0-flash");
        assert_eq!(model.provider, "gemini");
        assert!(model.gemini_config.is_some());
        std::env::remove_var("__TEST_GEMINI_KEY");
    }

    #[tokio::test]
    async fn test_gemini_provider_is_available() {
        std::env::set_var("__TEST_GEMINI_AVAIL", "AIza-test");
        let config = crate::vectors::config::CloudGenerativeConfig {
            provider: "gemini".to_string(),
            gemini: crate::vectors::config::GeminiGenerativeConfig {
                api_key_env: "__TEST_GEMINI_AVAIL".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let model = CloudGenerativeModel::new(&config).unwrap();
        assert!(model.is_available().await);
        std::env::remove_var("__TEST_GEMINI_AVAIL");
    }

    #[test]
    fn test_openai_provider_still_works() {
        std::env::set_var("__TEST_OPENAI_KEY", "sk-test");
        let config = crate::vectors::config::CloudGenerativeConfig {
            provider: "openai".to_string(),
            api_key_env: "__TEST_OPENAI_KEY".to_string(),
            ..Default::default()
        };
        let model = CloudGenerativeModel::new(&config).unwrap();
        assert_eq!(model.model_name(), "gpt-4o-mini");
        assert!(model.gemini_config.is_none());
        std::env::remove_var("__TEST_OPENAI_KEY");
    }

    #[test]
    fn test_anthropic_provider_still_works() {
        std::env::set_var("__TEST_ANTHRO_KEY", "sk-ant-test");
        let config = crate::vectors::config::CloudGenerativeConfig {
            provider: "anthropic".to_string(),
            api_key_env: "__TEST_ANTHRO_KEY".to_string(),
            ..Default::default()
        };
        let model = CloudGenerativeModel::new(&config).unwrap();
        assert_eq!(model.provider, "anthropic");
        assert!(model.gemini_config.is_none());
        std::env::remove_var("__TEST_ANTHRO_KEY");
    }

    // -- OpenRouterGenerativeModel tests ------------------------------------

    #[test]
    fn test_openrouter_requires_api_key() {
        std::env::remove_var("__TEST_OR_KEY_MISSING");
        let result = OpenRouterGenerativeModel::new(
            "__TEST_OR_KEY_MISSING",
            "openai/gpt-4.1-mini",
            "https://openrouter.ai/api/v1",
            std::collections::HashMap::new(),
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("__TEST_OR_KEY_MISSING"),
            "expected env var name in error, got: {err_msg}"
        );
    }

    #[test]
    fn test_openrouter_creates_with_valid_key() {
        std::env::set_var("__TEST_OR_KEY", "sk-or-test-key");
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "HTTP-Referer".to_string(),
            "https://emailibrium.app".to_string(),
        );
        headers.insert("X-Title".to_string(), "Emailibrium".to_string());

        let model = OpenRouterGenerativeModel::new(
            "__TEST_OR_KEY",
            "openai/gpt-4.1-mini",
            "https://openrouter.ai/api/v1",
            headers,
        )
        .unwrap();
        assert_eq!(model.model_name(), "openai/gpt-4.1-mini");
        assert_eq!(model.extra_headers.len(), 2);
        std::env::remove_var("__TEST_OR_KEY");
    }

    #[test]
    fn test_openrouter_default_base_url() {
        std::env::set_var("__TEST_OR_KEY2", "sk-or-test");
        let model = OpenRouterGenerativeModel::new(
            "__TEST_OR_KEY2",
            "qwen/qwen3-32b",
            "",
            std::collections::HashMap::new(),
        )
        .unwrap();
        assert_eq!(model.base_url, "https://openrouter.ai/api/v1");
        std::env::remove_var("__TEST_OR_KEY2");
    }

    #[tokio::test]
    async fn test_openrouter_is_available() {
        std::env::set_var("__TEST_OR_AVAIL", "sk-or-available");
        let model = OpenRouterGenerativeModel::new(
            "__TEST_OR_AVAIL",
            "openai/gpt-4.1-mini",
            "https://openrouter.ai/api/v1",
            std::collections::HashMap::new(),
        )
        .unwrap();
        assert!(model.is_available().await);
        std::env::remove_var("__TEST_OR_AVAIL");
    }

    #[test]
    fn test_openrouter_default_env_var_name() {
        std::env::set_var("OPENROUTER_API_KEY", "sk-or-default-env");
        let model = OpenRouterGenerativeModel::new(
            "",
            "meta-llama/llama-4-scout",
            "",
            std::collections::HashMap::new(),
        )
        .unwrap();
        assert_eq!(model.model_name(), "meta-llama/llama-4-scout");
        std::env::remove_var("OPENROUTER_API_KEY");
    }

    // -- Batch classification helper tests -----------------------------------

    #[test]
    fn test_build_batch_prompt_structure() {
        let prompts = PromptsConfig::default();
        let texts = &["Hello from HR", "Your invoice is ready"];
        let categories = &["Work", "Finance", "Personal"];

        let prompt = build_batch_prompt(&prompts, texts, categories);
        assert!(prompt.contains("Work, Finance, Personal"));
        assert!(prompt.contains("Email 1:"));
        assert!(prompt.contains("Email 2:"));
        assert!(prompt.contains("Hello from HR"));
        assert!(prompt.contains("Your invoice is ready"));
        assert!(prompt.contains("---"));
        assert!(prompt.contains("2 category names"));
    }

    #[test]
    fn test_parse_batch_response_exact_match() {
        let response = "Work\nFinance\nPersonal";
        let categories = &["Work", "Finance", "Personal"];
        let results = parse_batch_response(response, 3, categories);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert_eq!(results[1].as_ref().unwrap(), "Finance");
        assert_eq!(results[2].as_ref().unwrap(), "Personal");
    }

    #[test]
    fn test_parse_batch_response_case_insensitive() {
        let response = "work\nFINANCE";
        let categories = &["Work", "Finance"];
        let results = parse_batch_response(response, 2, categories);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert_eq!(results[1].as_ref().unwrap(), "Finance");
    }

    #[test]
    fn test_parse_batch_response_fuzzy_match() {
        let response = "Category: Work\nThis is Finance";
        let categories = &["Work", "Finance"];
        let results = parse_batch_response(response, 2, categories);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert_eq!(results[1].as_ref().unwrap(), "Finance");
    }

    #[test]
    fn test_parse_batch_response_missing_lines() {
        let response = "Work";
        let categories = &["Work", "Finance"];
        let results = parse_batch_response(response, 2, categories);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert!(results[1].is_err());
    }

    #[test]
    fn test_parse_batch_response_invalid_category() {
        let response = "Work\nUnknownCategory\nFinance";
        let categories = &["Work", "Finance", "Personal"];
        let results = parse_batch_response(response, 3, categories);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert!(results[1].is_err());
        assert_eq!(results[2].as_ref().unwrap(), "Finance");
    }

    #[test]
    fn test_parse_batch_response_empty_lines_filtered() {
        let response = "Work\n\n\nFinance\n";
        let categories = &["Work", "Finance"];
        let results = parse_batch_response(response, 2, categories);
        assert_eq!(results[0].as_ref().unwrap(), "Work");
        assert_eq!(results[1].as_ref().unwrap(), "Finance");
    }
}
