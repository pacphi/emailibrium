//! `EmailProviderFactory` — account-scoped EmailProvider construction
//! (Item #1 of the Phase D backend follow-ups).
//!
//! ApplyOrchestrator originally received a static `HashMap<Provider,
//! Arc<dyn EmailProvider>>` whose contents had to be wired before any
//! account context existed. That couldn't work for OAuth-based providers
//! since we need a *per-account* access token. The factory abstracts
//! "give me a working EmailProvider for this accountId" so the worker can
//! resolve providers lazily, with caching for the duration of an apply job.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::email::oauth::{OAuthError, OAuthManager};
use crate::email::provider::EmailProvider;
use crate::email::types::ProviderKind;

#[derive(Debug, Error)]
pub enum FactoryError {
    #[error("account not found: {0}")]
    NotFound(String),
    #[error("oauth: {0}")]
    OAuth(String),
    #[error("provider kind not supported by factory: {0}")]
    UnsupportedKind(&'static str),
    #[error("provider configuration error: {0}")]
    Config(String),
}

impl From<OAuthError> for FactoryError {
    fn from(value: OAuthError) -> Self {
        FactoryError::OAuth(value.to_string())
    }
}

/// Result of resolving a provider for an account: the trait object plus the
/// access token to pass into every call. Tokens are paired with the
/// provider so callers don't need a second lookup.
pub struct ResolvedProvider {
    pub provider: Arc<dyn EmailProvider>,
    pub access_token: String,
    pub kind: ProviderKind,
}

#[async_trait]
pub trait EmailProviderFactory: Send + Sync {
    /// Resolve a provider+token for the given account. Implementations
    /// SHOULD cache so repeated calls within an apply job are cheap.
    async fn provider_for(&self, account_id: &str) -> Result<ResolvedProvider, FactoryError>;
}

// ---------------------------------------------------------------------------
// Production: OAuth-derived factory (Gmail / Outlook only).
// ---------------------------------------------------------------------------
//
// IMAP/POP3 require user-supplied credentials (host/port/username/password)
// that aren't reachable from the cleanup orchestrator today. Those return
// `UnsupportedKind` and the worker maps the error to a `provider_error`
// `OpFailed` event. Wiring IMAP/POP3 is a follow-up.

pub struct OAuthEmailProviderFactory {
    oauth_manager: Arc<OAuthManager>,
    gmail_config: Option<crate::email::types::ProviderConfig>,
    outlook_config: Option<crate::email::types::ProviderConfig>,
    cache: Mutex<HashMap<String, Arc<dyn EmailProvider>>>,
}

impl OAuthEmailProviderFactory {
    pub fn new(
        oauth_manager: Arc<OAuthManager>,
        gmail_config: Option<crate::email::types::ProviderConfig>,
        outlook_config: Option<crate::email::types::ProviderConfig>,
    ) -> Self {
        Self {
            oauth_manager,
            gmail_config,
            outlook_config,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl EmailProviderFactory for OAuthEmailProviderFactory {
    async fn provider_for(&self, account_id: &str) -> Result<ResolvedProvider, FactoryError> {
        let accounts = self.oauth_manager.list_accounts().await?;
        let account = accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or_else(|| FactoryError::NotFound(account_id.to_string()))?;

        let access_token = self.oauth_manager.get_access_token(account_id).await?;

        // Cached provider instance lookup.
        {
            let cache = self.cache.lock().await;
            if let Some(p) = cache.get(account_id) {
                return Ok(ResolvedProvider {
                    provider: p.clone(),
                    access_token,
                    kind: account.provider,
                });
            }
        }

        let provider: Arc<dyn EmailProvider> = match account.provider {
            ProviderKind::Gmail => {
                let cfg = self
                    .gmail_config
                    .clone()
                    .ok_or_else(|| FactoryError::Config("Gmail OAuth not configured".into()))?;
                Arc::new(crate::email::gmail::GmailProvider::new(cfg))
            }
            ProviderKind::Outlook => {
                let cfg = self
                    .outlook_config
                    .clone()
                    .ok_or_else(|| FactoryError::Config("Outlook OAuth not configured".into()))?;
                Arc::new(crate::email::outlook::OutlookProvider::new(cfg))
            }
            ProviderKind::Imap => return Err(FactoryError::UnsupportedKind("imap")),
            ProviderKind::Pop3 => return Err(FactoryError::UnsupportedKind("pop3")),
        };

        let mut cache = self.cache.lock().await;
        cache.insert(account_id.to_string(), provider.clone());
        Ok(ResolvedProvider {
            provider,
            access_token,
            kind: account.provider,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests / no-op default: factory that always returns a caller-supplied
// provider (or fails). Used as the default in [`ApplyOrchestrator::new`] so
// existing tests don't need a real OAuth manager.
// ---------------------------------------------------------------------------

type MockProviderResolver =
    Arc<dyn Fn(&str) -> Result<ResolvedProvider, FactoryError> + Send + Sync>;

pub struct MockEmailProviderFactory {
    inner: MockProviderResolver,
}

impl MockEmailProviderFactory {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&str) -> Result<ResolvedProvider, FactoryError> + Send + Sync + 'static,
    {
        Self { inner: Arc::new(f) }
    }

    /// Factory that fails every call — wired as the default so existing
    /// tests that DO expect "no provider" semantics behave like the old
    /// HashMap-empty path: dispatch returns Ok(()) (handled in the worker).
    pub fn no_op() -> Self {
        Self::new(|account_id| Err(FactoryError::NotFound(account_id.to_string())))
    }
}

#[async_trait]
impl EmailProviderFactory for MockEmailProviderFactory {
    async fn provider_for(&self, account_id: &str) -> Result<ResolvedProvider, FactoryError> {
        (self.inner)(account_id)
    }
}
