//! Gmail provider implementation (DDD-005 ACL).
//!
//! Wraps the Gmail REST API v1 using reqwest, translating between
//! Google-specific JSON responses and the domain EmailMessage model.

use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};

use super::provider::{EmailProvider, ProviderError, SendDraft};
use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens, ProviderConfig};

// ---------------------------------------------------------------------------
// Gmail Incremental Sync Types (R-01)
// ---------------------------------------------------------------------------

/// High-level response from Gmail incremental sync via history.list API.
///
/// This is a convenience wrapper around the lower-level `GmailHistoryDelta`
/// that provides a simpler interface for consumers who only need message IDs
/// and label change tuples.
#[derive(Debug, Clone, Default)]
pub struct HistoryResponse {
    /// Message IDs that were added to the mailbox since the last sync.
    pub messages_added: Vec<String>,
    /// Message IDs that were deleted from the mailbox.
    pub messages_deleted: Vec<String>,
    /// Label additions: (message_id, label_ids) pairs.
    pub labels_added: Vec<(String, Vec<String>)>,
    /// Label removals: (message_id, label_ids) pairs.
    pub labels_removed: Vec<(String, Vec<String>)>,
    /// The new history ID to use as `startHistoryId` in the next call.
    pub new_history_id: String,
}

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// Gmail provider using the Gmail REST API v1.
pub struct GmailProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

impl GmailProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Fetch the authenticated user's email address from the Gmail profile.
    pub async fn get_user_email(&self, access_token: &str) -> Result<String, ProviderError> {
        let http_resp = self
            .http
            .get(format!("{GMAIL_API_BASE}/profile"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        let status = http_resp.status();
        let body: serde_json::Value = http_resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        if !status.is_success() {
            let api_error = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(ProviderError::RequestFailed(format!(
                "Gmail profile API returned {status}: {api_error}"
            )));
        }

        body["emailAddress"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                tracing::warn!("Gmail profile response missing emailAddress: {body}");
                ProviderError::ParseError("Missing emailAddress in profile".into())
            })
    }

    /// Parse a Gmail API message JSON into an EmailMessage.
    ///
    /// `label_map` resolves Gmail label IDs (e.g. `Label_356207...`) to
    /// human-readable names (e.g. "Receipts"). Pass an empty map to skip
    /// resolution — system labels like INBOX/UNREAD are kept as-is.
    fn parse_message(
        msg: &serde_json::Value,
        label_map: &std::collections::HashMap<String, String>,
    ) -> Result<EmailMessage, ProviderError> {
        let id = msg["id"].as_str().unwrap_or_default().to_string();
        let thread_id = msg["threadId"].as_str().map(|s| s.to_string());
        let snippet = msg["snippet"].as_str().unwrap_or_default().to_string();

        let raw_label_ids: Vec<String> = msg["labelIds"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Resolve label IDs to human-readable names where possible.
        let labels: Vec<String> = raw_label_ids
            .iter()
            .map(|lid| label_map.get(lid).cloned().unwrap_or_else(|| lid.clone()))
            .collect();

        let is_read = !labels.contains(&"UNREAD".to_string());

        // Extract headers.
        let headers = msg["payload"]["headers"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let get_header = |name: &str| -> String {
            headers
                .iter()
                .find(|h| {
                    h["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
                .and_then(|h| h["value"].as_str())
                .unwrap_or_default()
                .to_string()
        };

        let from = get_header("From");
        let to_raw = get_header("To");
        let to: Vec<String> = to_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let subject = get_header("Subject");
        let date_str = get_header("Date");

        // Parse the internal date (milliseconds since epoch).
        let date = msg["internalDate"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
            .or_else(|| {
                DateTime::parse_from_rfc2822(&date_str)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
            .unwrap_or_else(Utc::now);

        // Extract List-Unsubscribe headers (RFC 2369 / RFC 8058).
        let unsub = super::types::UnsubscribeHeaders::from_json_headers(&headers);

        // Extract plain-text body from parts.
        let body = extract_body_text(msg);
        // Extract and sanitize HTML body from parts.
        let body_html = extract_body_html(msg)
            .map(|html| crate::content::email_sanitizer::sanitize_email_html(&html));

        Ok(EmailMessage {
            id,
            thread_id,
            from,
            to,
            subject,
            snippet,
            body,
            body_html,
            labels,
            date,
            is_read,
            list_unsubscribe: unsub.list_unsubscribe,
            list_unsubscribe_post: unsub.list_unsubscribe_post,
        })
    }
}

/// Recursively extract plain-text body from Gmail message parts.
fn extract_body_text(msg: &serde_json::Value) -> Option<String> {
    // Check direct body data.
    if let Some(data) = msg["payload"]["body"]["data"].as_str() {
        return decode_base64url(data);
    }

    // Check parts recursively.
    if let Some(parts) = msg["payload"]["parts"].as_array() {
        for part in parts {
            let mime = part["mimeType"].as_str().unwrap_or_default();
            if mime == "text/plain" {
                if let Some(data) = part["body"]["data"].as_str() {
                    return decode_base64url(data);
                }
            }
            // Recurse into multipart sub-parts.
            if let Some(sub_parts) = part["parts"].as_array() {
                for sub in sub_parts {
                    if sub["mimeType"].as_str() == Some("text/plain") {
                        if let Some(data) = sub["body"]["data"].as_str() {
                            return decode_base64url(data);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Recursively extract HTML body from Gmail message parts.
///
/// Searches `payload.parts[]` for `text/html` MIME parts, including nested
/// `multipart/alternative` sub-parts. Falls back to `payload.body.data`
/// when the top-level `mimeType` is `text/html` (simple non-multipart messages).
fn extract_body_html(msg: &serde_json::Value) -> Option<String> {
    // Check if the top-level payload itself is text/html (non-multipart message).
    if msg["payload"]["mimeType"].as_str() == Some("text/html") {
        if let Some(data) = msg["payload"]["body"]["data"].as_str() {
            return decode_base64url(data);
        }
    }

    // Check parts recursively.
    if let Some(parts) = msg["payload"]["parts"].as_array() {
        if let Some(html) = find_html_in_parts(parts) {
            return Some(html);
        }
    }

    None
}

/// Recursively search a list of MIME parts for `text/html` content.
fn find_html_in_parts(parts: &[serde_json::Value]) -> Option<String> {
    for part in parts {
        let mime = part["mimeType"].as_str().unwrap_or_default();
        if mime == "text/html" {
            if let Some(data) = part["body"]["data"].as_str() {
                return decode_base64url(data);
            }
        }
        // Recurse into multipart sub-parts (e.g., multipart/alternative).
        if let Some(sub_parts) = part["parts"].as_array() {
            if let Some(html) = find_html_in_parts(sub_parts) {
                return Some(html);
            }
        }
    }
    None
}

/// Decode Gmail's URL-safe base64 encoded content.
///
/// Gmail uses URL-safe base64 without padding. This handles:
/// - Whitespace/newlines that some encoders insert (stripped before decode)
/// - Padding characters that some responses include
/// - Non-UTF-8 charsets (ISO-8859-1, Windows-1252) via lossy conversion
fn decode_base64url(data: &str) -> Option<String> {
    use base64::Engine;

    // Strip whitespace — some base64 data contains newlines or spaces.
    let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();

    // Try URL-safe without padding first (Gmail's documented format).
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&cleaned)
        // Fall back to URL-safe with padding.
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&cleaned))
        // Fall back to standard base64.
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&cleaned));

    match bytes {
        Ok(bytes) => {
            // Try strict UTF-8 first, then lossy for non-UTF-8 charsets
            // (ISO-8859-1, Windows-1252 are common in marketing emails).
            Some(
                String::from_utf8(bytes.clone())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned()),
            )
        }
        Err(e) => {
            tracing::warn!(
                data_len = cleaned.len(),
                error = %e,
                "Failed to base64-decode body data"
            );
            None
        }
    }
}

#[async_trait]
impl EmailProvider for GmailProvider {
    async fn authenticate(&self, auth_code: &str) -> Result<OAuthTokens, ProviderError> {
        let resp = self
            .http
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", auth_code),
                ("redirect_uri", &self.config.redirect_uri),
                ("client_id", &self.config.client_id),
                ("client_secret", &self.config.client_secret),
            ])
            .send()
            .await
            .map_err(|e| ProviderError::OAuthError(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::OAuthError(format!(
                "Token exchange failed: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| ProviderError::OAuthError("Missing access_token".into()))?
            .to_string();
        let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        Ok(OAuthTokens {
            access_token,
            refresh_token,
            expires_at: Some(Utc::now() + chrono::Duration::seconds(expires_in)),
            email: None,
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, ProviderError> {
        let resp = self
            .http
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.config.client_id),
                ("client_secret", &self.config.client_secret),
            ])
            .send()
            .await
            .map_err(|e| ProviderError::TokenExpired(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::TokenExpired(body));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        Ok(OAuthTokens {
            access_token: body["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            refresh_token: body["refresh_token"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| Some(refresh_token.to_string())),
            expires_at: Some(
                Utc::now() + chrono::Duration::seconds(body["expires_in"].as_i64().unwrap_or(3600)),
            ),
            email: None,
        })
    }

    async fn list_messages(
        &self,
        access_token: &str,
        params: &ListParams,
    ) -> Result<EmailPage, ProviderError> {
        let mut url = format!(
            "{GMAIL_API_BASE}/messages?maxResults={}",
            params.max_results
        );

        if let Some(ref pt) = params.page_token {
            url.push_str(&format!("&pageToken={pt}"));
        }
        if let Some(ref q) = params.query {
            url.push_str(&format!("&q={}", urlencoding::encode(q)));
        }
        if let Some(ref label) = params.label {
            url.push_str(&format!("&labelIds={}", urlencoding::encode(label)));
        }

        let resp: serde_json::Value = self
            .http
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;

        let message_refs = resp["messages"].as_array().cloned().unwrap_or_default();

        // Fetch full message details concurrently.
        // Gmail allows 250 quota units/sec; messages.get costs 5 units = 50 req/sec max.
        // 5 concurrent requests at ~100ms each ≈ 50 req/sec — safe within quota.
        let msg_ids: Vec<String> = message_refs
            .iter()
            .filter_map(|r| r["id"].as_str().map(|s| s.to_string()))
            .collect();

        use futures::StreamExt;

        // Fetch label ID→name mapping so custom labels display human-readable names.
        let label_map: std::sync::Arc<std::collections::HashMap<String, String>> =
            match self.list_labels(access_token).await {
                Ok(pairs) => std::sync::Arc::new(pairs.into_iter().collect()),
                Err(e) => {
                    tracing::warn!("Failed to fetch Gmail label names: {e}. Labels will show IDs.");
                    std::sync::Arc::new(std::collections::HashMap::new())
                }
            };

        let token = access_token.to_string();
        let http = self.http.clone();
        let results: Vec<Result<EmailMessage, ProviderError>> =
            futures::stream::iter(msg_ids.into_iter())
                .map(|msg_id| {
                    let t = token.clone();
                    let h = http.clone();
                    let lm = label_map.clone();
                    async move {
                        let full: serde_json::Value = h
                            .get(format!("{GMAIL_API_BASE}/messages/{msg_id}?format=full"))
                            .bearer_auth(&t)
                            .send()
                            .await
                            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
                            .json()
                            .await
                            .map_err(|e| ProviderError::ParseError(e.to_string()))?;
                        check_error_response(&full)?;
                        Self::parse_message(&full, &lm)
                    }
                })
                .buffer_unordered(5)
                .collect()
                .await;

        let mut messages = Vec::with_capacity(results.len());
        for result in results {
            messages.push(result?);
        }

        Ok(EmailPage {
            messages,
            next_page_token: resp["nextPageToken"].as_str().map(|s| s.to_string()),
            result_size_estimate: resp["resultSizeEstimate"].as_u64().map(|n| n as u32),
        })
    }

    async fn get_message(
        &self,
        access_token: &str,
        id: &str,
    ) -> Result<EmailMessage, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(format!("{GMAIL_API_BASE}/messages/{id}?format=full"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Self::parse_message(&resp, &std::collections::HashMap::new())
    }

    async fn archive_message(&self, access_token: &str, id: &str) -> Result<(), ProviderError> {
        // Gmail archive = remove INBOX label.
        let body = serde_json::json!({
            "removeLabelIds": ["INBOX"]
        });

        let resp = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed(format!(
                "Archive failed: {body}"
            )));
        }

        Ok(())
    }

    async fn label_message(
        &self,
        access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        let body = serde_json::json!({
            "addLabelIds": labels
        });

        let resp = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed(format!(
                "Label failed: {body}"
            )));
        }

        Ok(())
    }

    async fn remove_labels(
        &self,
        access_token: &str,
        id: &str,
        labels: &[String],
    ) -> Result<(), ProviderError> {
        let body = serde_json::json!({
            "removeLabelIds": labels
        });

        let resp = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed(format!(
                "Remove labels failed: {body}"
            )));
        }

        Ok(())
    }

    async fn create_label(&self, access_token: &str, name: &str) -> Result<String, ProviderError> {
        let body = serde_json::json!({
            "name": name,
            "labelListVisibility": "labelShow",
            "messageListVisibility": "show"
        });

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/labels"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        // If label already exists, Gmail returns 409. The error message
        // contains the existing label info. For idempotency, we treat this
        // as success and look up the existing label.
        if let Some(error) = resp["error"].as_object() {
            let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            if code == 409 {
                // Label already exists; fetch it by name.
                return self.find_label_id(access_token, name).await;
            }
            return Err(ProviderError::RequestFailed(format!(
                "Create label failed: {}",
                serde_json::to_string(error).unwrap_or_default()
            )));
        }

        resp["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ProviderError::ParseError("Missing label id".into()))
    }

    async fn list_labels(
        &self,
        access_token: &str,
    ) -> Result<Vec<(String, String)>, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(format!("{GMAIL_API_BASE}/labels"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;

        let labels = resp["labels"]
            .as_array()
            .ok_or_else(|| ProviderError::ParseError("Missing labels array".into()))?;

        Ok(labels
            .iter()
            .filter_map(|l| {
                let id = l["id"].as_str()?.to_string();
                let name = l["name"].as_str()?.to_string();
                Some((id, name))
            })
            .collect())
    }

    async fn delete_label(&self, access_token: &str, label_id: &str) -> Result<(), ProviderError> {
        let resp = self
            .http
            .delete(format!("{GMAIL_API_BASE}/labels/{label_id}"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(ProviderError::RequestFailed(format!(
                "Delete label failed: {msg}"
            )));
        }
        Ok(())
    }

    async fn unarchive_message(&self, access_token: &str, id: &str) -> Result<(), ProviderError> {
        let body = serde_json::json!({
            "addLabelIds": ["INBOX"]
        });

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(())
    }

    async fn list_folders(
        &self,
        access_token: &str,
    ) -> Result<Vec<super::provider::FolderOrLabel>, ProviderError> {
        use super::provider::{FolderOrLabel, MoveKind};

        let labels = self.list_labels(access_token).await?;

        // System labels that are valid move targets.
        let movable_system = ["INBOX", "SENT", "TRASH", "SPAM", "DRAFT", "IMPORTANT"];
        // System labels to hide (status flags, internal labels, superstars).
        let hidden = [
            "STARRED",
            "UNREAD",
            "CHAT",
            "YELLOW_STAR",
            "ORANGE_STAR",
            "RED_STAR",
            "PURPLE_STAR",
            "BLUE_STAR",
            "GREEN_STAR",
            "RED_BANG",
            "ORANGE_GUILLEMET",
            "YELLOW_BANG",
            "GREEN_CHECK",
            "BLUE_INFO",
            "PURPLE_QUESTION",
        ];

        let friendly = |id: &str, name: &str| -> String {
            match id {
                "INBOX" => "Inbox".into(),
                "SENT" => "Sent".into(),
                "TRASH" => "Trash".into(),
                "SPAM" => "Spam".into(),
                "DRAFT" => "Drafts".into(),
                "IMPORTANT" => "Important".into(),
                _ => name.to_string(),
            }
        };

        Ok(labels
            .into_iter()
            .filter(|(id, _)| !hidden.contains(&id.as_str()) && !id.starts_with("CATEGORY_"))
            .map(|(id, name)| {
                let is_system = movable_system.contains(&id.as_str());
                let display = friendly(&id, &name);
                FolderOrLabel {
                    id,
                    name: display,
                    kind: if is_system {
                        MoveKind::Folder
                    } else {
                        MoveKind::Label
                    },
                    is_system,
                }
            })
            .collect())
    }

    async fn move_message(
        &self,
        access_token: &str,
        message_id: &str,
        target_id: &str,
        kind: super::provider::MoveKind,
    ) -> Result<(), ProviderError> {
        use super::provider::MoveKind;

        let body = match kind {
            MoveKind::Folder => {
                // Move to folder: add target label, remove INBOX.
                serde_json::json!({
                    "addLabelIds": [target_id],
                    "removeLabelIds": ["INBOX"]
                })
            }
            MoveKind::Label => {
                // Add label only (additive).
                serde_json::json!({
                    "addLabelIds": [target_id]
                })
            }
        };

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{message_id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(())
    }

    async fn mark_read(
        &self,
        access_token: &str,
        message_id: &str,
        read: bool,
    ) -> Result<(), ProviderError> {
        // Gmail: UNREAD is a label. Read = remove UNREAD; Unread = add UNREAD.
        let body = if read {
            serde_json::json!({ "removeLabelIds": ["UNREAD"] })
        } else {
            serde_json::json!({ "addLabelIds": ["UNREAD"] })
        };

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{message_id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(())
    }

    async fn star_message(
        &self,
        access_token: &str,
        message_id: &str,
        starred: bool,
    ) -> Result<(), ProviderError> {
        let body = if starred {
            serde_json::json!({ "addLabelIds": ["STARRED"] })
        } else {
            serde_json::json!({ "removeLabelIds": ["STARRED"] })
        };

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/{message_id}/modify"))
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(())
    }

    async fn send_message(
        &self,
        access_token: &str,
        draft: &SendDraft<'_>,
    ) -> Result<String, ProviderError> {
        let raw = build_rfc2822(
            None,
            draft.to,
            draft.cc,
            draft.bcc,
            draft.subject,
            None,
            draft.body_text,
            draft.body_html,
        );
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let payload = serde_json::json!({ "raw": encoded });

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/send"))
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(resp["id"].as_str().unwrap_or("").to_string())
    }

    async fn reply_to_message(
        &self,
        access_token: &str,
        message_id: &str,
        body_text: Option<&str>,
        body_html: Option<&str>,
    ) -> Result<String, ProviderError> {
        // Fetch the original message to get thread_id and headers for the reply.
        let original = self.get_message(access_token, message_id).await?;
        let to = &original.from;
        let subject = if original.subject.starts_with("Re: ") {
            original.subject.clone()
        } else {
            format!("Re: {}", original.subject)
        };

        let raw = build_rfc2822(
            Some(message_id),
            to,
            None,
            None,
            &subject,
            original.thread_id.as_deref(),
            body_text,
            body_html,
        );
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let mut payload = serde_json::json!({ "raw": encoded });
        if let Some(tid) = &original.thread_id {
            payload["threadId"] = serde_json::Value::String(tid.clone());
        }

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/send"))
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(resp["id"].as_str().unwrap_or("").to_string())
    }

    async fn forward_message(
        &self,
        access_token: &str,
        message_id: &str,
        to: &str,
    ) -> Result<String, ProviderError> {
        let original = self.get_message(access_token, message_id).await?;
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

        let raw = build_rfc2822(None, to, None, None, &subject, None, Some(&fwd_body), None);
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());

        let payload = serde_json::json!({ "raw": encoded });

        let resp: serde_json::Value = self
            .http
            .post(format!("{GMAIL_API_BASE}/messages/send"))
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(resp["id"].as_str().unwrap_or("").to_string())
    }
}

impl GmailProvider {
    // -----------------------------------------------------------------------
    // Gmail History API (incremental / delta sync)
    // -----------------------------------------------------------------------

    /// Fetch the current history ID from the user's Gmail profile.
    ///
    /// This is used as the starting point for subsequent `history_list` calls.
    pub async fn get_history_id(&self, access_token: &str) -> Result<String, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(format!("{GMAIL_API_BASE}/profile"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;

        resp["historyId"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ProviderError::ParseError("Missing historyId in profile".into()))
    }

    /// Call the Gmail `history.list` API to get changes since `start_history_id`.
    ///
    /// Returns the raw JSON response which can be parsed with
    /// `delta::parse_gmail_history`.
    pub async fn history_list(
        &self,
        access_token: &str,
        start_history_id: &str,
        page_token: Option<&str>,
    ) -> Result<serde_json::Value, ProviderError> {
        let mut url = format!("{GMAIL_API_BASE}/history?startHistoryId={start_history_id}");

        if let Some(pt) = page_token {
            url.push_str(&format!("&pageToken={pt}"));
        }

        let resp: serde_json::Value = self
            .http
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_error_response(&resp)?;
        Ok(resp)
    }

    /// Fetch all history pages and return an aggregated delta.
    pub async fn history_list_all(
        &self,
        access_token: &str,
        start_history_id: &str,
    ) -> Result<super::delta::GmailHistoryDelta, ProviderError> {
        let mut all_added = Vec::new();
        let mut all_deleted = Vec::new();
        let mut all_label_changes = Vec::new();
        let mut latest_history_id = None;
        let mut page_token: Option<String> = None;

        loop {
            let resp = self
                .history_list(access_token, start_history_id, page_token.as_deref())
                .await?;

            let delta =
                super::delta::parse_gmail_history(&resp).map_err(ProviderError::ParseError)?;

            all_added.extend(delta.added_message_ids);
            all_deleted.extend(delta.deleted_message_ids);
            all_label_changes.extend(delta.label_changes);

            if delta.new_history_id.is_some() {
                latest_history_id = delta.new_history_id;
            }

            // Check for next page.
            match resp["nextPageToken"].as_str() {
                Some(token) => page_token = Some(token.to_string()),
                None => break,
            }
        }

        Ok(super::delta::GmailHistoryDelta {
            added_message_ids: all_added,
            deleted_message_ids: all_deleted,
            label_changes: all_label_changes,
            new_history_id: latest_history_id,
        })
    }

    // -----------------------------------------------------------------------
    // Convenience Wrappers (R-01)
    // -----------------------------------------------------------------------

    /// Get the current history ID from the user's Gmail profile.
    ///
    /// Alias for `get_history_id` — named to match the R-01 specification.
    pub async fn get_current_history_id(
        &self,
        access_token: &str,
    ) -> Result<String, ProviderError> {
        self.get_history_id(access_token).await
    }

    /// Fetch changes since the given history ID and return a typed
    /// `HistoryResponse`.
    ///
    /// This aggregates all pages from `history.list` and converts the
    /// low-level `GmailHistoryDelta` into the simpler `HistoryResponse`
    /// format with tuple-based label changes.
    pub async fn history_list_typed(
        &self,
        access_token: &str,
        start_history_id: &str,
    ) -> Result<HistoryResponse, ProviderError> {
        let delta = self
            .history_list_all(access_token, start_history_id)
            .await?;

        let labels_added: Vec<(String, Vec<String>)> = delta
            .label_changes
            .iter()
            .filter(|lc| !lc.added_labels.is_empty())
            .map(|lc| (lc.message_id.clone(), lc.added_labels.clone()))
            .collect();

        let labels_removed: Vec<(String, Vec<String>)> = delta
            .label_changes
            .iter()
            .filter(|lc| !lc.removed_labels.is_empty())
            .map(|lc| (lc.message_id.clone(), lc.removed_labels.clone()))
            .collect();

        Ok(HistoryResponse {
            messages_added: delta.added_message_ids,
            messages_deleted: delta.deleted_message_ids,
            labels_added,
            labels_removed,
            new_history_id: delta.new_history_id.unwrap_or_default(),
        })
    }

    // -----------------------------------------------------------------------
    // Batch Message Fetching
    // -----------------------------------------------------------------------

    /// Fetch multiple messages concurrently with bounded concurrency.
    ///
    /// Limits to `max_concurrent` simultaneous requests (default 10) to
    /// avoid Gmail API rate limits.
    pub async fn batch_get_messages(
        &self,
        access_token: &str,
        message_ids: &[String],
        max_concurrent: usize,
    ) -> Result<Vec<super::types::EmailMessage>, ProviderError> {
        use futures::stream::{self, StreamExt};

        let concurrency = if max_concurrent == 0 {
            10
        } else {
            max_concurrent
        };

        let results: Vec<Result<super::types::EmailMessage, ProviderError>> =
            stream::iter(message_ids.iter())
                .map(|msg_id| {
                    let token = access_token.to_string();
                    let id = msg_id.clone();
                    let http = self.http.clone();
                    async move {
                        let resp: serde_json::Value = http
                            .get(format!("{GMAIL_API_BASE}/messages/{id}?format=full"))
                            .bearer_auth(&token)
                            .send()
                            .await
                            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
                            .json()
                            .await
                            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

                        check_error_response(&resp)?;
                        Self::parse_message(&resp, &std::collections::HashMap::new())
                    }
                })
                .buffer_unordered(concurrency)
                .collect()
                .await;

        // Collect successes, propagate first error.
        let mut messages = Vec::with_capacity(results.len());
        for result in results {
            messages.push(result?);
        }
        Ok(messages)
    }

    /// Find a label's ID by its name (for idempotent create).
    async fn find_label_id(&self, access_token: &str, name: &str) -> Result<String, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(format!("{GMAIL_API_BASE}/labels"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let labels = resp["labels"]
            .as_array()
            .ok_or_else(|| ProviderError::ParseError("Missing labels array".into()))?;

        for label in labels {
            if label["name"].as_str() == Some(name) {
                if let Some(id) = label["id"].as_str() {
                    return Ok(id.to_string());
                }
            }
        }

        Err(ProviderError::NotFound(format!("Label '{name}' not found")))
    }
}

/// Check if a Gmail API response contains an error object and convert it.
fn check_error_response(resp: &serde_json::Value) -> Result<(), ProviderError> {
    if let Some(error) = resp["error"].as_object() {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");

        if code == 429 {
            return Err(ProviderError::RateLimited {
                retry_after_secs: 60,
            });
        }
        // Gmail returns 403 with "Quota exceeded" or "Rate Limit Exceeded"
        // for per-user quota violations (~250 queries/minute).
        if code == 403 {
            let msg_lower = message.to_lowercase();
            if msg_lower.contains("quota") || msg_lower.contains("rate limit") {
                return Err(ProviderError::RateLimited {
                    retry_after_secs: 60,
                });
            }
        }
        if code == 401 {
            return Err(ProviderError::TokenExpired(message.to_string()));
        }
        if code == 404 {
            return Err(ProviderError::NotFound(message.to_string()));
        }

        return Err(ProviderError::RequestFailed(format!(
            "Gmail API error ({code}): {message}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ProviderConfig {
        ProviderConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            redirect_uri: "http://localhost:3000/callback".to_string(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            scopes: vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()],
        }
    }

    #[test]
    fn test_history_response_default() {
        let resp = HistoryResponse::default();
        assert!(resp.messages_added.is_empty());
        assert!(resp.messages_deleted.is_empty());
        assert!(resp.labels_added.is_empty());
        assert!(resp.labels_removed.is_empty());
        assert!(resp.new_history_id.is_empty());
    }

    #[test]
    fn test_history_response_fields() {
        let resp = HistoryResponse {
            messages_added: vec!["msg-1".to_string(), "msg-2".to_string()],
            messages_deleted: vec!["msg-3".to_string()],
            labels_added: vec![("msg-1".to_string(), vec!["STARRED".to_string()])],
            labels_removed: vec![("msg-1".to_string(), vec!["UNREAD".to_string()])],
            new_history_id: "12345".to_string(),
        };
        assert_eq!(resp.messages_added.len(), 2);
        assert_eq!(resp.messages_deleted.len(), 1);
        assert_eq!(resp.labels_added[0].0, "msg-1");
        assert_eq!(resp.labels_added[0].1, vec!["STARRED"]);
        assert_eq!(resp.labels_removed[0].1, vec!["UNREAD"]);
        assert_eq!(resp.new_history_id, "12345");
    }

    #[test]
    fn test_check_error_response_ok() {
        let resp = serde_json::json!({"messages": []});
        assert!(check_error_response(&resp).is_ok());
    }

    #[test]
    fn test_check_error_response_rate_limited() {
        let resp = serde_json::json!({
            "error": { "code": 429, "message": "Rate limit exceeded" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(
            err,
            ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));
    }

    #[test]
    fn test_check_error_response_403_quota_exceeded() {
        let resp = serde_json::json!({
            "error": { "code": 403, "message": "Quota exceeded for quota metric" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(
            err,
            ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));
    }

    #[test]
    fn test_check_error_response_403_rate_limit_exceeded() {
        let resp = serde_json::json!({
            "error": { "code": 403, "message": "Rate Limit Exceeded" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(
            err,
            ProviderError::RateLimited {
                retry_after_secs: 60
            }
        ));
    }

    #[test]
    fn test_check_error_response_403_non_quota() {
        // A 403 that is not quota-related should remain a generic RequestFailed.
        let resp = serde_json::json!({
            "error": { "code": 403, "message": "Insufficient Permission" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::RequestFailed(_)));
    }

    #[test]
    fn test_check_error_response_token_expired() {
        let resp = serde_json::json!({
            "error": { "code": 401, "message": "Invalid credentials" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::TokenExpired(_)));
    }

    #[test]
    fn test_check_error_response_not_found() {
        let resp = serde_json::json!({
            "error": { "code": 404, "message": "Not found" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)));
    }

    #[test]
    fn test_check_error_response_generic() {
        let resp = serde_json::json!({
            "error": { "code": 500, "message": "Internal error" }
        });
        let err = check_error_response(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::RequestFailed(_)));
        assert!(err.to_string().contains("500"));
    }

    #[test]
    fn test_parse_message_minimal() {
        let msg = serde_json::json!({
            "id": "abc123",
            "threadId": "thread-1",
            "snippet": "Hello world",
            "labelIds": ["INBOX", "UNREAD"],
            "internalDate": "1700000000000",
            "payload": {
                "headers": [
                    { "name": "From", "value": "sender@test.com" },
                    { "name": "To", "value": "me@test.com" },
                    { "name": "Subject", "value": "Test Subject" }
                ]
            }
        });
        let email = GmailProvider::parse_message(&msg, &std::collections::HashMap::new()).unwrap();
        assert_eq!(email.id, "abc123");
        assert_eq!(email.thread_id, Some("thread-1".to_string()));
        assert_eq!(email.from, "sender@test.com");
        assert_eq!(email.to, vec!["me@test.com"]);
        assert_eq!(email.subject, "Test Subject");
        assert_eq!(email.snippet, "Hello world");
        assert!(!email.is_read); // UNREAD label present
        assert_eq!(email.labels, vec!["INBOX", "UNREAD"]);
    }

    #[test]
    fn test_parse_message_read() {
        let msg = serde_json::json!({
            "id": "read-msg",
            "labelIds": ["INBOX"],
            "internalDate": "1700000000000",
            "payload": { "headers": [] }
        });
        let email = GmailProvider::parse_message(&msg, &std::collections::HashMap::new()).unwrap();
        assert!(email.is_read); // No UNREAD label
    }

    #[test]
    fn test_parse_message_multiple_recipients() {
        let msg = serde_json::json!({
            "id": "multi-to",
            "internalDate": "1700000000000",
            "payload": {
                "headers": [
                    { "name": "To", "value": "a@test.com, b@test.com, c@test.com" }
                ]
            }
        });
        let email = GmailProvider::parse_message(&msg, &std::collections::HashMap::new()).unwrap();
        assert_eq!(email.to.len(), 3);
    }

    #[test]
    fn test_decode_base64url_valid() {
        // "Hello" in URL-safe base64 no padding
        let encoded = "SGVsbG8";
        let decoded = decode_base64url(encoded);
        assert_eq!(decoded, Some("Hello".to_string()));
    }

    #[test]
    fn test_decode_base64url_invalid() {
        let decoded = decode_base64url("!!!invalid!!!");
        assert!(decoded.is_none());
    }

    #[test]
    fn test_extract_body_text_direct() {
        let msg = serde_json::json!({
            "payload": {
                "body": { "data": "SGVsbG8gV29ybGQ" }
            }
        });
        let body = extract_body_text(&msg);
        assert_eq!(body, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_body_text_from_parts() {
        let msg = serde_json::json!({
            "payload": {
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": "UGxhaW4gdGV4dA" }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": "PGh0bWw-" }
                    }
                ]
            }
        });
        let body = extract_body_text(&msg);
        assert_eq!(body, Some("Plain text".to_string()));
    }

    #[test]
    fn test_gmail_provider_new() {
        let config = test_config();
        let provider = GmailProvider::new(config.clone());
        assert_eq!(provider.config.client_id, "test-client-id");
    }

    #[test]
    fn test_extract_body_html_from_parts() {
        // "PGh0bWw-" is base64url for "<html>"
        let msg = serde_json::json!({
            "payload": {
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": "UGxhaW4gdGV4dA" }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": "PGh0bWw-" }
                    }
                ]
            }
        });
        let html = extract_body_html(&msg);
        assert_eq!(html, Some("<html>".to_string()));
    }

    #[test]
    fn test_extract_body_html_direct() {
        // Simple non-multipart message where payload itself is text/html.
        let msg = serde_json::json!({
            "payload": {
                "mimeType": "text/html",
                "body": { "data": "PHA-SGVsbG88L3A-" }
            }
        });
        let html = extract_body_html(&msg);
        assert_eq!(html, Some("<p>Hello</p>".to_string()));
    }

    #[test]
    fn test_extract_body_html_nested() {
        // Nested multipart/alternative inside multipart/mixed.
        let msg = serde_json::json!({
            "payload": {
                "mimeType": "multipart/mixed",
                "parts": [
                    {
                        "mimeType": "multipart/alternative",
                        "parts": [
                            {
                                "mimeType": "text/plain",
                                "body": { "data": "UGxhaW4" }
                            },
                            {
                                "mimeType": "text/html",
                                "body": { "data": "PGI-Qm9sZDwvYj4" }
                            }
                        ]
                    }
                ]
            }
        });
        let html = extract_body_html(&msg);
        assert_eq!(html, Some("<b>Bold</b>".to_string()));
    }

    #[test]
    fn test_extract_body_html_none() {
        // Message with no HTML part.
        let msg = serde_json::json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": "UGxhaW4" }
            }
        });
        let html = extract_body_html(&msg);
        assert!(html.is_none());
    }

    #[test]
    fn test_parse_message_includes_body_html() {
        // Ensure parse_message populates body_html when HTML part exists.
        let msg = serde_json::json!({
            "id": "html-msg",
            "threadId": "thread-1",
            "snippet": "Hello",
            "labelIds": ["INBOX"],
            "internalDate": "1700000000000",
            "payload": {
                "headers": [
                    { "name": "From", "value": "sender@test.com" },
                    { "name": "To", "value": "me@test.com" },
                    { "name": "Subject", "value": "HTML Test" }
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": "SGVsbG8" }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": "PHA-SGVsbG88L3A-" }
                    }
                ]
            }
        });
        let email = GmailProvider::parse_message(&msg, &std::collections::HashMap::new()).unwrap();
        assert_eq!(email.body, Some("Hello".to_string()));
        assert!(email.body_html.is_some());
        // Sanitizer should preserve <p> tags.
        assert!(email.body_html.unwrap().contains("<p>"));
    }
}

/// Build a minimal RFC 2822 message for Gmail's `messages.send` API.
#[allow(clippy::too_many_arguments)]
fn build_rfc2822(
    in_reply_to: Option<&str>,
    to: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
    subject: &str,
    _thread_id: Option<&str>,
    body_text: Option<&str>,
    body_html: Option<&str>,
) -> String {
    let mut headers = format!("To: {to}\r\nSubject: {subject}\r\n");
    if let Some(cc) = cc {
        headers.push_str(&format!("Cc: {cc}\r\n"));
    }
    if let Some(bcc) = bcc {
        headers.push_str(&format!("Bcc: {bcc}\r\n"));
    }
    if let Some(reply_id) = in_reply_to {
        headers.push_str(&format!(
            "In-Reply-To: <{reply_id}>\r\nReferences: <{reply_id}>\r\n"
        ));
    }
    headers.push_str("MIME-Version: 1.0\r\n");

    if let Some(html) = body_html {
        headers.push_str("Content-Type: text/html; charset=UTF-8\r\n\r\n");
        headers.push_str(html);
    } else {
        headers.push_str("Content-Type: text/plain; charset=UTF-8\r\n\r\n");
        headers.push_str(body_text.unwrap_or(""));
    }
    headers
}
