//! Types for the email provider bounded context (DDD-005).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Supported email providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gmail,
    Outlook,
    Imap,
    Pop3,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Outlook => "outlook",
            Self::Imap => "imap",
            Self::Pop3 => "pop3",
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "gmail" => Ok(Self::Gmail),
            "outlook" => Ok(Self::Outlook),
            "imap" => Ok(Self::Imap),
            "pop3" => Ok(Self::Pop3),
            other => Err(format!("Unknown provider: {other}")),
        }
    }
}

/// Account connection status (DDD-005 value object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Connected,
    Disconnected,
    Error,
    Suspended,
}

impl AccountStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
            Self::Suspended => "suspended",
        }
    }
}

impl std::str::FromStr for AccountStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "connected" => Ok(Self::Connected),
            "disconnected" => Ok(Self::Disconnected),
            "error" => Ok(Self::Error),
            "suspended" => Ok(Self::Suspended),
            other => Err(format!("Unknown account status: {other}")),
        }
    }
}

/// A connected email account (DDD-005 aggregate root).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedAccount {
    pub id: String,
    pub provider: ProviderKind,
    pub email_address: String,
    pub status: AccountStatus,
    pub archive_strategy: String,
    pub label_prefix: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Per-account synchronization state (DDD-005 aggregate).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub account_id: String,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub history_id: Option<String>,
    pub next_page_token: Option<String>,
    pub emails_synced: u64,
    pub sync_failures: u32,
    pub last_error: Option<String>,
    pub status: String,
}

/// OAuth2 token pair returned after token exchange or refresh.
#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub email: Option<String>,
}

/// Parameters for listing email messages from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    /// Maximum number of messages to return.
    #[serde(default = "default_max_results")]
    pub max_results: u32,
    /// Page token for pagination.
    pub page_token: Option<String>,
    /// Optional label/folder filter.
    pub label: Option<String>,
    /// Optional query string (provider-specific).
    pub query: Option<String>,
}

fn default_max_results() -> u32 {
    50
}

/// A normalized email message from any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailMessage {
    /// Provider-specific message ID.
    pub id: String,
    /// Thread/conversation ID (if supported).
    pub thread_id: Option<String>,
    /// Sender address.
    pub from: String,
    /// Recipient addresses.
    pub to: Vec<String>,
    /// Subject line.
    pub subject: String,
    /// Plain-text body snippet.
    pub snippet: String,
    /// Full body text (if fetched).
    pub body: Option<String>,
    /// Provider-specific labels/categories.
    pub labels: Vec<String>,
    /// When the message was received.
    pub date: DateTime<Utc>,
    /// Whether the message has been read.
    pub is_read: bool,
}

/// Response envelope for paginated message lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailPage {
    pub messages: Vec<EmailMessage>,
    pub next_page_token: Option<String>,
    pub result_size_estimate: Option<u32>,
}

/// Configuration for connecting to a specific provider instance.
/// Holds resolved (non-encrypted) client credentials for a single OAuth flow.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_roundtrip() {
        assert_eq!(
            "gmail".parse::<ProviderKind>().unwrap(),
            ProviderKind::Gmail
        );
        assert_eq!(ProviderKind::Outlook.as_str(), "outlook");
        assert!("unknown".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn account_status_roundtrip() {
        assert_eq!(
            "connected".parse::<AccountStatus>().unwrap(),
            AccountStatus::Connected
        );
        assert_eq!(AccountStatus::Suspended.as_str(), "suspended");
    }
}
