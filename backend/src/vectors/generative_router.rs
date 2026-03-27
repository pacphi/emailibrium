//! Generative router for runtime provider switching (DDD-006: AI Providers, Audit Item #38).
//!
//! `GenerativeRouter` wraps multiple `GenerativeModel` implementations and
//! routes inference requests based on provider availability, load, and
//! configured priority. Supports automatic failover.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::error::VectorError;
use super::generative::GenerativeModel;
use super::model_registry::ProviderType;

// ---------------------------------------------------------------------------
// Router Config
// ---------------------------------------------------------------------------

/// A registered provider with its priority and model reference.
#[derive(Clone)]
pub struct RegisteredProvider {
    pub provider_type: ProviderType,
    pub model: Arc<dyn GenerativeModel>,
    /// Lower number = higher priority.
    pub priority: u8,
    /// Whether this provider is currently enabled.
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Generative router trait for testability.
#[async_trait]
pub trait GenerativeRouterService: Send + Sync {
    /// Generate text using the best available provider.
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError>;

    /// Classify text using the best available provider.
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError>;

    /// Get the currently active provider type.
    async fn active_provider(&self) -> Option<ProviderType>;

    /// Disable a provider (e.g. after repeated failures).
    async fn disable_provider(&self, provider: ProviderType);

    /// Re-enable a provider.
    async fn enable_provider(&self, provider: ProviderType);
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Runtime generative model router with failover.
pub struct GenerativeRouter {
    providers: Arc<RwLock<Vec<RegisteredProvider>>>,
}

impl GenerativeRouter {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a provider with the given priority.
    pub async fn register(
        &self,
        provider_type: ProviderType,
        model: Arc<dyn GenerativeModel>,
        priority: u8,
    ) {
        let mut providers = self.providers.write().await;
        providers.push(RegisteredProvider {
            provider_type,
            model,
            priority,
            enabled: true,
        });
        // Keep sorted by priority (lower = higher priority)
        providers.sort_by_key(|p| p.priority);
    }

    /// Get the best available provider (enabled + responding).
    async fn best_provider(&self) -> Option<RegisteredProvider> {
        let providers = self.providers.read().await;
        for provider in providers.iter() {
            if provider.enabled && provider.model.is_available().await {
                return Some(provider.clone());
            }
        }
        None
    }

    /// Get all enabled providers sorted by priority for failover.
    async fn enabled_providers(&self) -> Vec<RegisteredProvider> {
        let providers = self.providers.read().await;
        providers.iter().filter(|p| p.enabled).cloned().collect()
    }

    /// List all registered providers with their current status.
    pub async fn list_providers(&self) -> Vec<ProviderStatus> {
        let providers = self.providers.read().await;
        let mut statuses = Vec::with_capacity(providers.len());
        for p in providers.iter() {
            statuses.push(ProviderStatus {
                provider_type: p.provider_type,
                priority: p.priority,
                enabled: p.enabled,
                available: p.model.is_available().await,
            });
        }
        statuses
    }
}

/// Status snapshot for a registered provider.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    pub provider_type: ProviderType,
    pub priority: u8,
    pub enabled: bool,
    pub available: bool,
}

impl Default for GenerativeRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GenerativeRouterService for GenerativeRouter {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        let providers = self.enabled_providers().await;

        if providers.is_empty() {
            return Err(VectorError::AllProvidersUnavailable(
                "No generative providers registered or enabled".into(),
            ));
        }

        for provider in &providers {
            if !provider.model.is_available().await {
                debug!(
                    provider = %provider.provider_type,
                    "Provider unavailable, trying next"
                );
                continue;
            }

            match provider.model.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!(
                        provider = %provider.provider_type,
                        model = %provider.model.model_name(),
                        "Generation succeeded"
                    );
                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        provider = %provider.provider_type,
                        error = %e,
                        "Generation failed, trying next provider"
                    );
                    continue;
                }
            }
        }

        Err(VectorError::AllProvidersUnavailable(
            "All registered providers failed".into(),
        ))
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let providers = self.enabled_providers().await;

        if providers.is_empty() {
            return Err(VectorError::AllProvidersUnavailable(
                "No generative providers registered or enabled".into(),
            ));
        }

        for provider in &providers {
            if !provider.model.is_available().await {
                continue;
            }

            match provider.model.classify(text, categories).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!(
                        provider = %provider.provider_type,
                        error = %e,
                        "Classification failed, trying next provider"
                    );
                    continue;
                }
            }
        }

        Err(VectorError::AllProvidersUnavailable(
            "All registered providers failed classification".into(),
        ))
    }

    async fn active_provider(&self) -> Option<ProviderType> {
        self.best_provider().await.map(|p| p.provider_type)
    }

    async fn disable_provider(&self, provider: ProviderType) {
        let mut providers = self.providers.write().await;
        for p in providers.iter_mut() {
            if p.provider_type == provider {
                p.enabled = false;
            }
        }
    }

    async fn enable_provider(&self, provider: ProviderType) {
        let mut providers = self.providers.write().await;
        for p in providers.iter_mut() {
            if p.provider_type == provider {
                p.enabled = true;
            }
        }
    }
}

// Also implement GenerativeModel so the router can be used as a drop-in
// replacement for any single model.
#[async_trait]
impl GenerativeModel for GenerativeRouter {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        GenerativeRouterService::generate(self, prompt, max_tokens).await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        GenerativeRouterService::classify(self, text, categories).await
    }

    fn model_name(&self) -> &str {
        "generative-router"
    }

    async fn is_available(&self) -> bool {
        self.best_provider().await.is_some()
    }

    fn configured_max_tokens(&self) -> Option<u32> {
        // Cannot async here (sync trait method), so check providers synchronously.
        // Use try_read to avoid blocking; if lock is held, fall back to None.
        if let Ok(providers) = self.providers.try_read() {
            for p in providers.iter() {
                if p.enabled {
                    if let Some(tokens) = p.model.configured_max_tokens() {
                        return Some(tokens);
                    }
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock generative model for testing.
    struct MockModel {
        name: String,
        available: bool,
        response: String,
    }

    impl MockModel {
        fn new(name: &str, available: bool, response: &str) -> Self {
            Self {
                name: name.into(),
                available,
                response: response.into(),
            }
        }
    }

    #[async_trait]
    impl GenerativeModel for MockModel {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, VectorError> {
            if self.available {
                Ok(self.response.clone())
            } else {
                Err(VectorError::CategorizationFailed("unavailable".into()))
            }
        }

        async fn classify(&self, _text: &str, categories: &[&str]) -> Result<String, VectorError> {
            if self.available {
                Ok(categories.first().unwrap_or(&"Unknown").to_string())
            } else {
                Err(VectorError::CategorizationFailed("unavailable".into()))
            }
        }

        fn model_name(&self) -> &str {
            &self.name
        }

        async fn is_available(&self) -> bool {
            self.available
        }
    }

    #[tokio::test]
    async fn test_router_routes_to_highest_priority() {
        let router = GenerativeRouter::new();

        let model_a = Arc::new(MockModel::new("model-a", true, "response-a"));
        let model_b = Arc::new(MockModel::new("model-b", true, "response-b"));

        router
            .register(ProviderType::Ollama, model_a, 1) // higher priority
            .await;
        router.register(ProviderType::OpenAi, model_b, 2).await;

        let result = GenerativeRouterService::generate(&router, "test", 100)
            .await
            .unwrap();
        assert_eq!(result, "response-a");
    }

    #[tokio::test]
    async fn test_router_failover() {
        let router = GenerativeRouter::new();

        // Primary is unavailable
        let model_a = Arc::new(MockModel::new("model-a", false, "response-a"));
        let model_b = Arc::new(MockModel::new("model-b", true, "response-b"));

        router.register(ProviderType::Ollama, model_a, 1).await;
        router.register(ProviderType::OpenAi, model_b, 2).await;

        let result = GenerativeRouterService::generate(&router, "test", 100)
            .await
            .unwrap();
        assert_eq!(result, "response-b");
    }

    #[tokio::test]
    async fn test_router_all_unavailable() {
        let router = GenerativeRouter::new();

        let model_a = Arc::new(MockModel::new("model-a", false, ""));
        router.register(ProviderType::Ollama, model_a, 1).await;

        let result = GenerativeRouterService::generate(&router, "test", 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_router_disable_provider() {
        let router = GenerativeRouter::new();

        let model_a = Arc::new(MockModel::new("model-a", true, "response-a"));
        let model_b = Arc::new(MockModel::new("model-b", true, "response-b"));

        router.register(ProviderType::Ollama, model_a, 1).await;
        router.register(ProviderType::OpenAi, model_b, 2).await;

        // Disable the primary
        router.disable_provider(ProviderType::Ollama).await;

        let result = GenerativeRouterService::generate(&router, "test", 100)
            .await
            .unwrap();
        assert_eq!(result, "response-b");

        // Re-enable
        router.enable_provider(ProviderType::Ollama).await;
        let result = GenerativeRouterService::generate(&router, "test", 100)
            .await
            .unwrap();
        assert_eq!(result, "response-a");
    }

    #[tokio::test]
    async fn test_router_classify() {
        let router = GenerativeRouter::new();
        let model = Arc::new(MockModel::new("model-a", true, ""));

        router.register(ProviderType::Ollama, model, 1).await;

        let result =
            GenerativeRouterService::classify(&router, "test email", &["Work", "Personal"])
                .await
                .unwrap();
        assert_eq!(result, "Work");
    }

    #[tokio::test]
    async fn test_router_active_provider() {
        let router = GenerativeRouter::new();

        assert!(router.active_provider().await.is_none());

        let model = Arc::new(MockModel::new("model", true, ""));
        router.register(ProviderType::Ollama, model, 1).await;

        assert_eq!(router.active_provider().await, Some(ProviderType::Ollama));
    }

    #[tokio::test]
    async fn test_router_no_providers_error() {
        let router = GenerativeRouter::new();
        let result = GenerativeRouterService::generate(&router, "test", 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_router_as_generative_model() {
        let router = GenerativeRouter::new();
        let model = Arc::new(MockModel::new("model", true, "routed-response"));
        router.register(ProviderType::Ollama, model, 1).await;

        // Use through the GenerativeModel trait
        let gen: &dyn GenerativeModel = &router;
        assert!(gen.is_available().await);
        assert_eq!(gen.model_name(), "generative-router");
        let result = gen.generate("test", 100).await.unwrap();
        assert_eq!(result, "routed-response");
    }

    #[tokio::test]
    async fn test_list_providers_shows_status() {
        let router = GenerativeRouter::new();
        let model_a = Arc::new(MockModel::new("a", true, ""));
        let model_b = Arc::new(MockModel::new("b", false, ""));

        router.register(ProviderType::Ollama, model_a, 1).await;
        router.register(ProviderType::OpenAi, model_b, 2).await;

        let statuses = router.list_providers().await;
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].provider_type, ProviderType::Ollama);
        assert!(statuses[0].enabled);
        assert!(statuses[0].available);
        assert_eq!(statuses[1].provider_type, ProviderType::OpenAi);
        assert!(statuses[1].enabled);
        assert!(!statuses[1].available); // model_b is not available
    }

    #[tokio::test]
    async fn test_disable_removes_from_best_provider() {
        let router = GenerativeRouter::new();
        let model = Arc::new(MockModel::new("m", true, "resp"));
        router.register(ProviderType::OpenAi, model, 1).await;

        assert_eq!(router.active_provider().await, Some(ProviderType::OpenAi));

        router.disable_provider(ProviderType::OpenAi).await;
        assert_eq!(router.active_provider().await, None);

        // list_providers still shows it but disabled
        let statuses = router.list_providers().await;
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].enabled);
    }

    #[tokio::test]
    async fn test_enable_restores_provider() {
        let router = GenerativeRouter::new();
        let model = Arc::new(MockModel::new("m", true, "resp"));
        router.register(ProviderType::Anthropic, model, 1).await;

        router.disable_provider(ProviderType::Anthropic).await;
        assert!(router.active_provider().await.is_none());

        router.enable_provider(ProviderType::Anthropic).await;
        assert_eq!(
            router.active_provider().await,
            Some(ProviderType::Anthropic)
        );
    }
}
