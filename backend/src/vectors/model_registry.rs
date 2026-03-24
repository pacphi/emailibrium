//! Model registry with lifecycle management (DDD-006: AI Providers, Audit Item #38).
//!
//! `ModelRegistry` tracks AI models through their lifecycle states:
//! Available -> Downloading -> Downloaded -> Verifying -> Verified -> Active
//! Any state can transition to Quarantined on failure.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lifecycle state of a model in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// Known but not yet downloaded.
    Available,
    /// Currently being downloaded.
    Downloading,
    /// Downloaded to local storage.
    Downloaded,
    /// Integrity verification in progress.
    Verifying,
    /// Verified and ready for use.
    Verified,
    /// Currently loaded and serving inference.
    Active,
    /// Quarantined due to integrity failure or runtime errors.
    Quarantined,
}

impl std::fmt::Display for ModelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => write!(f, "available"),
            Self::Downloading => write!(f, "downloading"),
            Self::Downloaded => write!(f, "downloaded"),
            Self::Verifying => write!(f, "verifying"),
            Self::Verified => write!(f, "verified"),
            Self::Active => write!(f, "active"),
            Self::Quarantined => write!(f, "quarantined"),
        }
    }
}

/// Provider type enum replacing raw strings (Audit Item #38).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    /// ONNX Runtime (local, default).
    Onnx,
    /// Ollama local inference server.
    Ollama,
    /// OpenAI cloud API.
    OpenAi,
    /// Anthropic cloud API.
    Anthropic,
    /// Google Gemini cloud API.
    Gemini,
    /// Rule-based (no model needed).
    RuleBased,
    /// Disabled / none.
    None,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Onnx => write!(f, "onnx"),
            Self::Ollama => write!(f, "ollama"),
            Self::OpenAi => write!(f, "openai"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Gemini => write!(f, "gemini"),
            Self::RuleBased => write!(f, "rule_based"),
            Self::None => write!(f, "none"),
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "onnx" => Ok(Self::Onnx),
            "ollama" => Ok(Self::Ollama),
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            "cloud" => Ok(Self::OpenAi), // backward compat
            "rule_based" | "rules" => Ok(Self::RuleBased),
            "none" | "disabled" | "" => Ok(Self::None),
            other => Err(format!("Unknown provider type: {other}")),
        }
    }
}

/// A registered model entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub provider: ProviderType,
    pub state: ModelState,
    pub version: String,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub last_state_change: DateTime<Utc>,
    pub quarantine_reason: Option<String>,
}

/// Error type for model registry operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Model not found: {0}")]
    NotFound(String),

    #[error("Invalid state transition: {from} -> {to} for model {model_id}")]
    InvalidTransition {
        model_id: String,
        from: String,
        to: String,
    },

    #[error("Model already exists: {0}")]
    AlreadyExists(String),

    #[error("Verification failed: {0}")]
    VerificationFailed(String),
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Model registry trait for testability.
#[async_trait]
pub trait ModelRegistryService: Send + Sync {
    /// Register a new model in Available state.
    async fn register(&self, entry: ModelEntry) -> Result<(), RegistryError>;

    /// Transition a model's state.
    async fn transition(
        &self,
        model_id: &str,
        new_state: ModelState,
    ) -> Result<ModelEntry, RegistryError>;

    /// Quarantine a model with a reason.
    async fn quarantine(&self, model_id: &str, reason: &str) -> Result<ModelEntry, RegistryError>;

    /// Get a model entry by ID.
    async fn get(&self, model_id: &str) -> Result<ModelEntry, RegistryError>;

    /// List all models, optionally filtered by state.
    async fn list(&self, state_filter: Option<ModelState>) -> Vec<ModelEntry>;

    /// Get the currently active model for a provider type.
    async fn active_model(&self, provider: ProviderType) -> Option<ModelEntry>;
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// In-memory model registry.
pub struct ModelRegistry {
    models: Arc<RwLock<HashMap<String, ModelEntry>>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Validate that a state transition is allowed.
    fn validate_transition(from: ModelState, to: ModelState) -> bool {
        matches!(
            (from, to),
            // Happy path
            (ModelState::Available, ModelState::Downloading)
                | (ModelState::Downloading, ModelState::Downloaded)
                | (ModelState::Downloaded, ModelState::Verifying)
                | (ModelState::Verifying, ModelState::Verified)
                | (ModelState::Verified, ModelState::Active)
                // Re-activation from verified
                | (ModelState::Active, ModelState::Verified)
                // Any state can be quarantined
                | (_, ModelState::Quarantined)
                // Quarantined can go back to available for retry
                | (ModelState::Quarantined, ModelState::Available)
        )
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModelRegistryService for ModelRegistry {
    async fn register(&self, entry: ModelEntry) -> Result<(), RegistryError> {
        let mut models = self.models.write().await;
        if models.contains_key(&entry.id) {
            return Err(RegistryError::AlreadyExists(entry.id));
        }
        models.insert(entry.id.clone(), entry);
        Ok(())
    }

    async fn transition(
        &self,
        model_id: &str,
        new_state: ModelState,
    ) -> Result<ModelEntry, RegistryError> {
        let mut models = self.models.write().await;
        let entry = models
            .get_mut(model_id)
            .ok_or_else(|| RegistryError::NotFound(model_id.into()))?;

        if !Self::validate_transition(entry.state, new_state) {
            return Err(RegistryError::InvalidTransition {
                model_id: model_id.into(),
                from: entry.state.to_string(),
                to: new_state.to_string(),
            });
        }

        entry.state = new_state;
        entry.last_state_change = Utc::now();
        if new_state != ModelState::Quarantined {
            entry.quarantine_reason = None;
        }

        Ok(entry.clone())
    }

    async fn quarantine(&self, model_id: &str, reason: &str) -> Result<ModelEntry, RegistryError> {
        let mut models = self.models.write().await;
        let entry = models
            .get_mut(model_id)
            .ok_or_else(|| RegistryError::NotFound(model_id.into()))?;

        entry.state = ModelState::Quarantined;
        entry.quarantine_reason = Some(reason.to_string());
        entry.last_state_change = Utc::now();

        Ok(entry.clone())
    }

    async fn get(&self, model_id: &str) -> Result<ModelEntry, RegistryError> {
        let models = self.models.read().await;
        models
            .get(model_id)
            .cloned()
            .ok_or_else(|| RegistryError::NotFound(model_id.into()))
    }

    async fn list(&self, state_filter: Option<ModelState>) -> Vec<ModelEntry> {
        let models = self.models.read().await;
        models
            .values()
            .filter(|m| state_filter.is_none_or(|s| m.state == s))
            .cloned()
            .collect()
    }

    async fn active_model(&self, provider: ProviderType) -> Option<ModelEntry> {
        let models = self.models.read().await;
        models
            .values()
            .find(|m| m.provider == provider && m.state == ModelState::Active)
            .cloned()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, provider: ProviderType) -> ModelEntry {
        ModelEntry {
            id: id.into(),
            name: format!("test-model-{id}"),
            provider,
            state: ModelState::Available,
            version: "1.0".into(),
            size_bytes: Some(1024),
            checksum: Some("abc123".into()),
            registered_at: Utc::now(),
            last_state_change: Utc::now(),
            quarantine_reason: None,
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = ModelRegistry::new();
        let entry = make_entry("m-1", ProviderType::Onnx);

        registry.register(entry.clone()).await.unwrap();
        let fetched = registry.get("m-1").await.unwrap();
        assert_eq!(fetched.id, "m-1");
        assert_eq!(fetched.state, ModelState::Available);
    }

    #[tokio::test]
    async fn test_register_duplicate_fails() {
        let registry = ModelRegistry::new();
        let entry = make_entry("m-1", ProviderType::Onnx);

        registry.register(entry.clone()).await.unwrap();
        let result = registry.register(entry).await;
        assert!(matches!(result, Err(RegistryError::AlreadyExists(_))));
    }

    #[tokio::test]
    async fn test_happy_path_lifecycle() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Ollama))
            .await
            .unwrap();

        // Available -> Downloading -> Downloaded -> Verifying -> Verified -> Active
        registry
            .transition("m-1", ModelState::Downloading)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Downloaded)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Verifying)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Verified)
            .await
            .unwrap();
        let entry = registry
            .transition("m-1", ModelState::Active)
            .await
            .unwrap();

        assert_eq!(entry.state, ModelState::Active);
    }

    #[tokio::test]
    async fn test_invalid_transition() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Onnx))
            .await
            .unwrap();

        // Available -> Active is not valid (must go through download/verify)
        let result = registry.transition("m-1", ModelState::Active).await;
        assert!(matches!(
            result,
            Err(RegistryError::InvalidTransition { .. })
        ));
    }

    #[tokio::test]
    async fn test_quarantine_from_any_state() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Onnx))
            .await
            .unwrap();

        let entry = registry
            .quarantine("m-1", "checksum mismatch")
            .await
            .unwrap();
        assert_eq!(entry.state, ModelState::Quarantined);
        assert_eq!(
            entry.quarantine_reason.as_deref(),
            Some("checksum mismatch")
        );
    }

    #[tokio::test]
    async fn test_quarantined_can_retry() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Onnx))
            .await
            .unwrap();

        registry.quarantine("m-1", "bad checksum").await.unwrap();
        let entry = registry
            .transition("m-1", ModelState::Available)
            .await
            .unwrap();
        assert_eq!(entry.state, ModelState::Available);
        assert!(entry.quarantine_reason.is_none());
    }

    #[tokio::test]
    async fn test_list_with_filter() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Onnx))
            .await
            .unwrap();
        registry
            .register(make_entry("m-2", ProviderType::Ollama))
            .await
            .unwrap();

        // Transition m-1 through to Active
        registry
            .transition("m-1", ModelState::Downloading)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Downloaded)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Verifying)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Verified)
            .await
            .unwrap();
        registry
            .transition("m-1", ModelState::Active)
            .await
            .unwrap();

        let all = registry.list(None).await;
        assert_eq!(all.len(), 2);

        let active = registry.list(Some(ModelState::Active)).await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "m-1");

        let available = registry.list(Some(ModelState::Available)).await;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].id, "m-2");
    }

    #[tokio::test]
    async fn test_active_model_by_provider() {
        let registry = ModelRegistry::new();
        registry
            .register(make_entry("m-1", ProviderType::Onnx))
            .await
            .unwrap();
        registry
            .register(make_entry("m-2", ProviderType::Ollama))
            .await
            .unwrap();

        // Make m-1 active
        for state in [
            ModelState::Downloading,
            ModelState::Downloaded,
            ModelState::Verifying,
            ModelState::Verified,
            ModelState::Active,
        ] {
            registry.transition("m-1", state).await.unwrap();
        }

        let active = registry.active_model(ProviderType::Onnx).await;
        assert!(active.is_some());
        assert_eq!(active.unwrap().id, "m-1");

        let no_active = registry.active_model(ProviderType::Ollama).await;
        assert!(no_active.is_none());
    }

    #[test]
    fn test_provider_type_roundtrip() {
        assert_eq!("onnx".parse::<ProviderType>().unwrap(), ProviderType::Onnx);
        assert_eq!(
            "ollama".parse::<ProviderType>().unwrap(),
            ProviderType::Ollama
        );
        assert_eq!(
            "openai".parse::<ProviderType>().unwrap(),
            ProviderType::OpenAi
        );
        assert_eq!(
            "cloud".parse::<ProviderType>().unwrap(),
            ProviderType::OpenAi
        );
        assert_eq!(
            "anthropic".parse::<ProviderType>().unwrap(),
            ProviderType::Anthropic
        );
        assert_eq!(
            "gemini".parse::<ProviderType>().unwrap(),
            ProviderType::Gemini
        );
        assert_eq!("none".parse::<ProviderType>().unwrap(), ProviderType::None);
        assert!("invalid".parse::<ProviderType>().is_err());
    }

    #[test]
    fn test_model_state_display() {
        assert_eq!(ModelState::Available.to_string(), "available");
        assert_eq!(ModelState::Quarantined.to_string(), "quarantined");
        assert_eq!(ModelState::Active.to_string(), "active");
    }
}
