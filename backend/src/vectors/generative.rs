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
}

// ---------------------------------------------------------------------------
// Tier 0 — Rule-based classifier (no LLM)
// ---------------------------------------------------------------------------

/// Rule-based classification fallback for Tier 0 (no generative model).
/// Uses sender domain patterns and keyword heuristics.
pub struct RuleBasedClassifier;

impl RuleBasedClassifier {
    /// Attempt to classify an email using domain and keyword heuristics.
    ///
    /// Returns `None` when no rule matches.
    pub fn classify_by_rules(text: &str, from_addr: &str) -> Option<String> {
        let domain = from_addr.split('@').next_back().unwrap_or("");
        let lower = text.to_lowercase();

        // --- Domain-based rules ---
        if domain.contains("github.com") || domain.contains("gitlab.com") {
            return Some("Notification".into());
        }
        if domain.contains("linkedin.com") || domain.contains("facebook.com") {
            return Some("Social".into());
        }
        if domain.contains("amazon.com") || domain.contains("ebay.com") {
            return Some("Shopping".into());
        }
        if domain.contains("paypal.com") || domain.contains("stripe.com") {
            return Some("Finance".into());
        }
        if domain.contains("substack.com") || domain.contains("medium.com") {
            return Some("Newsletter".into());
        }
        if domain.contains("slack.com") || domain.contains("discord.com") {
            return Some("Notification".into());
        }
        if domain.contains("twitter.com")
            || domain.contains("x.com")
            || domain.contains("instagram.com")
            || domain.contains("tiktok.com")
        {
            return Some("Social".into());
        }
        if domain.contains("shopify.com") {
            return Some("Shopping".into());
        }
        if domain.contains("mint.com") || domain.contains("venmo.com") {
            return Some("Finance".into());
        }
        if domain.contains("mailchimp.com")
            || domain.contains("sendgrid.net")
            || domain.contains("constantcontact.com")
        {
            return Some("Marketing".into());
        }

        // --- Sender prefix rules ---
        let local_part = from_addr.split('@').next().unwrap_or("");
        if local_part == "noreply" || local_part == "no-reply" {
            return Some("Notification".into());
        }
        if local_part.contains("newsletter") || local_part.contains("digest") {
            return Some("Newsletter".into());
        }

        // --- Keyword-based rules ---
        if lower.contains("invoice")
            || lower.contains("receipt")
            || lower.contains("payment")
            || lower.contains("bank statement")
        {
            return Some("Finance".into());
        }
        if lower.contains("unsubscribe") || lower.contains("opt out") || lower.contains("opt-out") {
            return Some("Marketing".into());
        }
        if lower.contains("meeting")
            || lower.contains("calendar")
            || lower.contains("schedule")
            || lower.contains("standup")
        {
            return Some("Work".into());
        }
        if lower.contains("order shipped")
            || lower.contains("delivery")
            || lower.contains("tracking number")
        {
            return Some("Shopping".into());
        }
        if lower.contains("alert") || lower.contains("security notice") {
            return Some("Alerts".into());
        }
        if lower.contains("promotion") || lower.contains("% off") || lower.contains("sale ends") {
            return Some("Promotions".into());
        }
        if lower.contains("your order") || lower.contains("track your package") {
            return Some("Shopping".into());
        }
        if lower.contains("security alert")
            || lower.contains("unusual sign-in")
            || lower.contains("verify your")
        {
            return Some("Alerts".into());
        }
        if lower.contains("weekly report") || lower.contains("monthly summary") {
            return Some("Work".into());
        }
        if lower.contains("you're invited") || lower.contains("rsvp") {
            return Some("Personal".into());
        }

        None
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
}

impl OllamaGenerativeModel {
    pub fn new(config: &OllamaGenerativeConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: config.base_url.clone(),
            classification_model: config.classification_model.clone(),
            chat_model: config.chat_model.clone(),
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
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[async_trait]
impl GenerativeModel for OllamaGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = OllamaGenerateRequest {
            model: &self.chat_model,
            prompt,
            stream: false,
            options: OllamaOptions {
                num_predict: max_tokens,
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

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        let prompt = format!(
            "Classify the following email into exactly one of these categories: [{cats}].\n\
             Respond with only the category name, nothing else.\n\n\
             Email:\n{text}"
        );

        let response = self.generate(&prompt, 50).await?;
        validate_classification(&response, categories, &cats)
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
    pub fn new(config: &CloudGenerativeConfig) -> Result<Self, VectorError> {
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
        })
    }
}

#[async_trait]
impl GenerativeModel for CloudGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        match self.provider.as_str() {
            "openai" => self.generate_openai(prompt, max_tokens).await,
            "anthropic" => self.generate_anthropic(prompt, max_tokens).await,
            "gemini" => self.generate_gemini(prompt, max_tokens).await,
            other => Err(VectorError::ConfigError(format!(
                "Unknown cloud provider: {other}"
            ))),
        }
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        let prompt = format!(
            "Classify the following email into exactly one of these categories: [{cats}].\n\
             Respond with only the category name, nothing else.\n\n\
             Email:\n{text}"
        );

        let response = self.generate(&prompt, 50).await?;
        validate_classification(&response, categories, &cats)
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
    async fn generate_openai(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0.0,
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
    ) -> Result<String, VectorError> {
        let url = format!("{}/v1/messages", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": prompt}],
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
    async fn generate_gemini(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
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
                "temperature": 0.0,
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
}

impl OpenRouterGenerativeModel {
    /// Create a new OpenRouter generative model.
    ///
    /// Reads the API key from `OPENROUTER_API_KEY` env var (or the name specified
    /// in `api_key_env`). The `base_url` defaults to `https://openrouter.ai/api/v1`.
    pub fn new(
        api_key_env: &str,
        model: &str,
        base_url: &str,
        extra_headers: std::collections::HashMap<String, String>,
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
        })
    }

    /// Send a chat completion request using the OpenAI-compatible format.
    async fn generate_openai_compat(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, VectorError> {
        let url = format!("{}/chat/completions", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0.0,
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
        self.generate_openai_compat(prompt, max_tokens).await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats = categories.join(", ");
        let prompt = format!(
            "Classify the following email into exactly one of these categories: [{cats}].\n\
             Respond with only the category name, nothing else.\n\n\
             Email:\n{text}"
        );

        let response = self.generate(&prompt, 50).await?;
        validate_classification(&response, categories, &cats)
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
}
