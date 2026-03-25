//! EmailProvider trait (DDD-005: Anti-Corruption Layer).
//!
//! All email providers are abstracted behind this trait so that provider-specific
//! APIs never leak into the domain. Each implementation translates between the
//! provider's REST API and the domain model types.

use async_trait::async_trait;

use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens};

/// Errors from email provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("OAuth error: {0}")]
    OAuthError(String),

    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("Token expired and refresh failed: {0}")]
    TokenExpired(String),

    #[error("Rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("Message not found: {0}")]
    NotFound(String),

    #[error("Provider configuration error: {0}")]
    ConfigError(String),

    #[error("Deserialization error: {0}")]
    ParseError(String),
}

/// Unified interface for interacting with email providers (DDD-005 ACL).
///
/// Each provider (Gmail, Outlook, IMAP, POP3) implements this trait,
/// translating provider-specific responses into the domain model.
#[async_trait]
pub trait EmailProvider: Send + Sync {
    /// Exchange an authorization code for OAuth tokens.
    async fn authenticate(&self, auth_code: &str) -> Result<OAuthTokens, ProviderError>;

    /// Refresh an expired access token using a refresh token.
    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, ProviderError>;

    /// List messages with pagination and optional filters.
    async fn list_messages(
        &self,
        access_token: &str,
        params: &ListParams,
    ) -> Result<EmailPage, ProviderError>;

    /// Get a single message by provider-specific ID.
    async fn get_message(
        &self,
        access_token: &str,
        id: &str,
    ) -> Result<EmailMessage, ProviderError>;

    /// Archive a message (Gmail: remove INBOX label; Outlook: move to Archive).
    async fn archive_message(&self, access_token: &str, id: &str) -> Result<(), ProviderError>;

    /// Apply labels/categories to a message.
    async fn label_message(
        &self,
        access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError>;

    /// Remove labels/categories from a message.
    async fn remove_labels(
        &self,
        access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError>;

    /// Create a label/category in the provider if it doesn't exist.
    async fn create_label(&self, access_token: &str, name: &str) -> Result<String, ProviderError>;

    /// List all labels/categories. Returns `Vec<(id, name)>`.
    async fn list_labels(
        &self,
        _access_token: &str,
    ) -> Result<Vec<(String, String)>, ProviderError> {
        Err(ProviderError::ConfigError(
            "list_labels not supported by this provider".into(),
        ))
    }

    /// Delete a label/category definition by ID.
    async fn delete_label(
        &self,
        _access_token: &str,
        _label_id: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::ConfigError(
            "delete_label not supported by this provider".into(),
        ))
    }

    /// Move a message back to the inbox (undo archive).
    async fn unarchive_message(
        &self,
        _access_token: &str,
        _id: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::ConfigError(
            "unarchive not supported by this provider".into(),
        ))
    }
}
