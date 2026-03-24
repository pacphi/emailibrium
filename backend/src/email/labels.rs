//! Label management service (DDD-005: Account Management, Audit Item #37).
//!
//! `LabelManager` provides CRUD operations for email labels/categories
//! via provider APIs. Labels are the primary organizational primitive
//! used by Emailibrium's classification system.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::provider::{EmailProvider, ProviderError};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A cached label entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelEntry {
    /// Provider-assigned label ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whether this is a system label (INBOX, SENT, etc.) or user-created.
    pub is_system: bool,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Label manager trait for testability.
#[async_trait]
pub trait LabelManagerService: Send + Sync {
    /// Ensure a label exists on the provider, creating it if necessary.
    /// Returns the provider-assigned label ID.
    async fn ensure_label(&self, access_token: &str, name: &str) -> Result<String, ProviderError>;

    /// Apply labels to a message.
    async fn apply_labels(
        &self,
        access_token: &str,
        message_id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError>;

    /// Remove labels from a message.
    async fn remove_labels(
        &self,
        access_token: &str,
        message_id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError>;

    /// Get the list of known labels (from cache).
    async fn list_labels(&self) -> Vec<LabelEntry>;
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Concrete label manager backed by an `EmailProvider`.
///
/// Maintains a local cache of label name -> label ID mappings to avoid
/// redundant create calls when the label already exists.
pub struct LabelManager {
    provider: Arc<dyn EmailProvider>,
    /// Prefix to add to all Emailibrium-managed labels.
    prefix: String,
    /// Cache: label name -> label entry.
    cache: RwLock<HashMap<String, LabelEntry>>,
}

impl LabelManager {
    pub fn new(provider: Arc<dyn EmailProvider>, prefix: impl Into<String>) -> Self {
        Self {
            provider,
            prefix: prefix.into(),
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Build the full label name with the configured prefix.
    fn prefixed_name(&self, name: &str) -> String {
        if self.prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", self.prefix, name)
        }
    }
}

#[async_trait]
impl LabelManagerService for LabelManager {
    async fn ensure_label(&self, access_token: &str, name: &str) -> Result<String, ProviderError> {
        let full_name = self.prefixed_name(name);

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&full_name) {
                return Ok(entry.id.clone());
            }
        }

        // Create on provider
        let label_id = self.provider.create_label(access_token, &full_name).await?;

        // Cache the result
        let entry = LabelEntry {
            id: label_id.clone(),
            name: full_name.clone(),
            is_system: false,
        };
        let mut cache = self.cache.write().await;
        cache.insert(full_name, entry);

        Ok(label_id)
    }

    async fn apply_labels(
        &self,
        access_token: &str,
        message_id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        // Ensure all labels exist first
        let mut label_ids = Vec::with_capacity(labels.len());
        for label in labels {
            let id = self.ensure_label(access_token, label).await?;
            label_ids.push(id);
        }

        self.provider
            .label_message(access_token, message_id, &label_ids)
            .await
    }

    async fn remove_labels(
        &self,
        access_token: &str,
        message_id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        self.provider
            .remove_labels(access_token, message_id, labels)
            .await
    }

    async fn list_labels(&self) -> Vec<LabelEntry> {
        let cache = self.cache.read().await;
        cache.values().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::types::{EmailMessage, EmailPage, ListParams, OAuthTokens};

    struct MockLabelProvider;

    #[async_trait]
    impl EmailProvider for MockLabelProvider {
        async fn authenticate(&self, _code: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn refresh_token(&self, _token: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn list_messages(
            &self,
            _t: &str,
            _p: &ListParams,
        ) -> Result<EmailPage, ProviderError> {
            unimplemented!()
        }
        async fn get_message(&self, _t: &str, _id: &str) -> Result<EmailMessage, ProviderError> {
            unimplemented!()
        }
        async fn archive_message(&self, _t: &str, _id: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn label_message(
            &self,
            _t: &str,
            _id: &str,
            _l: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn remove_labels(
            &self,
            _t: &str,
            _id: &str,
            _l: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn create_label(&self, _t: &str, name: &str) -> Result<String, ProviderError> {
            // Simulate provider returning an ID based on the name
            Ok(format!("label-id-{}", name.replace('/', "-")))
        }
    }

    #[tokio::test]
    async fn test_ensure_label_creates_and_caches() {
        let provider = Arc::new(MockLabelProvider);
        let manager = LabelManager::new(provider, "Emailibrium");

        let id = manager.ensure_label("token", "Work").await.unwrap();
        assert_eq!(id, "label-id-Emailibrium-Work");

        // Second call should return cached value without hitting provider
        let id2 = manager.ensure_label("token", "Work").await.unwrap();
        assert_eq!(id, id2);

        let labels = manager.list_labels().await;
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Emailibrium/Work");
    }

    #[tokio::test]
    async fn test_ensure_label_no_prefix() {
        let provider = Arc::new(MockLabelProvider);
        let manager = LabelManager::new(provider, "");

        let id = manager.ensure_label("token", "Finance").await.unwrap();
        assert_eq!(id, "label-id-Finance");
    }

    #[tokio::test]
    async fn test_apply_labels() {
        let provider = Arc::new(MockLabelProvider);
        let manager = LabelManager::new(provider, "");

        let result = manager
            .apply_labels("token", "msg-1", &["Work".into(), "Important".into()])
            .await;
        assert!(result.is_ok());

        // Labels should be cached after apply
        let labels = manager.list_labels().await;
        assert_eq!(labels.len(), 2);
    }

    #[tokio::test]
    async fn test_remove_labels() {
        let provider = Arc::new(MockLabelProvider);
        let manager = LabelManager::new(provider, "");

        let result = manager
            .remove_labels("token", "msg-1", &["Work".into()])
            .await;
        assert!(result.is_ok());
    }
}
