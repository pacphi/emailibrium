//! Archive execution service (DDD-005: Account Management, Audit Item #37).
//!
//! `ArchiveExecutor` applies archive strategies to email messages,
//! delegating the actual provider-level archive operation to the
//! `EmailProvider` trait implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::provider::{EmailProvider, ProviderError};

// ---------------------------------------------------------------------------
// Archive Strategy
// ---------------------------------------------------------------------------

/// Supported archive strategies.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveStrategy {
    /// Archive immediately after classification.
    Immediate,
    /// Archive after a configurable delay (e.g. 7 days).
    Delayed,
    /// Archive only when the user explicitly confirms.
    #[default]
    Manual,
    /// Never archive (read-only mode).
    None,
}

impl std::fmt::Display for ArchiveStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Immediate => write!(f, "immediate"),
            Self::Delayed => write!(f, "delayed"),
            Self::Manual => write!(f, "manual"),
            Self::None => write!(f, "none"),
        }
    }
}

// ---------------------------------------------------------------------------
// Archive Result
// ---------------------------------------------------------------------------

/// Result of an archive operation.
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveResult {
    pub message_id: String,
    pub archived: bool,
    pub strategy: ArchiveStrategy,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Archive executor trait for testability.
#[async_trait]
pub trait ArchiveExecutorService: Send + Sync {
    /// Archive a single message using the given strategy.
    async fn archive_message(
        &self,
        access_token: &str,
        message_id: &str,
        strategy: ArchiveStrategy,
    ) -> Result<ArchiveResult, ProviderError>;

    /// Archive a batch of messages.
    async fn archive_batch(
        &self,
        access_token: &str,
        message_ids: &[String],
        strategy: ArchiveStrategy,
    ) -> Vec<ArchiveResult>;
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Concrete archive executor backed by an `EmailProvider`.
pub struct ArchiveExecutor {
    provider: Arc<dyn EmailProvider>,
}

impl ArchiveExecutor {
    pub fn new(provider: Arc<dyn EmailProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl ArchiveExecutorService for ArchiveExecutor {
    async fn archive_message(
        &self,
        access_token: &str,
        message_id: &str,
        strategy: ArchiveStrategy,
    ) -> Result<ArchiveResult, ProviderError> {
        match strategy {
            ArchiveStrategy::None => Ok(ArchiveResult {
                message_id: message_id.to_string(),
                archived: false,
                strategy,
                error: None,
            }),
            ArchiveStrategy::Manual => Ok(ArchiveResult {
                message_id: message_id.to_string(),
                archived: false,
                strategy,
                error: Some("Manual archive requires explicit user action".into()),
            }),
            ArchiveStrategy::Immediate | ArchiveStrategy::Delayed => {
                // Both immediate and delayed ultimately call the provider
                // archive method. For delayed, the caller is responsible
                // for scheduling; this executor just executes.
                match self
                    .provider
                    .archive_message(access_token, message_id)
                    .await
                {
                    Ok(()) => Ok(ArchiveResult {
                        message_id: message_id.to_string(),
                        archived: true,
                        strategy,
                        error: None,
                    }),
                    Err(e) => Ok(ArchiveResult {
                        message_id: message_id.to_string(),
                        archived: false,
                        strategy,
                        error: Some(e.to_string()),
                    }),
                }
            }
        }
    }

    async fn archive_batch(
        &self,
        access_token: &str,
        message_ids: &[String],
        strategy: ArchiveStrategy,
    ) -> Vec<ArchiveResult> {
        let mut results = Vec::with_capacity(message_ids.len());
        for id in message_ids {
            let result = self.archive_message(access_token, id, strategy).await;
            match result {
                Ok(r) => results.push(r),
                Err(e) => results.push(ArchiveResult {
                    message_id: id.clone(),
                    archived: false,
                    strategy,
                    error: Some(e.to_string()),
                }),
            }
        }
        results
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::provider::ProviderError;
    use crate::email::types::{EmailMessage, EmailPage, ListParams, OAuthTokens};

    struct MockArchiveProvider {
        should_fail: bool,
    }

    #[async_trait]
    impl EmailProvider for MockArchiveProvider {
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
            if self.should_fail {
                Err(ProviderError::RequestFailed("Archive failed".into()))
            } else {
                Ok(())
            }
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
            Ok(name.into())
        }
    }

    #[tokio::test]
    async fn test_archive_immediate_success() {
        let provider = Arc::new(MockArchiveProvider { should_fail: false });
        let executor = ArchiveExecutor::new(provider);

        let result = executor
            .archive_message("token", "msg-1", ArchiveStrategy::Immediate)
            .await
            .unwrap();

        assert!(result.archived);
        assert_eq!(result.strategy, ArchiveStrategy::Immediate);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_archive_none_strategy() {
        let provider = Arc::new(MockArchiveProvider { should_fail: false });
        let executor = ArchiveExecutor::new(provider);

        let result = executor
            .archive_message("token", "msg-1", ArchiveStrategy::None)
            .await
            .unwrap();

        assert!(!result.archived);
    }

    #[tokio::test]
    async fn test_archive_manual_strategy() {
        let provider = Arc::new(MockArchiveProvider { should_fail: false });
        let executor = ArchiveExecutor::new(provider);

        let result = executor
            .archive_message("token", "msg-1", ArchiveStrategy::Manual)
            .await
            .unwrap();

        assert!(!result.archived);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_archive_failure_returns_error_in_result() {
        let provider = Arc::new(MockArchiveProvider { should_fail: true });
        let executor = ArchiveExecutor::new(provider);

        let result = executor
            .archive_message("token", "msg-1", ArchiveStrategy::Immediate)
            .await
            .unwrap();

        assert!(!result.archived);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_archive_batch() {
        let provider = Arc::new(MockArchiveProvider { should_fail: false });
        let executor = ArchiveExecutor::new(provider);

        let ids = vec!["msg-1".into(), "msg-2".into(), "msg-3".into()];
        let results = executor
            .archive_batch("token", &ids, ArchiveStrategy::Immediate)
            .await;

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.archived));
    }
}
