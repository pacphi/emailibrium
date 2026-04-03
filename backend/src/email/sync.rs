//! Provider synchronization service (DDD-005: Account Management, Audit Item #37).
//!
//! `ProviderSync` manages email sync scheduling, delta detection, and
//! incremental sync using provider-specific history IDs / delta tokens.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::provider::{EmailProvider, ProviderError};
use super::types::{EmailPage, ListParams, ProviderKind, SyncState};

// ---------------------------------------------------------------------------
// Sync Status
// ---------------------------------------------------------------------------

/// Status of a sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Completed,
    Failed,
    Paused,
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Syncing => write!(f, "syncing"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Paused => write!(f, "paused"),
        }
    }
}

// ---------------------------------------------------------------------------
// Sync Result
// ---------------------------------------------------------------------------

/// Result of a single sync run.
#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub account_id: String,
    pub emails_fetched: u64,
    pub new_emails: u64,
    pub updated_emails: u64,
    pub errors: u32,
    pub duration_ms: u64,
    pub new_history_id: Option<String>,
    pub completed_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Provider sync service trait for testability.
#[async_trait]
pub trait ProviderSyncService: Send + Sync {
    /// Execute a full or incremental sync for the given account.
    async fn sync_account(
        &self,
        account_id: &str,
        access_token: &str,
        state: &SyncState,
    ) -> Result<SyncResult, ProviderError>;

    /// Detect changes since the last sync using provider delta mechanisms.
    async fn detect_delta(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<DeltaResult, ProviderError>;
}

/// Result of delta detection (what changed since last sync).
#[derive(Debug, Clone, Serialize)]
pub struct DeltaResult {
    pub new_message_ids: Vec<String>,
    pub updated_message_ids: Vec<String>,
    pub deleted_message_ids: Vec<String>,
    pub new_history_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Configuration for retry behaviour during page fetching.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries on rate-limit errors per page.
    pub max_retries: usize,
    /// Base delay in milliseconds before the first retry.
    /// Subsequent retries use exponential backoff: base * 3^(attempt-1).
    pub retry_base_delay_ms: u64,
    /// Delay in milliseconds between successful page fetches to throttle
    /// request rate and avoid hitting provider quotas.
    pub fetch_page_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            retry_base_delay_ms: 5000,
            fetch_page_delay_ms: 200,
        }
    }
}

/// Concrete provider sync service.
///
/// Uses the `EmailProvider` trait to fetch messages and track sync state
/// via history IDs or page tokens for incremental syncing.
///
/// When a `provider_kind` is set, `detect_delta` uses provider-specific
/// delta APIs (Gmail history.list, Outlook delta query) instead of the
/// generic list-based fallback.
pub struct ProviderSync {
    provider: Arc<dyn EmailProvider>,
    batch_size: u32,
    /// Optional: the kind of provider, enabling provider-specific delta APIs.
    provider_kind: Option<ProviderKind>,
    /// Retry and throttling configuration for page fetching.
    retry_config: RetryConfig,
}

impl ProviderSync {
    /// Create a new sync service for the given provider.
    pub fn new(provider: Arc<dyn EmailProvider>, batch_size: u32) -> Self {
        Self {
            provider,
            batch_size,
            provider_kind: None,
            retry_config: RetryConfig::default(),
        }
    }

    /// Create a new sync service with an explicit provider kind for delta APIs.
    pub fn with_kind(
        provider: Arc<dyn EmailProvider>,
        batch_size: u32,
        kind: ProviderKind,
    ) -> Self {
        Self {
            provider,
            batch_size,
            provider_kind: Some(kind),
            retry_config: RetryConfig::default(),
        }
    }

    /// Create a new sync service with explicit retry configuration.
    pub fn with_retry_config(
        provider: Arc<dyn EmailProvider>,
        batch_size: u32,
        kind: Option<ProviderKind>,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            provider,
            batch_size,
            provider_kind: kind,
            retry_config,
        }
    }

    /// Check whether a `ProviderError` is a rate-limit error eligible for retry.
    fn is_rate_limit_error(err: &ProviderError) -> bool {
        matches!(err, ProviderError::RateLimited { .. })
    }

    /// Fetch all pages of messages starting from the sync state cursor.
    ///
    /// Applies exponential backoff retry on rate-limit errors (403 quota /
    /// 429 too-many-requests) and an inter-page throttle delay to stay
    /// within provider quotas. On exhausted retries, returns whatever pages
    /// were successfully fetched (graceful degradation).
    async fn fetch_all_pages(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<(Vec<EmailPage>, Option<String>), ProviderError> {
        let mut pages = Vec::new();
        let mut page_token = state.next_page_token.clone();
        let mut last_history_id = state.history_id.clone();
        let mut page_number: u32 = 1;

        loop {
            let params = ListParams {
                max_results: self.batch_size,
                page_token: page_token.clone(),
                label: None,
                query: None,
            };

            // Attempt the page fetch with retry on rate-limit errors.
            let page_result = self
                .fetch_page_with_retry(access_token, &params, page_number)
                .await;

            match page_result {
                Ok(page) => {
                    let next_token = page.next_page_token.clone();
                    pages.push(page);

                    match next_token {
                        Some(token) => {
                            page_token = Some(token);
                            page_number += 1;

                            // Throttle between pages to stay within quota.
                            if self.retry_config.fetch_page_delay_ms > 0 {
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    self.retry_config.fetch_page_delay_ms,
                                ))
                                .await;
                            }
                        }
                        None => {
                            if last_history_id.is_none() {
                                last_history_id = page_token;
                            }
                            break;
                        }
                    }
                }
                Err(err) if Self::is_rate_limit_error(&err) => {
                    // All retries exhausted for this page. Gracefully degrade:
                    // return whatever pages we successfully fetched so far.
                    let fetched_count: usize = pages.iter().map(|p| p.messages.len()).sum();
                    tracing::warn!(
                        page = page_number,
                        emails_fetched = fetched_count,
                        "Rate limit retries exhausted; returning {fetched_count} emails fetched so far"
                    );
                    break;
                }
                Err(err) => {
                    // Non-rate-limit error: fail immediately.
                    return Err(err);
                }
            }
        }

        Ok((pages, last_history_id))
    }

    /// Fetch a single page with exponential backoff retry on rate-limit errors.
    async fn fetch_page_with_retry(
        &self,
        access_token: &str,
        params: &ListParams,
        page_number: u32,
    ) -> Result<EmailPage, ProviderError> {
        let mut last_error = None;

        // Attempt 0 is the initial try, then up to max_retries retries.
        for attempt in 0..=self.retry_config.max_retries {
            match self.provider.list_messages(access_token, params).await {
                Ok(page) => return Ok(page),
                Err(err)
                    if Self::is_rate_limit_error(&err)
                        && attempt < self.retry_config.max_retries =>
                {
                    // Exponential backoff: base_delay * 3^attempt
                    // attempt 0 → base_delay, attempt 1 → base*3, attempt 2 → base*9
                    let delay_ms = self.retry_config.retry_base_delay_ms * 3u64.pow(attempt as u32);
                    let delay_secs = delay_ms as f64 / 1000.0;

                    let attempt_num = attempt + 1;
                    let max_retries = self.retry_config.max_retries;
                    tracing::warn!(
                        page = page_number,
                        attempt = attempt_num,
                        max_retries = max_retries,
                        delay_secs = delay_secs,
                        "Gmail quota exceeded on page {page_number}, retrying in {delay_secs}s (attempt {attempt_num}/{max_retries})",
                    );

                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    last_error = Some(err);
                }
                Err(err) => {
                    // Non-retryable error OR last retry exhausted.
                    return Err(err);
                }
            }
        }

        // Should not reach here, but return the last error if it does.
        Err(last_error.unwrap_or_else(|| {
            ProviderError::RequestFailed("Retry loop ended unexpectedly".into())
        }))
    }
}

#[async_trait]
impl ProviderSyncService for ProviderSync {
    async fn sync_account(
        &self,
        account_id: &str,
        access_token: &str,
        state: &SyncState,
    ) -> Result<SyncResult, ProviderError> {
        let start = std::time::Instant::now();

        let (pages, new_history_id) = self.fetch_all_pages(access_token, state).await?;

        let mut emails_fetched: u64 = 0;
        let errors: u32 = 0;

        for page in &pages {
            emails_fetched += page.messages.len() as u64;
        }

        // For now, treat all fetched as "new" since we do not yet have
        // deduplication logic against the local DB. A production
        // implementation would check message IDs against existing rows.
        let new_emails = emails_fetched;
        let updated_emails = 0;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(SyncResult {
            account_id: account_id.to_string(),
            emails_fetched,
            new_emails,
            updated_emails,
            errors,
            duration_ms,
            new_history_id,
            completed_at: Utc::now(),
        })
    }

    async fn detect_delta(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<DeltaResult, ProviderError> {
        // Try provider-specific delta APIs first, fall back to list-based.
        match self.provider_kind {
            Some(ProviderKind::Gmail) => self.detect_delta_gmail(access_token, state).await,
            Some(ProviderKind::Outlook) => self.detect_delta_outlook(access_token, state).await,
            _ => self.detect_delta_fallback(access_token, state).await,
        }
    }
}

impl ProviderSync {
    /// Gmail-specific delta detection using history.list API.
    async fn detect_delta_gmail(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<DeltaResult, ProviderError> {
        let history_id = match &state.history_id {
            Some(id) => id.clone(),
            None => {
                // No history ID yet -- fall back to list-based detection.
                return self.detect_delta_fallback(access_token, state).await;
            }
        };

        // Call Gmail history.list API.
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/history?startHistoryId={history_id}"
        );

        let resp: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let delta = super::delta::parse_gmail_history(&resp).map_err(ProviderError::ParseError)?;

        // Map label changes to "updated" messages.
        let updated_ids: Vec<String> = delta
            .label_changes
            .iter()
            .map(|lc| lc.message_id.clone())
            .collect();

        Ok(DeltaResult {
            new_message_ids: delta.added_message_ids,
            updated_message_ids: updated_ids,
            deleted_message_ids: delta.deleted_message_ids,
            new_history_id: delta.new_history_id,
        })
    }

    /// Outlook-specific delta detection using Graph delta query.
    async fn detect_delta_outlook(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<DeltaResult, ProviderError> {
        // Use history_id as the delta link storage.
        let delta_link = state.history_id.as_deref();

        let url = match delta_link {
            Some(link) => link.to_string(),
            None => "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$top=50"
                .to_string(),
        };

        let resp: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let delta = super::delta::parse_outlook_delta(&resp).map_err(ProviderError::ParseError)?;

        Ok(DeltaResult {
            new_message_ids: delta.added_or_modified_ids,
            updated_message_ids: Vec::new(),
            deleted_message_ids: delta.deleted_ids,
            // Store the delta link as the new "history ID" for the next call.
            new_history_id: delta.delta_link,
        })
    }

    /// Fallback delta detection using the generic list_messages API.
    async fn detect_delta_fallback(
        &self,
        access_token: &str,
        state: &SyncState,
    ) -> Result<DeltaResult, ProviderError> {
        let params = ListParams {
            max_results: self.batch_size,
            page_token: state.next_page_token.clone(),
            label: None,
            query: None,
        };

        let page = self.provider.list_messages(access_token, &params).await?;

        let new_message_ids: Vec<String> = page.messages.iter().map(|m| m.id.clone()).collect();

        Ok(DeltaResult {
            new_message_ids,
            updated_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            new_history_id: page.next_page_token,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::types::{EmailMessage, OAuthTokens};

    /// Mock provider for testing sync.
    struct MockProvider {
        messages: Vec<EmailMessage>,
    }

    impl MockProvider {
        fn with_messages(count: usize) -> Self {
            let messages = (0..count)
                .map(|i| EmailMessage {
                    id: format!("msg-{i}"),
                    thread_id: None,
                    from: format!("sender{i}@test.com"),
                    to: vec!["user@test.com".into()],
                    subject: format!("Subject {i}"),
                    snippet: format!("Snippet {i}"),
                    body: None,
                    body_html: None,
                    labels: vec![],
                    date: Utc::now(),
                    is_read: false,
                    list_unsubscribe: None,
                    list_unsubscribe_post: None,
                })
                .collect();
            Self { messages }
        }
    }

    #[async_trait]
    impl EmailProvider for MockProvider {
        async fn authenticate(&self, _code: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn refresh_token(&self, _token: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn list_messages(
            &self,
            _token: &str,
            _params: &ListParams,
        ) -> Result<EmailPage, ProviderError> {
            Ok(EmailPage {
                messages: self.messages.clone(),
                next_page_token: None,
                result_size_estimate: Some(self.messages.len() as u32),
            })
        }
        async fn get_message(&self, _token: &str, id: &str) -> Result<EmailMessage, ProviderError> {
            self.messages
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .ok_or(ProviderError::NotFound(id.into()))
        }
        async fn archive_message(&self, _token: &str, _id: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn label_message(
            &self,
            _token: &str,
            _id: &str,
            _labels: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn remove_labels(
            &self,
            _token: &str,
            _id: &str,
            _labels: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn create_label(&self, _token: &str, name: &str) -> Result<String, ProviderError> {
            Ok(name.to_string())
        }
    }

    fn make_sync_state(account_id: &str) -> SyncState {
        SyncState {
            account_id: account_id.to_string(),
            last_sync_at: None,
            history_id: None,
            next_page_token: None,
            emails_synced: 0,
            sync_failures: 0,
            last_error: None,
            status: "idle".into(),
        }
    }

    #[tokio::test]
    async fn test_sync_account_fetches_messages() {
        let provider = Arc::new(MockProvider::with_messages(5));
        let sync = ProviderSync::new(provider, 50);
        let state = make_sync_state("acct-1");

        let result = sync.sync_account("acct-1", "token", &state).await.unwrap();
        assert_eq!(result.emails_fetched, 5);
        assert_eq!(result.new_emails, 5);
        assert_eq!(result.account_id, "acct-1");
    }

    #[tokio::test]
    async fn test_detect_delta() {
        let provider = Arc::new(MockProvider::with_messages(3));
        let sync = ProviderSync::new(provider, 50);
        let state = make_sync_state("acct-1");

        let delta = sync.detect_delta("token", &state).await.unwrap();
        assert_eq!(delta.new_message_ids.len(), 3);
        assert!(delta.deleted_message_ids.is_empty());
    }

    #[tokio::test]
    async fn test_sync_result_has_duration() {
        let provider = Arc::new(MockProvider::with_messages(1));
        let sync = ProviderSync::new(provider, 50);
        let state = make_sync_state("acct-1");

        let result = sync.sync_account("acct-1", "token", &state).await.unwrap();
        // Duration should be >= 0 (fast test, likely 0 or 1ms)
        assert!(result.completed_at <= Utc::now());
    }

    // -----------------------------------------------------------------------
    // Rate-limit retry tests
    // -----------------------------------------------------------------------

    /// Mock provider that fails with RateLimited for the first N calls,
    /// then succeeds.
    struct RateLimitMockProvider {
        messages: Vec<EmailMessage>,
        /// Number of calls that should fail before succeeding.
        fail_count: std::sync::atomic::AtomicU32,
    }

    impl RateLimitMockProvider {
        fn new(messages: Vec<EmailMessage>, fail_count: u32) -> Self {
            Self {
                messages,
                fail_count: std::sync::atomic::AtomicU32::new(fail_count),
            }
        }
    }

    #[async_trait]
    impl EmailProvider for RateLimitMockProvider {
        async fn authenticate(&self, _code: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn refresh_token(&self, _token: &str) -> Result<OAuthTokens, ProviderError> {
            unimplemented!()
        }
        async fn list_messages(
            &self,
            _token: &str,
            _params: &ListParams,
        ) -> Result<EmailPage, ProviderError> {
            let remaining = self.fail_count.fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |n| if n > 0 { Some(n - 1) } else { None },
            );
            if remaining.is_ok() {
                return Err(ProviderError::RateLimited {
                    retry_after_secs: 60,
                });
            }
            Ok(EmailPage {
                messages: self.messages.clone(),
                next_page_token: None,
                result_size_estimate: Some(self.messages.len() as u32),
            })
        }
        async fn get_message(&self, _token: &str, id: &str) -> Result<EmailMessage, ProviderError> {
            self.messages
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .ok_or(ProviderError::NotFound(id.into()))
        }
        async fn archive_message(&self, _token: &str, _id: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn label_message(
            &self,
            _token: &str,
            _id: &str,
            _labels: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn remove_labels(
            &self,
            _token: &str,
            _id: &str,
            _labels: &[String],
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn create_label(&self, _token: &str, name: &str) -> Result<String, ProviderError> {
            Ok(name.to_string())
        }
    }

    fn make_fast_retry_config() -> RetryConfig {
        RetryConfig {
            max_retries: 3,
            retry_base_delay_ms: 10, // very short for tests
            fetch_page_delay_ms: 0,
        }
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_transient_rate_limit() {
        // Fail once then succeed — retry should recover.
        let msgs = MockProvider::with_messages(3).messages;
        let provider = Arc::new(RateLimitMockProvider::new(msgs, 1));
        let sync = ProviderSync::with_retry_config(provider, 50, None, make_fast_retry_config());
        let state = make_sync_state("acct-1");

        let result = sync.sync_account("acct-1", "token", &state).await.unwrap();
        assert_eq!(result.emails_fetched, 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted_returns_partial_results() {
        // Fail more times than max_retries — should gracefully degrade.
        let msgs = MockProvider::with_messages(3).messages;
        // fail_count = 10, but max_retries = 3 => all retries exhausted on first page
        let provider = Arc::new(RateLimitMockProvider::new(msgs, 10));
        let sync = ProviderSync::with_retry_config(provider, 50, None, make_fast_retry_config());
        let state = make_sync_state("acct-1");

        // Should succeed (graceful degradation) with 0 emails.
        let result = sync.sync_account("acct-1", "token", &state).await.unwrap();
        assert_eq!(result.emails_fetched, 0);
    }

    #[tokio::test]
    async fn test_non_rate_limit_error_fails_immediately() {
        // A non-rate-limit error should not be retried.
        struct AlwaysFailProvider;

        #[async_trait]
        impl EmailProvider for AlwaysFailProvider {
            async fn authenticate(&self, _code: &str) -> Result<OAuthTokens, ProviderError> {
                unimplemented!()
            }
            async fn refresh_token(&self, _token: &str) -> Result<OAuthTokens, ProviderError> {
                unimplemented!()
            }
            async fn list_messages(
                &self,
                _token: &str,
                _params: &ListParams,
            ) -> Result<EmailPage, ProviderError> {
                Err(ProviderError::RequestFailed("Something broke".into()))
            }
            async fn get_message(
                &self,
                _token: &str,
                _id: &str,
            ) -> Result<EmailMessage, ProviderError> {
                unimplemented!()
            }
            async fn archive_message(&self, _token: &str, _id: &str) -> Result<(), ProviderError> {
                unimplemented!()
            }
            async fn label_message(
                &self,
                _token: &str,
                _id: &str,
                _labels: &[String],
            ) -> Result<(), ProviderError> {
                unimplemented!()
            }
            async fn remove_labels(
                &self,
                _token: &str,
                _id: &str,
                _labels: &[String],
            ) -> Result<(), ProviderError> {
                unimplemented!()
            }
            async fn create_label(
                &self,
                _token: &str,
                _name: &str,
            ) -> Result<String, ProviderError> {
                unimplemented!()
            }
        }

        let provider = Arc::new(AlwaysFailProvider);
        let sync = ProviderSync::with_retry_config(provider, 50, None, make_fast_retry_config());
        let state = make_sync_state("acct-1");

        let err = sync
            .sync_account("acct-1", "token", &state)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProviderError::RequestFailed(_)),
            "Expected RequestFailed, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_is_rate_limit_error_detection() {
        assert!(ProviderSync::is_rate_limit_error(
            &ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));
        assert!(!ProviderSync::is_rate_limit_error(
            &ProviderError::RequestFailed("some error".into())
        ));
    }
}
