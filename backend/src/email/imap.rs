//! IMAP provider implementation (DDD-005 ACL).
//!
//! Full async IMAP client using `async-imap` (runtime-tokio) + `async-native-tls`
//! + `mail-parser`, providing feature parity with the Gmail and Outlook providers.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::provider::{EmailProvider, FolderOrLabel, MoveKind, ProviderError, SendDraft};
use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How transport security is established for an IMAP (and the paired SMTP)
/// connection.
///
/// Per RFC 8314, implicit TLS is preferred; STARTTLS is supported for servers
/// that only offer it, and MUST fail closed (never fall back to cleartext if
/// the upgrade fails). Plaintext is intended only for trusted local relays
/// (e.g. a bridge on loopback) and never sends credentials over an untrusted
/// network in practice because the SSRF guard restricts where it can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImapEncryption {
    /// Implicit TLS — TLS negotiated immediately on connect (typically 993).
    Ssl,
    /// STARTTLS — plain connect, then upgrade to TLS before login (typically 143).
    StartTls,
    /// No transport encryption (local/trusted relays only).
    Plaintext,
}

impl ImapEncryption {
    /// Parse the frontend's encryption string ("ssl" | "tls" | "none").
    /// "tls" denotes STARTTLS in the UI's vocabulary.
    pub fn from_request_str(s: &str) -> Self {
        match s {
            "ssl" => Self::Ssl,
            "tls" => Self::StartTls,
            "none" => Self::Plaintext,
            // Default to the safest mode for anything unexpected.
            _ => Self::Ssl,
        }
    }

    /// Stable string form for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssl => "ssl",
            Self::StartTls => "starttls",
            Self::Plaintext => "plaintext",
        }
    }

    /// Parse the persisted string form (tolerates the request forms too).
    pub fn from_stored_str(s: &str) -> Self {
        match s {
            "ssl" => Self::Ssl,
            "starttls" | "tls" => Self::StartTls,
            "plaintext" | "none" => Self::Plaintext,
            _ => Self::Ssl,
        }
    }
}

fn default_encryption() -> ImapEncryption {
    ImapEncryption::Ssl
}

/// How this account authenticates to the IMAP (and paired SMTP) server.
///
/// `Password` covers every app-password provider (Gmail/Workspace, iCloud,
/// Fastmail, Yahoo) and Proton Bridge's own Bridge-generated password.
/// `OAuthBearer` is Outlook/Microsoft 365's device-code path (milestone 6):
/// basic auth ended for all Outlook accounts 16 Sep 2024, so XOAUTH2 with a
/// bearer access token is the only way in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImapAuth {
    Password(String),
    OAuthBearer(String),
}

impl ImapAuth {
    fn is_empty(&self) -> bool {
        match self {
            Self::Password(p) => p.is_empty(),
            Self::OAuthBearer(t) => t.is_empty(),
        }
    }
}

/// Provider-specific quirks that change how Kind/Topic/State labels
/// (`cleanup::domain::label_namespace`) get applied over IMAP — see
/// docs/plan/mail-agent-2026-10.md row 6 and ADR-040.
///
/// `Generic` (the default) is every provider this repo already supported
/// before milestone 6 (Gmail/Workspace/iCloud/Fastmail/Yahoo app-password):
/// labels are additive IMAP keyword flags, unchanged by this milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfile {
    #[default]
    Generic,
    /// Proton Mail Bridge: labels are real mailboxes under `Labels/`, and
    /// "applying a label" is a non-destructive `COPY` (a message keeps every
    /// label it's ever had unless explicitly removed) — never a keyword flag.
    ProtonBridge,
    /// Outlook/Microsoft 365 over IMAP: no custom keyword flags at all.
    /// `Kind/`/`Topic/` labels are redirected to a real folder; `State/`
    /// labels are ledger-only (never written to the provider).
    OutlookImap,
}

impl ProviderProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::ProtonBridge => "proton_bridge",
            Self::OutlookImap => "outlook_imap",
        }
    }

    pub fn from_stored_str(s: &str) -> Self {
        match s {
            "proton_bridge" => Self::ProtonBridge,
            "outlook_imap" => Self::OutlookImap,
            _ => Self::Generic,
        }
    }
}

/// Which label namespace (`cleanup::domain::label_namespace`) a label string
/// belongs to — drives folder-vs-keyword-vs-ledger routing per `ProviderProfile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelNamespace {
    Kind,
    Topic,
    State,
    Other,
}

/// Guess a provider's profile from the host/port a user (or onboarding's
/// provider-detection table) supplied — a pure, overridable default, not an
/// irrevocable classification: an account's stored `imap_auth_profile` wins
/// once set.
pub fn detect_provider_profile(host: &str, port: u16) -> ProviderProfile {
    let host_lower = host.to_ascii_lowercase();
    if host_lower.contains("protonmail") || host_lower.contains("proton.me") {
        return ProviderProfile::ProtonBridge;
    }
    if matches!(host_lower.as_str(), "127.0.0.1" | "localhost" | "::1")
        && matches!(port, 1143 | 1025)
    {
        return ProviderProfile::ProtonBridge;
    }
    if host_lower.contains("outlook.office365.com") || host_lower.contains("imap-mail.outlook.com")
    {
        return ProviderProfile::OutlookImap;
    }
    ProviderProfile::Generic
}

/// Configuration for connecting to an IMAP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    /// IMAP server hostname (e.g., "imap.gmail.com").
    pub host: String,
    /// IMAP server port (993 for implicit TLS, 143 for STARTTLS/plaintext).
    pub port: u16,
    /// Transport security mode.
    #[serde(default = "default_encryption")]
    pub encryption: ImapEncryption,
    /// Username (typically the email address).
    pub username: String,
    /// Password, app-specific password, or (Outlook) an OAuth bearer token.
    pub auth: ImapAuth,
    /// Provider quirks for label application (milestone 6). Defaults to
    /// `Generic`, i.e. every pre-milestone-6 behavior is unchanged.
    #[serde(default)]
    pub profile: ProviderProfile,
    /// Mailbox to sync from (default: "INBOX").
    #[serde(default = "default_mailbox")]
    pub mailbox: String,
    /// Archive folder name (default: "Archive").
    #[serde(default = "default_archive_folder")]
    pub archive_folder: String,
    /// SMTP server hostname for outbound email (e.g., "smtp.gmail.com").
    #[serde(default)]
    pub smtp_host: Option<String>,
    /// SMTP server port (587 for STARTTLS, 465 for TLS).
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// Pre-resolved, SSRF-validated socket address to connect to. When set,
    /// the TCP connection targets this exact IP (TLS still uses `host` for
    /// SNI/cert validation), preventing a DNS-rebinding TOCTOU between the
    /// SSRF check and the connect. Not serialized/persisted — populated at
    /// request/sync time after the guard runs.
    #[serde(skip)]
    pub pinned_addr: Option<std::net::SocketAddr>,
}

fn default_mailbox() -> String {
    "INBOX".to_string()
}

fn default_archive_folder() -> String {
    "Archive".to_string()
}

fn default_smtp_port() -> u16 {
    587
}

// ---------------------------------------------------------------------------
// Stream + session types
// ---------------------------------------------------------------------------

/// A connected IMAP transport that may be either a plaintext TCP stream or a
/// TLS-wrapped stream. Implementing `AsyncRead`/`AsyncWrite` lets a single
/// `async_imap::Session` type carry any of the three encryption modes.
pub enum ImapStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<async_native_tls::TlsStream<tokio::net::TcpStream>>),
}

impl std::fmt::Debug for ImapStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImapStream::Plain(_) => f.write_str("ImapStream::Plain"),
            ImapStream::Tls(_) => f.write_str("ImapStream::Tls"),
        }
    }
}

impl tokio::io::AsyncRead for ImapStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            ImapStream::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for ImapStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ImapStream::Plain(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            ImapStream::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            ImapStream::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStream::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            ImapStream::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

type ImapSession = async_imap::Session<ImapStream>;

/// SASL XOAUTH2 (<https://developers.google.com/gmail/imap/xoauth2-protocol>,
/// the same mechanism Outlook/Microsoft 365 IMAP expects). There's no real
/// challenge/response: the server sends an empty initial challenge and the
/// client's one reply carries the full credential.
struct XOAuth2Authenticator {
    user: String,
    token: String,
}

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

// ---------------------------------------------------------------------------
// IMAP Provider
// ---------------------------------------------------------------------------

/// IMAP email provider using `async-imap` for async TLS connections.
pub struct ImapProvider {
    config: ImapConfig,
}

impl ImapProvider {
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    /// Validate that the IMAP configuration has the required fields.
    fn validate_config(&self) -> Result<(), ProviderError> {
        if self.config.host.is_empty() {
            return Err(ProviderError::ConfigError(
                "IMAP host is required".to_string(),
            ));
        }
        if self.config.username.is_empty() {
            return Err(ProviderError::ConfigError(
                "IMAP username is required".to_string(),
            ));
        }
        if self.config.auth.is_empty() {
            return Err(ProviderError::ConfigError(
                "IMAP password or OAuth token is required".to_string(),
            ));
        }
        Ok(())
    }

    /// Open a raw TCP connection to the configured server.
    ///
    /// Connects to the pre-resolved, SSRF-validated address when available
    /// (avoids a DNS-rebinding TOCTOU vs. the guard); otherwise resolves the
    /// hostname normally.
    async fn tcp_connect(&self) -> Result<tokio::net::TcpStream, ProviderError> {
        match self.config.pinned_addr {
            Some(addr) => tokio::net::TcpStream::connect(addr).await,
            None => tokio::net::TcpStream::connect((&*self.config.host, self.config.port)).await,
        }
        .map_err(|e| {
            ProviderError::RequestFailed(format!(
                "TCP connect to {}:{} failed: {e}",
                self.config.host, self.config.port
            ))
        })
    }

    /// Wrap a TCP stream in TLS, validating the certificate against `host`.
    ///
    /// Uses the platform trust store with certificate verification ON (no
    /// `danger_accept_invalid_certs`). The hostname is used for SNI and
    /// certificate name validation per RFC 7817.
    async fn tls_upgrade(
        &self,
        tcp: tokio::net::TcpStream,
    ) -> Result<async_native_tls::TlsStream<tokio::net::TcpStream>, ProviderError> {
        async_native_tls::TlsConnector::new()
            .connect(&self.config.host, tcp)
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "TLS handshake with {} failed: {e}",
                    self.config.host
                ))
            })
    }

    /// Establish a connection per the configured encryption mode, read the
    /// greeting, and log in. STARTTLS fails closed: if the upgrade fails we
    /// return an error and never authenticate over cleartext.
    async fn connect(&self) -> Result<ImapSession, ProviderError> {
        let tcp = self.tcp_connect().await?;

        let client = match self.config.encryption {
            ImapEncryption::Ssl => {
                // Implicit TLS: negotiate immediately, then read greeting.
                let tls = self.tls_upgrade(tcp).await?;
                let mut client = async_imap::Client::new(ImapStream::Tls(Box::new(tls)));
                client.read_response().await.map_err(|e| {
                    ProviderError::RequestFailed(format!("Failed to read greeting: {e}"))
                })?;
                client
            }
            ImapEncryption::StartTls => {
                // Plain connect, read greeting, issue STARTTLS, THEN upgrade.
                let mut plain = async_imap::Client::new(ImapStream::Plain(tcp));
                plain.read_response().await.map_err(|e| {
                    ProviderError::RequestFailed(format!("Failed to read greeting: {e}"))
                })?;
                // If STARTTLS is rejected/stripped this errors out — we do NOT
                // continue in cleartext.
                plain
                    .run_command_and_check_ok("STARTTLS", None)
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("STARTTLS command failed: {e}"))
                    })?;
                // Recover the raw TCP stream and upgrade it. There is no
                // greeting after a STARTTLS upgrade.
                let raw = match plain.into_inner() {
                    ImapStream::Plain(tcp) => tcp,
                    // Unreachable: we constructed a Plain client above.
                    ImapStream::Tls(_) => {
                        return Err(ProviderError::RequestFailed(
                            "internal: STARTTLS stream already TLS".to_string(),
                        ))
                    }
                };
                let tls = self.tls_upgrade(raw).await?;
                async_imap::Client::new(ImapStream::Tls(Box::new(tls)))
            }
            ImapEncryption::Plaintext => {
                let mut client = async_imap::Client::new(ImapStream::Plain(tcp));
                client.read_response().await.map_err(|e| {
                    ProviderError::RequestFailed(format!("Failed to read greeting: {e}"))
                })?;
                client
            }
        };

        let session = match &self.config.auth {
            ImapAuth::Password(password) => client
                .login(&self.config.username, password)
                .await
                .map_err(|(e, _client)| {
                    ProviderError::OAuthError(format!("IMAP login failed: {e}"))
                })?,
            ImapAuth::OAuthBearer(token) => client
                .authenticate(
                    "XOAUTH2",
                    XOAuth2Authenticator {
                        user: self.config.username.clone(),
                        token: token.clone(),
                    },
                )
                .await
                .map_err(|(e, _client)| {
                    ProviderError::OAuthError(format!("IMAP XOAUTH2 authentication failed: {e}"))
                })?,
        };

        Ok(session)
    }

    /// Connect, SELECT the configured mailbox, and return (session, exists_count).
    async fn connect_and_select(&self) -> Result<(ImapSession, u32), ProviderError> {
        let mut session = self.connect().await?;
        let mailbox = session
            .select(&self.config.mailbox)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SELECT failed: {e}")))?;

        Ok((session, mailbox.exists))
    }

    /// Move one message (by UID) out of the currently selected mailbox into
    /// `target`, touching no other message. Uses `UID MOVE` (RFC 6851) when
    /// the server has it, else `UID COPY` + `\Deleted` + `UID EXPUNGE`
    /// (UIDPLUS, RFC 4315).
    ///
    /// A server with neither (iCloud is reported to lack MOVE, and sources
    /// disagree on UIDPLUS) falls back to a plain `EXPUNGE`, which erases
    /// every `\Deleted` message in the mailbox. That is only done when a
    /// `UID SEARCH DELETED` shows no other message is marked: checked before
    /// copying (refuse with nothing changed) and again just before the
    /// expunge (unflag ours and refuse). A message another client flags in
    /// the instant between the last check and the expunge is still lost.
    async fn move_uid(
        session: &mut ImapSession,
        uid: &str,
        target: &str,
    ) -> Result<(), ProviderError> {
        let caps = session
            .capabilities()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("CAPABILITY failed: {e}")))?;

        if caps.has_str("MOVE") {
            return session.uid_mv(uid, target).await.map_err(|e| {
                ProviderError::RequestFailed(format!("UID MOVE to '{target}' failed: {e}"))
            });
        }
        if !caps.has_str("UIDPLUS") {
            let ours: u32 = uid.parse().map_err(|_| {
                ProviderError::RequestFailed(format!(
                    "server supports neither MOVE nor UIDPLUS; can only move a single UID, got '{uid}'"
                ))
            })?;
            Self::ensure_only_deleted(session, None, uid).await?;

            session.uid_copy(uid, target).await.map_err(|e| {
                ProviderError::RequestFailed(format!("UID COPY to '{target}' failed: {e}"))
            })?;
            Self::store_deleted(session, uid, "+FLAGS").await?;

            if let Err(e) = Self::ensure_only_deleted(session, Some(ours), uid).await {
                Self::store_deleted(session, uid, "-FLAGS").await?;
                return Err(ProviderError::RequestFailed(format!(
                    "{e}; UID {uid} was already copied to '{target}' and left in place"
                )));
            }

            session
                .expunge()
                .await
                .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE failed: {e}")))?
                .try_collect::<Vec<u32>>()
                .await
                .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE stream failed: {e}")))?;
            return Ok(());
        }

        session.uid_copy(uid, target).await.map_err(|e| {
            ProviderError::RequestFailed(format!("UID COPY to '{target}' failed: {e}"))
        })?;

        Self::flag_and_expunge_uid(session, uid).await
    }

    /// Permanently remove one message (by UID) from the selected mailbox with
    /// `UID EXPUNGE` (UIDPLUS, RFC 4315), leaving every other `\Deleted`
    /// message alone. A server without UIDPLUS is refused before anything is
    /// flagged, for the same reason as `move_uid`.
    async fn delete_uid(session: &mut ImapSession, uid: &str) -> Result<(), ProviderError> {
        let caps = session
            .capabilities()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("CAPABILITY failed: {e}")))?;

        if !caps.has_str("UIDPLUS") {
            return Err(ProviderError::RequestFailed(format!(
                "server does not support UIDPLUS; refusing to delete UID {uid} because a \
                 plain EXPUNGE would also erase other messages marked \\Deleted"
            )));
        }

        Self::flag_and_expunge_uid(session, uid).await
    }

    /// `\Deleted` + `UID EXPUNGE` on one UID. Callers must have confirmed
    /// UIDPLUS first.
    async fn flag_and_expunge_uid(
        session: &mut ImapSession,
        uid: &str,
    ) -> Result<(), ProviderError> {
        Self::store_deleted(session, uid, "+FLAGS").await?;

        session
            .uid_expunge(uid)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID EXPUNGE failed: {e}")))?
            .try_collect::<Vec<u32>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID EXPUNGE stream failed: {e}")))?;

        Ok(())
    }

    /// Refuse unless the set of `\Deleted` messages in the selected mailbox
    /// is exactly `expected` (none, or only our own UID) — the precondition
    /// for a plain `EXPUNGE` to erase nothing else.
    async fn ensure_only_deleted(
        session: &mut ImapSession,
        expected: Option<u32>,
        uid: &str,
    ) -> Result<(), ProviderError> {
        let deleted = session
            .uid_search("DELETED")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID SEARCH DELETED failed: {e}")))?;
        let others = deleted.iter().filter(|d| Some(**d) != expected).count();
        if others > 0 {
            return Err(ProviderError::RequestFailed(format!(
                "server supports neither MOVE nor UIDPLUS and {others} other message(s) in \
                 this mailbox are marked \\Deleted; refusing to move UID {uid} because a \
                 plain EXPUNGE would erase them"
            )));
        }
        Ok(())
    }

    /// `UID STORE <uid> {+FLAGS|-FLAGS} (\Deleted)`.
    async fn store_deleted(
        session: &mut ImapSession,
        uid: &str,
        op: &str,
    ) -> Result<(), ProviderError> {
        session
            .uid_store(uid, format!("{op} (\\Deleted)"))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE \\Deleted failed: {e}")))?
            .try_collect::<Vec<async_imap::types::Fetch>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE stream failed: {e}")))?;
        Ok(())
    }

    /// Parse an `async_imap::types::Fetch` result into our domain `EmailMessage`.
    fn parse_fetched(fetch: &async_imap::types::Fetch) -> Option<EmailMessage> {
        let uid = fetch.uid?;

        // Parse the full RFC822 body with mail-parser for robust MIME handling.
        let body_bytes = fetch.body()?;
        let parsed = mail_parser::MessageParser::default().parse(body_bytes)?;

        let from = parsed
            .from()
            .and_then(|addr| {
                addr.iter().next().and_then(|a| {
                    a.address
                        .as_deref()
                        .map(|s| s.to_string())
                        .or_else(|| a.name.as_deref().map(|s| s.to_string()))
                })
            })
            .unwrap_or_default();

        let to: Vec<String> = parsed
            .to()
            .map(|addr| {
                addr.iter()
                    .filter_map(|a| a.address.as_deref().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let subject = parsed.subject().unwrap_or("").to_string();

        let body_text = parsed.body_text(0).map(|s| s.to_string());
        let body_html = parsed.body_html(0).map(|s| s.to_string());
        let snippet = body_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();

        let date = parsed
            .date()
            .and_then(|dt| Utc.timestamp_opt(dt.to_timestamp(), 0).single())
            .unwrap_or_else(Utc::now);

        // Extract flags.
        let is_read = fetch
            .flags()
            .any(|f| matches!(f, async_imap::types::Flag::Seen));
        let is_flagged = fetch
            .flags()
            .any(|f| matches!(f, async_imap::types::Flag::Flagged));

        let mut labels: Vec<String> = fetch
            .flags()
            .map(|f| match f {
                async_imap::types::Flag::Seen => "\\Seen".to_string(),
                async_imap::types::Flag::Answered => "\\Answered".to_string(),
                async_imap::types::Flag::Flagged => "\\Flagged".to_string(),
                async_imap::types::Flag::Deleted => "\\Deleted".to_string(),
                async_imap::types::Flag::Draft => "\\Draft".to_string(),
                async_imap::types::Flag::Recent => "\\Recent".to_string(),
                async_imap::types::Flag::MayCreate => "\\*".to_string(),
                async_imap::types::Flag::Custom(ref cow) => cow.to_string(),
            })
            .collect();

        if is_flagged && !labels.contains(&"STARRED".to_string()) {
            labels.push("STARRED".to_string());
        }

        // Extract List-Unsubscribe headers via mail-parser's built-in support.
        let list_unsubscribe = extract_header_text(&parsed, "List-Unsubscribe");
        let list_unsubscribe_post = extract_header_text(&parsed, "List-Unsubscribe-Post");
        let cc = extract_cc(&parsed);

        Some(EmailMessage {
            id: uid.to_string(),
            thread_id: parsed.message_id().map(|s| s.to_string()),
            from,
            to,
            cc,
            subject,
            snippet,
            body: body_text,
            body_html,
            labels,
            date,
            is_read,
            list_unsubscribe,
            list_unsubscribe_post,
        })
    }

    /// Parse a header-only `async_imap::types::Fetch` result (from a
    /// `BODY.PEEK[HEADER]` fetch) into an `EmailMessage` with no body.
    ///
    /// Mirrors `parse_fetched` but reads `fetch.header()` instead of
    /// `fetch.body()`, deliberately duplicated rather than shared so that
    /// `parse_fetched` (and the full-body fetch path it serves) is untouched.
    fn parse_fetched_headers(fetch: &async_imap::types::Fetch) -> Option<EmailMessage> {
        let uid = fetch.uid?;

        let header_bytes = fetch.header()?;
        let parsed = mail_parser::MessageParser::default().parse(header_bytes)?;

        let from = parsed
            .from()
            .and_then(|addr| {
                addr.iter().next().and_then(|a| {
                    a.address
                        .as_deref()
                        .map(|s| s.to_string())
                        .or_else(|| a.name.as_deref().map(|s| s.to_string()))
                })
            })
            .unwrap_or_default();

        let to: Vec<String> = parsed
            .to()
            .map(|addr| {
                addr.iter()
                    .filter_map(|a| a.address.as_deref().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let subject = parsed.subject().unwrap_or("").to_string();

        let date = parsed
            .date()
            .and_then(|dt| Utc.timestamp_opt(dt.to_timestamp(), 0).single())
            .unwrap_or_else(Utc::now);

        let is_read = fetch
            .flags()
            .any(|f| matches!(f, async_imap::types::Flag::Seen));
        let is_flagged = fetch
            .flags()
            .any(|f| matches!(f, async_imap::types::Flag::Flagged));

        let mut labels: Vec<String> = fetch
            .flags()
            .map(|f| match f {
                async_imap::types::Flag::Seen => "\\Seen".to_string(),
                async_imap::types::Flag::Answered => "\\Answered".to_string(),
                async_imap::types::Flag::Flagged => "\\Flagged".to_string(),
                async_imap::types::Flag::Deleted => "\\Deleted".to_string(),
                async_imap::types::Flag::Draft => "\\Draft".to_string(),
                async_imap::types::Flag::Recent => "\\Recent".to_string(),
                async_imap::types::Flag::MayCreate => "\\*".to_string(),
                async_imap::types::Flag::Custom(ref cow) => cow.to_string(),
            })
            .collect();

        if is_flagged && !labels.contains(&"STARRED".to_string()) {
            labels.push("STARRED".to_string());
        }

        let list_unsubscribe = extract_header_text(&parsed, "List-Unsubscribe");
        let list_unsubscribe_post = extract_header_text(&parsed, "List-Unsubscribe-Post");
        let cc = extract_cc(&parsed);

        Some(EmailMessage {
            id: uid.to_string(),
            thread_id: parsed.message_id().map(|s| s.to_string()),
            from,
            to,
            cc,
            subject,
            snippet: String::new(),
            body: None,
            body_html: None,
            labels,
            date,
            is_read,
            list_unsubscribe,
            list_unsubscribe_post,
        })
    }

    /// List message headers only (From/To/Subject/Date/flags/List-Unsubscribe),
    /// never the body, via `BODY.PEEK[HEADER]`.
    ///
    /// Unlike `list_messages`, this (a) never transfers body bytes over the
    /// wire, matching the mail-agent plan's "headers only, never bodies"
    /// principle, and (b) never marks a message `\Seen` as a side effect,
    /// because `PEEK` suppresses that (plain `BODY[HEADER]`/`RFC822.HEADER`
    /// would not). Milestone 1's header-rule Kind classifier uses this
    /// instead of `list_messages`.
    pub async fn list_message_headers(
        &self,
        params: &ListParams,
    ) -> Result<EmailPage, ProviderError> {
        self.validate_config()?;

        let (mut session, exists) = self.connect_and_select().await?;

        let mut criteria = Vec::new();
        if let Some(ref label) = params.label {
            criteria.push(format!("KEYWORD {label}"));
        }
        if let Some(ref query) = params.query {
            criteria.push(format!("TEXT \"{query}\""));
        }
        let search_str = if criteria.is_empty() {
            "ALL".to_string()
        } else {
            criteria.join(" ")
        };

        let uids = session
            .uid_search(&search_str)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID SEARCH failed: {e}")))?;

        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        uid_list.sort_unstable_by(|a, b| b.cmp(a));

        let start_after: u32 = params
            .page_token
            .as_ref()
            .and_then(|t| t.parse().ok())
            .unwrap_or(u32::MAX);

        let paginated: Vec<u32> = uid_list
            .into_iter()
            .filter(|&uid| uid < start_after)
            .take(params.max_results as usize + 1)
            .collect();

        let has_more = paginated.len() > params.max_results as usize;
        let fetch_uids: Vec<u32> = paginated
            .into_iter()
            .take(params.max_results as usize)
            .collect();

        if fetch_uids.is_empty() {
            let _ = session.logout().await;
            return Ok(EmailPage {
                messages: vec![],
                next_page_token: None,
                result_size_estimate: Some(exists),
            });
        }

        let next_page_token = if has_more {
            fetch_uids.last().map(|uid| uid.to_string())
        } else {
            None
        };

        let uid_set = fetch_uids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");

        debug!(uid_set = %uid_set, count = fetch_uids.len(), "Fetching IMAP message headers (no body)");

        let fetches = session
            .uid_fetch(&uid_set, "(FLAGS BODY.PEEK[HEADER])")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID FETCH failed: {e}")))?;

        let raw_messages: Vec<_> = fetches
            .try_collect()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("FETCH stream failed: {e}")))?;

        let messages: Vec<EmailMessage> = raw_messages
            .iter()
            .filter_map(Self::parse_fetched_headers)
            .collect();

        let _ = session.logout().await;

        Ok(EmailPage {
            messages,
            next_page_token,
            result_size_estimate: Some(exists),
        })
    }

    /// Modify flags on a message by UID.
    async fn store_flags(
        &self,
        uid: &str,
        command: &str,
        flags: &str,
    ) -> Result<(), ProviderError> {
        let (mut session, _) = self.connect_and_select().await?;

        session
            .uid_store(uid, format!("{command} ({flags})"))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE failed: {e}")))?
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE stream failed: {e}")))?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Which `Kind/`/`Topic/`/`State/` namespace (or none) a label string
    /// belongs to (`cleanup::domain::label_namespace`'s constants/prefixes).
    fn namespace_of(label: &str) -> LabelNamespace {
        if label.starts_with("Kind/") {
            LabelNamespace::Kind
        } else if label.starts_with("Topic/") {
            LabelNamespace::Topic
        } else if label.starts_with("State/") {
            LabelNamespace::State
        } else {
            LabelNamespace::Other
        }
    }

    /// Reject a label that is not a valid IMAP keyword before it is
    /// interpolated into a raw `STORE` command. RFC 3501 §9: a keyword is an
    /// `atom` — printable ASCII except `( ) { SP CTL % * " \ ]`. A space would
    /// split one label into two keywords; `)` or CR/LF would end the flag
    /// list and inject further command text.
    fn validate_keyword(label: &str) -> Result<(), ProviderError> {
        let is_atom_char = |c: char| {
            c.is_ascii_graphic() && !matches!(c, '(' | ')' | '{' | '%' | '*' | '"' | '\\' | ']')
        };
        if label.is_empty() || !label.chars().all(is_atom_char) {
            return Err(ProviderError::ConfigError(format!(
                "label '{label}' is not a valid IMAP keyword (printable ASCII only, no spaces \
                 or any of ( ) {{ % * \" \\ ])"
            )));
        }
        Ok(())
    }

    /// Whether the selected mailbox will keep these labels as keywords.
    /// RFC 3501 §7.1: `\*` in `PERMANENTFLAGS` means new keywords can be
    /// created; a keyword listed by name is also permanent; and when the
    /// server sends no `PERMANENTFLAGS` at all, every flag is permanent.
    /// `async-imap` reports that last case as an empty list.
    fn keywords_accepted(permanent: &[async_imap::types::Flag<'_>], labels: &[String]) -> bool {
        use async_imap::types::Flag;
        permanent.is_empty()
            || permanent.iter().any(|f| matches!(f, Flag::MayCreate))
            || labels.iter().all(|label| {
                permanent
                    .iter()
                    .any(|f| matches!(f, Flag::Custom(c) if c == label))
            })
    }

    /// `Generic` profile: IMAP keywords when the mailbox accepts them,
    /// otherwise the folder strategy `OutlookImap` uses (`label_via_folders` /
    /// `unlabel_via_folders`). Every label is validated before any `STORE`,
    /// so a bad label never leaves the others half-applied.
    async fn generic_labels(
        &self,
        id: &str,
        labels: &[String],
        op: &str,
    ) -> Result<(), ProviderError> {
        let mut session = self.connect().await?;
        let mailbox = session
            .select(&self.config.mailbox)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SELECT failed: {e}")))?;

        if !Self::keywords_accepted(&mailbox.permanent_flags, labels) {
            let _ = session.logout().await;
            debug!("server does not accept custom keywords; routing labels through folders");
            return if op == "+FLAGS" {
                self.label_via_folders(id, labels).await
            } else {
                Self::unlabel_via_folders(labels)
            };
        }

        for label in labels {
            if let Err(e) = Self::validate_keyword(label) {
                let _ = session.logout().await;
                return Err(e);
            }
        }

        session
            .uid_store(id, format!("{op} ({})", labels.join(" ")))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE failed: {e}")))?
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE stream failed: {e}")))?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Labels on a server without custom keywords (Outlook, or any server
    /// whose `PERMANENTFLAGS` lacks `\*`): `Kind/`/`Topic/` become a real
    /// folder the message is moved into; `State/` stays in the ledger only.
    async fn label_via_folders(&self, id: &str, labels: &[String]) -> Result<(), ProviderError> {
        for label in labels {
            match Self::namespace_of(label) {
                LabelNamespace::Kind | LabelNamespace::Topic => {
                    self.outlook_move_to_label_folder(id, label).await?;
                }
                LabelNamespace::State => {
                    debug!(
                        label = %label,
                        "server has no custom keywords; State is ledger-only, not written to the provider"
                    );
                }
                LabelNamespace::Other => {
                    return Err(ProviderError::ConfigError(format!(
                        "this server has no custom keywords: it supports Kind/Topic (as folders) \
                         and State (ledger-only), not the generic keyword label '{label}'"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Counterpart of `label_via_folders`: `State/` removal is a no-op (never
    /// written); Kind/Topic undo goes through `move_message`.
    fn unlabel_via_folders(labels: &[String]) -> Result<(), ProviderError> {
        for label in labels {
            if Self::namespace_of(label) != LabelNamespace::State {
                return Err(ProviderError::ConfigError(format!(
                    "this server has no custom keywords: removing '{label}' isn't supported — \
                     Kind/Topic undo goes through move_message, not remove_labels"
                )));
            }
        }
        Ok(())
    }

    /// Fetch the RFC 822 `Message-ID` of a message in the currently-selected
    /// mailbox, by UID. Used to re-find a Proton `COPY`'d message in a
    /// `Labels/*` mailbox, where it has a different UID.
    async fn fetch_message_id(
        session: &mut ImapSession,
        uid: &str,
    ) -> Result<String, ProviderError> {
        let fetches = session
            .uid_fetch(uid, "BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)]")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID FETCH failed: {e}")))?
            .try_collect::<Vec<async_imap::types::Fetch>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("FETCH stream failed: {e}")))?;

        let header_bytes = fetches
            .first()
            .and_then(|f| f.header())
            .ok_or_else(|| ProviderError::NotFound(format!("Message UID {uid} not found")))?;

        let parsed = mail_parser::MessageParser::default()
            .parse(header_bytes)
            .ok_or_else(|| {
                ProviderError::ParseError(format!("could not parse headers for UID {uid}"))
            })?;

        // mail-parser returns the id without its angle brackets; the SEARCH
        // term (and the validator) use the header's on-the-wire `<...>` form.
        let message_id = parsed
            .message_id()
            .map(|s| format!("<{s}>"))
            .ok_or_else(|| {
                ProviderError::ConfigError(format!(
                "message UID {uid} has no Message-ID header; Proton cross-mailbox dedupe needs one"
            ))
            })?;

        Self::validate_message_id_for_imap_search(&message_id)?;
        Ok(message_id)
    }

    /// Reject a `Message-ID` before it's interpolated into a raw IMAP
    /// `SEARCH` command string. A crafted header value containing `"`,
    /// `\`, CR, LF, or other control characters could otherwise break out
    /// of the quoted search term and inject additional search criteria —
    /// RFC 5322 message IDs are `<id-left@id-right>` built from `dot-atom`
    /// (a restricted, printable-ASCII, no-quote/no-backslash charset), so
    /// this is a correctness check, not a lossy escaping hack.
    fn validate_message_id_for_imap_search(message_id: &str) -> Result<(), ProviderError> {
        let reject = || {
            ProviderError::ConfigError(
                "Message-ID contains characters not valid in RFC 5322 (rejected before use in \
                 an IMAP SEARCH command)"
                    .to_string(),
            )
        };

        if !message_id.starts_with('<') || !message_id.ends_with('>') {
            return Err(reject());
        }
        let inner = &message_id[1..message_id.len() - 1];
        if inner.matches('@').count() != 1 {
            return Err(reject());
        }
        let is_safe_char = |c: char| {
            c.is_ascii_graphic() && c != '"' && c != '\\' && c != '(' && c != ')' && c != '{'
        };
        if inner.is_empty() || !inner.chars().all(is_safe_char) {
            return Err(reject());
        }
        Ok(())
    }

    /// Proton Bridge: ensure `Labels/<name>` exists and non-destructively
    /// `COPY` the message into it (a message keeps every label it's ever
    /// had, unlike the exclusive-folder `move_message` path).
    async fn proton_add_label(&self, id: &str, label: &str) -> Result<(), ProviderError> {
        let target = format!("Labels/{label}");
        let (mut session, _) = self.connect_and_select().await?;

        if let Err(e) = session.create(&target).await {
            let msg = e.to_string();
            if !msg.contains("ALREADYEXISTS") && !msg.contains("already exists") {
                let _ = session.logout().await;
                return Err(ProviderError::RequestFailed(format!(
                    "CREATE '{target}' failed: {e}"
                )));
            }
        }

        session.uid_copy(id, &target).await.map_err(|e| {
            ProviderError::RequestFailed(format!("UID COPY to '{target}' failed: {e}"))
        })?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Proton Bridge: remove a message from `Labels/<name>` only — find it
    /// by `Message-ID` (its UID there differs from the source mailbox's),
    /// then `STORE \Deleted` + `UID EXPUNGE` of that one UID (`delete_uid`).
    /// The source mailbox, every other label the message carries, and any
    /// other `\Deleted` message in the label mailbox are untouched.
    async fn proton_remove_label(&self, id: &str, label: &str) -> Result<(), ProviderError> {
        let target = format!("Labels/{label}");

        let (mut source_session, _) = self.connect_and_select().await?;
        let message_id = Self::fetch_message_id(&mut source_session, id).await?;
        let _ = source_session.logout().await;

        let mut session = self.connect().await?;
        session
            .select(&target)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SELECT '{target}' failed: {e}")))?;

        let found: std::collections::HashSet<u32> = session
            .uid_search(format!("HEADER Message-ID \"{message_id}\""))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID SEARCH failed: {e}")))?;

        let target_uid = match found.len() {
            0 => {
                // Already not in this label — treat as success (idempotent).
                let _ = session.logout().await;
                return Ok(());
            }
            1 => *found.iter().next().expect("len checked above"),
            n => {
                let _ = session.logout().await;
                return Err(ProviderError::RequestFailed(format!(
                    "{n} messages in '{target}' share Message-ID {message_id}; refusing to \
                     expunge an ambiguous match"
                )));
            }
        };

        Self::delete_uid(&mut session, &target_uid.to_string()).await?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Outlook IMAP has no custom keyword flags, so `Kind/`/`Topic/` labels
    /// are redirected to a real, exclusive folder named after the label
    /// (creating it if needed) via the same `move_uid` move
    /// `move_message`'s `MoveKind::Folder` path uses.
    async fn outlook_move_to_label_folder(
        &self,
        id: &str,
        label: &str,
    ) -> Result<(), ProviderError> {
        self.create_label("", label).await?;

        let (mut session, _) = self.connect_and_select().await?;
        Self::move_uid(&mut session, id, label).await?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Send an email via SMTP using the configured SMTP server.
    #[allow(clippy::too_many_arguments)]
    async fn smtp_send(
        &self,
        to: &str,
        cc: Option<&str>,
        bcc: Option<&str>,
        subject: &str,
        in_reply_to: Option<&str>,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> Result<String, ProviderError> {
        use lettre::message::{header, Mailbox, MessageBuilder};
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

        let smtp_host =
            self.config.smtp_host.as_deref().ok_or_else(|| {
                ProviderError::ConfigError("SMTP host not configured".to_string())
            })?;

        let from: Mailbox = self
            .config
            .username
            .parse()
            .map_err(|e| ProviderError::ConfigError(format!("Invalid from address: {e}")))?;

        let mut builder = MessageBuilder::new().from(from).subject(subject);

        // Parse recipients.
        for addr in to.split(',') {
            let addr = addr.trim();
            if !addr.is_empty() {
                let mbox: Mailbox = addr.parse().map_err(|e| {
                    ProviderError::RequestFailed(format!("Invalid To address: {e}"))
                })?;
                builder = builder.to(mbox);
            }
        }

        if let Some(cc) = cc {
            for addr in cc.split(',') {
                let addr = addr.trim();
                if !addr.is_empty() {
                    let mbox: Mailbox = addr.parse().map_err(|e| {
                        ProviderError::RequestFailed(format!("Invalid Cc address: {e}"))
                    })?;
                    builder = builder.cc(mbox);
                }
            }
        }

        if let Some(bcc) = bcc {
            for addr in bcc.split(',') {
                let addr = addr.trim();
                if !addr.is_empty() {
                    let mbox: Mailbox = addr.parse().map_err(|e| {
                        ProviderError::RequestFailed(format!("Invalid Bcc address: {e}"))
                    })?;
                    builder = builder.bcc(mbox);
                }
            }
        }

        if let Some(reply_id) = in_reply_to {
            builder = builder.references(format!("<{reply_id}>"));
        }

        let message = if let Some(html) = body_html {
            builder
                .header(header::ContentType::TEXT_HTML)
                .body(html.to_string())
        } else {
            builder
                .header(header::ContentType::TEXT_PLAIN)
                .body(body_text.unwrap_or("").to_string())
        }
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to build message: {e}")))?;

        // XOAUTH2 submission isn't implemented — milestone 6's Outlook work is
        // IMAP-only (read + label), and the chief-of-staff tool surface never
        // sends mail anyway (AGENTS.md). Fail clearly rather than sending an
        // empty password.
        let password = match &self.config.auth {
            ImapAuth::Password(p) => p.clone(),
            ImapAuth::OAuthBearer(_) => {
                return Err(ProviderError::ConfigError(
                    "SMTP over OAuth (XOAUTH2) is not implemented for this account".to_string(),
                ))
            }
        };
        let creds = Credentials::new(self.config.username.clone(), password);

        // Choose the SMTP transport security to match the account's mode:
        // - Ssl       -> implicit TLS (typically port 465)
        // - StartTls  -> STARTTLS upgrade (typically port 587)
        // - Plaintext -> unencrypted (local/trusted relays only)
        // TLS modes validate the server certificate (lettre default).
        let builder = match self.config.encryption {
            ImapEncryption::Ssl => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host).map_err(|e| {
                    ProviderError::RequestFailed(format!("SMTP connection failed: {e}"))
                })?
            }
            ImapEncryption::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host).map_err(|e| {
                    ProviderError::RequestFailed(format!("SMTP connection failed: {e}"))
                })?
            }
            ImapEncryption::Plaintext => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(smtp_host)
            }
        };

        let mailer: AsyncSmtpTransport<Tokio1Executor> = builder
            .port(self.config.smtp_port)
            .credentials(creds)
            .build();

        mailer
            .send(message)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SMTP send failed: {e}")))?;

        Ok(String::new())
    }
}

/// Extract a header value as text from a parsed message.
fn extract_header_text(msg: &mail_parser::Message<'_>, name: &str) -> Option<String> {
    // `name` must be passed as-is (not wrapped in `HeaderName::Other`):
    // mail-parser recognizes well-known headers like `List-Unsubscribe` as
    // typed `HeaderName` variants (`HeaderName::ListUnsubscribe`, stored
    // and matched as that variant, never as `Other`), via
    // `HeaderName::parse` through the `From<&str>` impl `header_values`
    // accepts. Forcing `Other` here, as a prior version of this function
    // did, silently never matches any header mail-parser recognizes by
    // name — found via milestone 1's golden-mailbox fixtures, which were
    // the first test to round-trip a real `List-Unsubscribe` header
    // through this exact path end-to-end.
    msg.header_values(name).find_map(|hv| match hv {
        mail_parser::HeaderValue::Text(t) => Some(t.to_string()),
        mail_parser::HeaderValue::Address(addr) => {
            // List-Unsubscribe may be parsed as an address list.
            let urls: Vec<String> = addr
                .iter()
                .filter_map(|a| a.address.as_deref().map(|s| s.to_string()))
                .collect();
            if urls.is_empty() {
                None
            } else {
                Some(urls.join(", "))
            }
        }
        _ => None,
    })
}

/// Extract Cc addresses. Milestone 1's Kind classifier reads this to
/// sub-route GitHub notifications by the reason encoded in the cc address.
fn extract_cc(msg: &mail_parser::Message<'_>) -> Vec<String> {
    msg.cc()
        .map(|addr| {
            addr.iter()
                .filter_map(|a| a.address.as_deref().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl EmailProvider for ImapProvider {
    /// Authenticate by validating credentials against the IMAP server.
    async fn authenticate(&self, _auth_code: &str) -> Result<OAuthTokens, ProviderError> {
        self.validate_config()?;

        // Attempt a real login to verify credentials, then disconnect.
        let mut session = self.connect().await?;
        let _ = session.logout().await;

        Ok(OAuthTokens {
            access_token: format!(
                "imap://{}@{}:{}",
                self.config.username, self.config.host, self.config.port
            ),
            refresh_token: None,
            expires_at: None,
            email: Some(self.config.username.clone()),
        })
    }

    /// IMAP uses persistent credentials; no refresh needed.
    async fn refresh_token(&self, _refresh_token: &str) -> Result<OAuthTokens, ProviderError> {
        self.authenticate("").await
    }

    /// List messages from the configured IMAP mailbox with pagination.
    ///
    /// Pagination uses UID ranges: `page_token` holds the last-seen UID.
    /// `result_size_estimate` is populated from the mailbox's EXISTS count.
    async fn list_messages(
        &self,
        _access_token: &str,
        params: &ListParams,
    ) -> Result<EmailPage, ProviderError> {
        self.validate_config()?;

        let (mut session, exists) = self.connect_and_select().await?;

        // Build SEARCH criteria.
        let mut criteria = Vec::new();
        if let Some(ref label) = params.label {
            criteria.push(format!("KEYWORD {label}"));
        }
        if let Some(ref query) = params.query {
            criteria.push(format!("TEXT \"{query}\""));
        }
        let search_str = if criteria.is_empty() {
            "ALL".to_string()
        } else {
            criteria.join(" ")
        };

        // Search for matching UIDs.
        let uids = session
            .uid_search(&search_str)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID SEARCH failed: {e}")))?;

        let mut uid_list: Vec<u32> = uids.into_iter().collect();
        // Sort descending (newest first) for consistent pagination.
        uid_list.sort_unstable_by(|a, b| b.cmp(a));

        // Apply pagination: skip past UIDs we've already seen.
        let start_after: u32 = params
            .page_token
            .as_ref()
            .and_then(|t| t.parse().ok())
            .unwrap_or(u32::MAX);

        let paginated: Vec<u32> = uid_list
            .into_iter()
            .filter(|&uid| uid < start_after)
            .take(params.max_results as usize + 1)
            .collect();

        let has_more = paginated.len() > params.max_results as usize;
        let fetch_uids: Vec<u32> = paginated
            .into_iter()
            .take(params.max_results as usize)
            .collect();

        if fetch_uids.is_empty() {
            let _ = session.logout().await;
            return Ok(EmailPage {
                messages: vec![],
                next_page_token: None,
                result_size_estimate: Some(exists),
            });
        }

        let next_page_token = if has_more {
            fetch_uids.last().map(|uid| uid.to_string())
        } else {
            None
        };

        // FETCH the messages by UID.
        let uid_set = fetch_uids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");

        debug!(uid_set = %uid_set, count = fetch_uids.len(), "Fetching IMAP messages");

        // BODY.PEEK[] (not RFC822) -- same full-message bytes (both populate
        // Fetch::body() identically; see async-imap's BodySection{section:
        // None} match), but PEEK suppresses the implicit \Seen the server
        // would otherwise set on every message this ever looks at (RFC 3501
        // §6.4.5) -- plain RFC822 does not have a PEEK form.
        let fetches = session
            .uid_fetch(&uid_set, "(FLAGS BODY.PEEK[])")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID FETCH failed: {e}")))?;

        let raw_messages: Vec<_> = fetches
            .try_collect()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("FETCH stream failed: {e}")))?;

        let messages: Vec<EmailMessage> = raw_messages
            .iter()
            .filter_map(Self::parse_fetched)
            .collect();

        let _ = session.logout().await;

        Ok(EmailPage {
            messages,
            next_page_token,
            result_size_estimate: Some(exists),
        })
    }

    /// Get a single message by its IMAP UID.
    async fn get_message(
        &self,
        _access_token: &str,
        id: &str,
    ) -> Result<EmailMessage, ProviderError> {
        self.validate_config()?;

        let (mut session, _) = self.connect_and_select().await?;

        // See list_messages above: BODY.PEEK[] instead of RFC822, same
        // bytes, no implicit \Seen.
        let fetches = session
            .uid_fetch(id, "(FLAGS BODY.PEEK[])")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID FETCH failed: {e}")))?;

        let raw: Vec<_> = fetches
            .try_collect()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("FETCH stream failed: {e}")))?;

        let msg = raw
            .first()
            .and_then(Self::parse_fetched)
            .ok_or_else(|| ProviderError::NotFound(format!("Message UID {id} not found")))?;

        let _ = session.logout().await;
        Ok(msg)
    }

    /// Archive a message by moving it to the archive folder.
    async fn archive_message(&self, _access_token: &str, id: &str) -> Result<(), ProviderError> {
        self.validate_config()?;

        let (mut session, _) = self.connect_and_select().await?;
        Self::move_uid(&mut session, id, &self.config.archive_folder).await?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Apply labels to a message. `Generic` (Gmail/Workspace/iCloud/Fastmail/
    /// Yahoo app-password) writes IMAP keywords when the mailbox accepts them
    /// and falls back to folders when it doesn't (`generic_labels`).
    /// `ProtonBridge` and `OutlookImap` route per-label through the
    /// provider-specific strategy described on `ProviderProfile`.
    async fn label_message(
        &self,
        _access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        match self.config.profile {
            ProviderProfile::Generic => self.generic_labels(id, labels, "+FLAGS").await,
            ProviderProfile::ProtonBridge => {
                for label in labels {
                    self.proton_add_label(id, label).await?;
                }
                Ok(())
            }
            ProviderProfile::OutlookImap => self.label_via_folders(id, labels).await,
        }
    }

    /// Remove labels from a message. See `label_message` for the
    /// per-profile strategy; on a server without custom keywords (Outlook,
    /// or a `Generic` server whose mailbox refuses them) `State/` removal is
    /// a no-op for the same reason State is never applied there.
    async fn remove_labels(
        &self,
        _access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        match self.config.profile {
            ProviderProfile::Generic => self.generic_labels(id, labels, "-FLAGS").await,
            ProviderProfile::ProtonBridge => {
                for label in labels {
                    self.proton_remove_label(id, label).await?;
                }
                Ok(())
            }
            ProviderProfile::OutlookImap => Self::unlabel_via_folders(labels),
        }
    }

    /// Create a label. `Generic`/`OutlookImap` create an IMAP folder
    /// (mailbox) named `name`, unchanged from pre-milestone-6 behavior.
    /// `ProtonBridge` creates the `Labels/<name>` mailbox instead, since
    /// that's the mailbox `proton_add_label`/`proton_remove_label` target.
    async fn create_label(&self, _access_token: &str, name: &str) -> Result<String, ProviderError> {
        self.validate_config()?;

        let mailbox_name = match self.config.profile {
            ProviderProfile::ProtonBridge => format!("Labels/{name}"),
            ProviderProfile::Generic | ProviderProfile::OutlookImap => name.to_string(),
        };

        let mut session = self.connect().await?;

        match session.create(&mailbox_name).await {
            Ok(()) => {}
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("ALREADYEXISTS") && !msg.contains("already exists") {
                    let _ = session.logout().await;
                    return Err(ProviderError::RequestFailed(format!(
                        "CREATE '{mailbox_name}' failed: {e}"
                    )));
                }
                debug!(folder = %mailbox_name, "Folder already exists, treating as success");
            }
        }

        let _ = session.logout().await;
        Ok(name.to_string())
    }

    /// List all IMAP mailboxes as label tuples.
    async fn list_labels(
        &self,
        _access_token: &str,
    ) -> Result<Vec<(String, String)>, ProviderError> {
        self.validate_config()?;

        let mut session = self.connect().await?;

        let mailboxes: Vec<async_imap::types::Name> = session
            .list(Some(""), Some("*"))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("LIST failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("LIST stream failed: {e}")))?;

        let result: Vec<(String, String)> = mailboxes
            .iter()
            .map(|mb| {
                let name = mb.name().to_string();
                (name.clone(), name)
            })
            .collect();

        let _ = session.logout().await;
        Ok(result)
    }

    /// Delete an IMAP mailbox.
    async fn delete_label(&self, _access_token: &str, label_id: &str) -> Result<(), ProviderError> {
        self.validate_config()?;

        let mut session = self.connect().await?;

        session.delete(label_id).await.map_err(|e| {
            ProviderError::RequestFailed(format!("DELETE '{label_id}' failed: {e}"))
        })?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Move a message back to INBOX from the archive folder.
    async fn unarchive_message(&self, _access_token: &str, id: &str) -> Result<(), ProviderError> {
        self.validate_config()?;

        let mut session = self.connect().await?;

        // SELECT the archive folder, then move the message back to INBOX.
        session
            .select(&self.config.archive_folder)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SELECT Archive failed: {e}")))?;

        Self::move_uid(&mut session, id, "INBOX").await?;

        let _ = session.logout().await;
        Ok(())
    }

    /// List all IMAP mailboxes as folders.
    async fn list_folders(&self, _access_token: &str) -> Result<Vec<FolderOrLabel>, ProviderError> {
        self.validate_config()?;

        let mut session = self.connect().await?;

        let mailboxes: Vec<async_imap::types::Name> = session
            .list(Some(""), Some("*"))
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("LIST failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("LIST stream failed: {e}")))?;

        let system_names = [
            "INBOX",
            "Sent",
            "Sent Messages",
            "Sent Items",
            "Drafts",
            "Trash",
            "Deleted Items",
            "Deleted Messages",
            "Spam",
            "Junk",
            "Junk E-Mail",
            "Archive",
            "Outbox",
        ];

        let result: Vec<FolderOrLabel> = mailboxes
            .iter()
            .map(|mb| {
                let name = mb.name().to_string();
                let is_system = system_names.iter().any(|s| s.eq_ignore_ascii_case(&name));
                FolderOrLabel {
                    id: name.clone(),
                    name,
                    kind: MoveKind::Folder,
                    is_system,
                }
            })
            .collect();

        let _ = session.logout().await;
        Ok(result)
    }

    /// Move a message to a target folder or add a keyword label.
    async fn move_message(
        &self,
        _access_token: &str,
        message_id: &str,
        target_id: &str,
        kind: MoveKind,
    ) -> Result<(), ProviderError> {
        self.validate_config()?;

        match kind {
            MoveKind::Folder => {
                let (mut session, _) = self.connect_and_select().await?;
                Self::move_uid(&mut session, message_id, target_id).await?;
                let _ = session.logout().await;
            }
            MoveKind::Label => {
                Self::validate_keyword(target_id)?;
                self.store_flags(message_id, "+FLAGS", target_id).await?;
            }
        }

        Ok(())
    }

    /// Mark a message as read (\\Seen) or unread.
    async fn mark_read(
        &self,
        _access_token: &str,
        message_id: &str,
        read: bool,
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        let cmd = if read { "+FLAGS" } else { "-FLAGS" };
        self.store_flags(message_id, cmd, "\\Seen").await
    }

    /// Star (\\Flagged) or unstar a message.
    async fn star_message(
        &self,
        _access_token: &str,
        message_id: &str,
        starred: bool,
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        let cmd = if starred { "+FLAGS" } else { "-FLAGS" };
        self.store_flags(message_id, cmd, "\\Flagged").await
    }

    async fn send_message(
        &self,
        _access_token: &str,
        draft: &SendDraft<'_>,
    ) -> Result<String, ProviderError> {
        self.smtp_send(
            draft.to,
            draft.cc,
            draft.bcc,
            draft.subject,
            None,
            draft.body_text,
            draft.body_html,
        )
        .await
    }

    async fn reply_to_message(
        &self,
        _access_token: &str,
        message_id: &str,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> Result<String, ProviderError> {
        // Fetch original to get From (reply-to) and Subject.
        let original = self.get_message("", message_id).await?;
        let to = &original.from;
        let subject = if original.subject.starts_with("Re: ") {
            original.subject.clone()
        } else {
            format!("Re: {}", original.subject)
        };

        self.smtp_send(
            to,
            None,
            None,
            &subject,
            Some(message_id),
            body_text,
            body_html,
        )
        .await
    }

    async fn forward_message(
        &self,
        _access_token: &str,
        message_id: &str,
        to: &str,
    ) -> Result<String, ProviderError> {
        let original = self.get_message("", message_id).await?;
        let subject = if original.subject.starts_with("Fwd: ") {
            original.subject.clone()
        } else {
            format!("Fwd: {}", original.subject)
        };

        let fwd_body = format!(
            "---------- Forwarded message ----------\nFrom: {}\nSubject: {}\n\n{}",
            original.from,
            original.subject,
            original.body.as_deref().unwrap_or(""),
        );

        self.smtp_send(to, None, None, &subject, None, Some(&fwd_body), None)
            .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_provider_profile_recognizes_proton_bridge_by_host() {
        assert_eq!(
            detect_provider_profile("127.0.0.1", 1143),
            ProviderProfile::ProtonBridge
        );
        assert_eq!(
            detect_provider_profile("mail.protonmail.ch", 993),
            ProviderProfile::ProtonBridge
        );
    }

    #[test]
    fn detect_provider_profile_recognizes_outlook_by_host() {
        assert_eq!(
            detect_provider_profile("outlook.office365.com", 993),
            ProviderProfile::OutlookImap
        );
    }

    #[test]
    fn detect_provider_profile_defaults_to_generic() {
        assert_eq!(
            detect_provider_profile("imap.fastmail.com", 993),
            ProviderProfile::Generic
        );
        assert_eq!(
            detect_provider_profile("imap.mail.me.com", 993),
            ProviderProfile::Generic
        );
        // A loopback host on a non-Bridge port is NOT assumed to be Proton.
        assert_eq!(
            detect_provider_profile("127.0.0.1", 993),
            ProviderProfile::Generic
        );
    }

    #[test]
    fn provider_profile_round_trips_through_its_stored_string() {
        for p in [
            ProviderProfile::Generic,
            ProviderProfile::ProtonBridge,
            ProviderProfile::OutlookImap,
        ] {
            assert_eq!(ProviderProfile::from_stored_str(p.as_str()), p);
        }
        // An unrecognized/legacy value fails closed to Generic, not an error.
        assert_eq!(
            ProviderProfile::from_stored_str("something-unexpected"),
            ProviderProfile::Generic
        );
    }

    #[test]
    fn validate_keyword_accepts_namespace_labels() {
        for ok in [
            "Kind/Newsletters",
            "Topic/Billing_2026",
            "State/ReadLater",
            "$Label1",
        ] {
            assert!(ImapProvider::validate_keyword(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn validate_keyword_rejects_non_atoms() {
        for bad in [
            "",
            "Topic/Read Later",
            "Bad)Label",
            "x\r\nA1 DELETE INBOX",
            "a(b",
            "a{3}",
            "100%",
            "a*",
            "say\"hi",
            "back\\slash",
            "a]b",
            "Topic/Café",
        ] {
            assert!(ImapProvider::validate_keyword(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn keywords_accepted_follows_rfc_3501_permanentflags() {
        use async_imap::types::Flag;
        let labels = vec!["Kind/Newsletters".to_string()];

        // No PERMANENTFLAGS response: every flag is permanent.
        assert!(ImapProvider::keywords_accepted(&[], &labels));
        // `\*` present: new keywords may be created.
        assert!(ImapProvider::keywords_accepted(
            &[Flag::Seen, Flag::Deleted, Flag::MayCreate],
            &labels
        ));
        // Only system flags: custom keywords are not kept.
        assert!(!ImapProvider::keywords_accepted(
            &[Flag::Seen, Flag::Deleted],
            &labels
        ));
        // The keyword itself is listed by name.
        assert!(ImapProvider::keywords_accepted(
            &[Flag::Seen, Flag::Custom("Kind/Newsletters".into())],
            &labels
        ));
    }

    #[test]
    fn namespace_of_classifies_kind_topic_state_and_everything_else() {
        assert_eq!(
            ImapProvider::namespace_of("Kind/People"),
            LabelNamespace::Kind
        );
        assert_eq!(
            ImapProvider::namespace_of("Topic/Acme"),
            LabelNamespace::Topic
        );
        assert_eq!(
            ImapProvider::namespace_of("State/New"),
            LabelNamespace::State
        );
        assert_eq!(
            ImapProvider::namespace_of("Emailibrium/Work"),
            LabelNamespace::Other
        );
    }

    #[test]
    fn xoauth2_authenticator_encodes_the_sasl_initial_response() {
        let mut auth = XOAuth2Authenticator {
            user: "chris@example.com".to_string(),
            token: "ya29.fake-token".to_string(),
        };
        let response = async_imap::Authenticator::process(&mut auth, b"");
        assert_eq!(
            response,
            "user=chris@example.com\x01auth=Bearer ya29.fake-token\x01\x01"
        );
    }

    #[test]
    fn validate_message_id_accepts_a_well_formed_rfc5322_id() {
        assert!(
            ImapProvider::validate_message_id_for_imap_search("<abc123.def@mail.example.com>")
                .is_ok()
        );
    }

    #[test]
    fn validate_message_id_rejects_an_embedded_quote() {
        // `"` would let a crafted Message-ID break out of the quoted SEARCH
        // term and inject additional IMAP search criteria.
        let crafted = "<x\" OR 1=1 FOO BAR@evil.example>";
        assert!(ImapProvider::validate_message_id_for_imap_search(crafted).is_err());
    }

    #[test]
    fn validate_message_id_rejects_embedded_crlf() {
        let crafted = "<x\r\nDELETE INBOX@evil.example>";
        assert!(ImapProvider::validate_message_id_for_imap_search(crafted).is_err());
    }

    #[test]
    fn validate_message_id_rejects_a_backslash() {
        let crafted = "<x\\y@evil.example>";
        assert!(ImapProvider::validate_message_id_for_imap_search(crafted).is_err());
    }

    #[test]
    fn validate_message_id_rejects_missing_angle_brackets_or_at_sign() {
        assert!(ImapProvider::validate_message_id_for_imap_search("abc@example.com>").is_err());
        assert!(ImapProvider::validate_message_id_for_imap_search("<no-at-sign>").is_err());
        assert!(
            ImapProvider::validate_message_id_for_imap_search("<two@at@signs.example>").is_err()
        );
    }

    #[test]
    fn imap_auth_is_empty_checks_the_active_variant() {
        assert!(ImapAuth::Password(String::new()).is_empty());
        assert!(!ImapAuth::Password("x".to_string()).is_empty());
        assert!(ImapAuth::OAuthBearer(String::new()).is_empty());
        assert!(!ImapAuth::OAuthBearer("token".to_string()).is_empty());
    }

    #[test]
    fn encryption_from_request_str_maps_ui_vocabulary() {
        assert_eq!(ImapEncryption::from_request_str("ssl"), ImapEncryption::Ssl);
        assert_eq!(
            ImapEncryption::from_request_str("tls"),
            ImapEncryption::StartTls
        );
        assert_eq!(
            ImapEncryption::from_request_str("none"),
            ImapEncryption::Plaintext
        );
        // Unknown values fall back to the safest mode.
        assert_eq!(
            ImapEncryption::from_request_str("garbage"),
            ImapEncryption::Ssl
        );
    }

    #[test]
    fn encryption_stored_str_roundtrips() {
        for mode in [
            ImapEncryption::Ssl,
            ImapEncryption::StartTls,
            ImapEncryption::Plaintext,
        ] {
            assert_eq!(ImapEncryption::from_stored_str(mode.as_str()), mode);
        }
        // Legacy/request spellings are tolerated on read.
        assert_eq!(
            ImapEncryption::from_stored_str("tls"),
            ImapEncryption::StartTls
        );
        assert_eq!(
            ImapEncryption::from_stored_str("none"),
            ImapEncryption::Plaintext
        );
    }

    fn test_config() -> ImapConfig {
        ImapConfig {
            host: "imap.example.com".to_string(),
            port: 993,
            encryption: ImapEncryption::Ssl,
            username: "user@example.com".to_string(),
            auth: ImapAuth::Password("secret".to_string()),
            profile: ProviderProfile::Generic,
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
            pinned_addr: None,
        }
    }

    #[test]
    fn test_imap_config_defaults() {
        let config = ImapConfig {
            host: "imap.test.com".to_string(),
            port: 993,
            encryption: ImapEncryption::Ssl,
            username: "test@test.com".to_string(),
            auth: ImapAuth::Password("pass".to_string()),
            profile: ProviderProfile::Generic,
            mailbox: default_mailbox(),
            archive_folder: default_archive_folder(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
            pinned_addr: None,
        };
        assert_eq!(config.mailbox, "INBOX");
        assert_eq!(config.archive_folder, "Archive");
    }

    #[test]
    fn test_validate_config_empty_host() {
        let config = ImapConfig {
            host: "".to_string(),
            port: 993,
            encryption: ImapEncryption::Ssl,
            username: "user@test.com".to_string(),
            auth: ImapAuth::Password("pass".to_string()),
            profile: ProviderProfile::Generic,
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
            pinned_addr: None,
        };
        let provider = ImapProvider::new(config);
        let result = provider.validate_config();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("host"));
    }

    #[test]
    fn test_validate_config_empty_username() {
        let mut config = test_config();
        config.username = "".to_string();
        let provider = ImapProvider::new(config);
        assert!(provider.validate_config().is_err());
    }

    #[test]
    fn test_validate_config_empty_password() {
        let mut config = test_config();
        config.auth = ImapAuth::Password(String::new());
        let provider = ImapProvider::new(config);
        assert!(provider.validate_config().is_err());
    }

    #[test]
    fn test_validate_config_ok() {
        let provider = ImapProvider::new(test_config());
        assert!(provider.validate_config().is_ok());
    }

    // All methods that require a live IMAP server validate config first.
    // Tests below verify that config validation gates each operation.

    #[tokio::test]
    async fn test_authenticate_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.authenticate("").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("host"));
    }

    #[tokio::test]
    async fn test_list_messages_fails_with_bad_config() {
        let mut config = test_config();
        config.username = "".to_string();
        let provider = ImapProvider::new(config);
        let params = ListParams {
            max_results: 10,
            page_token: None,
            label: None,
            query: None,
        };
        let result = provider.list_messages("token", &params).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_message_fails_with_bad_config() {
        let mut config = test_config();
        config.auth = ImapAuth::Password(String::new());
        let provider = ImapProvider::new(config);
        let result = provider.get_message("token", "1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_archive_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.archive_message("token", "1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mark_read_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.mark_read("token", "1", true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_star_message_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.star_message("token", "1", true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_label_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.create_label("token", "TestFolder").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_labels_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.list_labels("token").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_folders_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.list_folders("token").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_move_message_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider
            .move_message("token", "1", "Trash", MoveKind::Folder)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unarchive_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.unarchive_message("token", "1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_label_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.delete_label("token", "OldFolder").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_label_message_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider
            .label_message("token", "1", &["important".to_string()])
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_labels_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider
            .remove_labels("token", "1", &["important".to_string()])
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_refresh_token_fails_with_bad_config() {
        let mut config = test_config();
        config.host = "".to_string();
        let provider = ImapProvider::new(config);
        let result = provider.refresh_token("unused").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_header_text_returns_none_for_missing() {
        let parsed = mail_parser::MessageParser::default()
            .parse(b"From: test@example.com\r\nSubject: Test\r\n\r\nBody")
            .unwrap();
        assert!(extract_header_text(&parsed, "List-Unsubscribe").is_none());
    }
}
