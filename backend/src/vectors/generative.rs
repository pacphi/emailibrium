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

        None
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
// Tier 2 — Cloud provider (OpenAI / Anthropic)
// ---------------------------------------------------------------------------

/// Cloud generative model backed by OpenAI or Anthropic APIs.
pub struct CloudGenerativeModel {
    client: reqwest::Client,
    provider: String,
    api_key: String,
    model: String,
    base_url: String,
}

impl CloudGenerativeModel {
    /// Create a new cloud generative model.
    ///
    /// Reads the API key from the environment variable named in `config.api_key_env`.
    pub fn new(config: &CloudGenerativeConfig) -> Result<Self, VectorError> {
        let api_key = std::env::var(&config.api_key_env).map_err(|_| {
            VectorError::ConfigError(format!("API key env var '{}' not set", config.api_key_env))
        })?;

        Ok(Self {
            client: reqwest::Client::new(),
            provider: config.provider.clone(),
            api_key,
            model: config.model.clone(),
            base_url: config.base_url.clone(),
        })
    }
}

#[async_trait]
impl GenerativeModel for CloudGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        match self.provider.as_str() {
            "openai" => self.generate_openai(prompt, max_tokens).await,
            "anthropic" => self.generate_anthropic(prompt, max_tokens).await,
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
        &self.model
    }

    async fn is_available(&self) -> bool {
        // Cloud providers are assumed available if configured.
        // A real health check would hit a lightweight endpoint.
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
}
