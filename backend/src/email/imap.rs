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

/// Configuration for connecting to an IMAP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    /// IMAP server hostname (e.g., "imap.gmail.com").
    pub host: String,
    /// IMAP server port (993 for TLS, 143 for STARTTLS).
    pub port: u16,
    /// Whether to use implicit TLS (port 993) vs STARTTLS.
    pub use_tls: bool,
    /// Username (typically the email address).
    pub username: String,
    /// Password or app-specific password.
    pub password: String,
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
// Session type alias
// ---------------------------------------------------------------------------

type ImapSession = async_imap::Session<async_native_tls::TlsStream<tokio::net::TcpStream>>;

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
        if self.config.password.is_empty() {
            return Err(ProviderError::ConfigError(
                "IMAP password is required".to_string(),
            ));
        }
        Ok(())
    }

    /// Establish a TLS connection, read the greeting, and log in.
    async fn connect(&self) -> Result<ImapSession, ProviderError> {
        let tcp = tokio::net::TcpStream::connect((&*self.config.host, self.config.port))
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "TCP connect to {}:{} failed: {e}",
                    self.config.host, self.config.port
                ))
            })?;

        let tls = async_native_tls::TlsConnector::new();
        let tls_stream = tls.connect(&self.config.host, tcp).await.map_err(|e| {
            ProviderError::RequestFailed(format!(
                "TLS handshake with {} failed: {e}",
                self.config.host
            ))
        })?;

        let mut client = async_imap::Client::new(tls_stream);

        // Read the server greeting (required before login/authenticate).
        let _greeting = client
            .read_response()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to read greeting: {e}")))?;

        let session = client
            .login(&self.config.username, &self.config.password)
            .await
            .map_err(|(e, _client)| ProviderError::OAuthError(format!("IMAP login failed: {e}")))?;

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

        Some(EmailMessage {
            id: uid.to_string(),
            thread_id: parsed.message_id().map(|s| s.to_string()),
            from,
            to,
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

        let creds = Credentials::new(self.config.username.clone(), self.config.password.clone());

        let mailer: AsyncSmtpTransport<Tokio1Executor> =
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host)
                .map_err(|e| ProviderError::RequestFailed(format!("SMTP connection failed: {e}")))?
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
    use mail_parser::HeaderName;

    msg.header_values(HeaderName::Other(name.into()))
        .find_map(|hv| match hv {
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

        let fetches = session
            .uid_fetch(&uid_set, "(FLAGS RFC822)")
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

        let fetches = session
            .uid_fetch(id, "(FLAGS RFC822)")
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

    /// Archive a message by copying it to the archive folder and expunging.
    async fn archive_message(&self, _access_token: &str, id: &str) -> Result<(), ProviderError> {
        self.validate_config()?;

        let (mut session, _) = self.connect_and_select().await?;

        // Copy to archive folder.
        session
            .uid_copy(id, &self.config.archive_folder)
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "UID COPY to '{}' failed: {e}",
                    self.config.archive_folder
                ))
            })?;

        // Mark as deleted in current mailbox.
        session
            .uid_store(id, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE \\Deleted failed: {e}")))?
            .try_collect::<Vec<async_imap::types::Fetch>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE stream failed: {e}")))?;

        // Expunge to permanently remove from current mailbox.
        session
            .expunge()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE failed: {e}")))?
            .try_collect::<Vec<u32>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE stream failed: {e}")))?;

        let _ = session.logout().await;
        Ok(())
    }

    /// Apply IMAP keywords (custom flags) to a message.
    async fn label_message(
        &self,
        _access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        let keywords = labels.join(" ");
        self.store_flags(id, "+FLAGS", &keywords).await
    }

    /// Remove IMAP keywords from a message.
    async fn remove_labels(
        &self,
        _access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        self.validate_config()?;
        let keywords = labels.join(" ");
        self.store_flags(id, "-FLAGS", &keywords).await
    }

    /// Create a label by creating an IMAP folder (mailbox).
    async fn create_label(&self, _access_token: &str, name: &str) -> Result<String, ProviderError> {
        self.validate_config()?;

        let mut session = self.connect().await?;

        match session.create(name).await {
            Ok(()) => {}
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("ALREADYEXISTS") && !msg.contains("already exists") {
                    let _ = session.logout().await;
                    return Err(ProviderError::RequestFailed(format!(
                        "CREATE '{name}' failed: {e}"
                    )));
                }
                debug!(folder = %name, "Folder already exists, treating as success");
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

        // SELECT the archive folder, copy back to INBOX, delete from archive.
        session
            .select(&self.config.archive_folder)
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("SELECT Archive failed: {e}")))?;

        session
            .uid_copy(id, "INBOX")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("UID COPY to INBOX failed: {e}")))?;

        session
            .uid_store(id, "+FLAGS (\\Deleted)")
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE \\Deleted failed: {e}")))?
            .try_collect::<Vec<async_imap::types::Fetch>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("STORE stream failed: {e}")))?;

        session
            .expunge()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE failed: {e}")))?
            .try_collect::<Vec<u32>>()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE stream failed: {e}")))?;

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

                session.uid_copy(message_id, target_id).await.map_err(|e| {
                    ProviderError::RequestFailed(format!("UID COPY to '{target_id}' failed: {e}"))
                })?;

                session
                    .uid_store(message_id, "+FLAGS (\\Deleted)")
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("STORE \\Deleted failed: {e}"))
                    })?
                    .try_collect::<Vec<async_imap::types::Fetch>>()
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("STORE stream failed: {e}"))
                    })?;

                session
                    .expunge()
                    .await
                    .map_err(|e| ProviderError::RequestFailed(format!("EXPUNGE failed: {e}")))?
                    .try_collect::<Vec<u32>>()
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("EXPUNGE stream failed: {e}"))
                    })?;

                let _ = session.logout().await;
            }
            MoveKind::Label => {
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

    fn test_config() -> ImapConfig {
        ImapConfig {
            host: "imap.example.com".to_string(),
            port: 993,
            use_tls: true,
            username: "user@example.com".to_string(),
            password: "secret".to_string(),
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
        }
    }

    #[test]
    fn test_imap_config_defaults() {
        let config = ImapConfig {
            host: "imap.test.com".to_string(),
            port: 993,
            use_tls: true,
            username: "test@test.com".to_string(),
            password: "pass".to_string(),
            mailbox: default_mailbox(),
            archive_folder: default_archive_folder(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
        };
        assert_eq!(config.mailbox, "INBOX");
        assert_eq!(config.archive_folder, "Archive");
    }

    #[test]
    fn test_validate_config_empty_host() {
        let config = ImapConfig {
            host: "".to_string(),
            port: 993,
            use_tls: true,
            username: "user@test.com".to_string(),
            password: "pass".to_string(),
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: None,
            smtp_port: default_smtp_port(),
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
        config.password = "".to_string();
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
        config.password = "".to_string();
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
