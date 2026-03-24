//! Gmail provider implementation (DDD-005 ACL).
//!
//! Wraps the Gmail REST API v1 using reqwest, translating between
//! Google-specific JSON responses and the domain EmailMessage model.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use super::provider::{EmailProvider, ProviderError};
use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens, ProviderConfig};

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

        resp["emailAddress"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ProviderError::ParseError("Missing emailAddress in profile".into()))
    }

    /// Parse a Gmail API message JSON into an EmailMessage.
    fn parse_message(msg: &serde_json::Value) -> Result<EmailMessage, ProviderError> {
        let id = msg["id"].as_str().unwrap_or_default().to_string();
        let thread_id = msg["threadId"].as_str().map(|s| s.to_string());
        let snippet = msg["snippet"].as_str().unwrap_or_default().to_string();

        let labels: Vec<String> = msg["labelIds"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

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

        // Extract plain-text body from parts.
        let body = extract_body_text(msg);

        Ok(EmailMessage {
            id,
            thread_id,
            from,
            to,
            subject,
            snippet,
            body,
            labels,
            date,
            is_read,
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

/// Decode Gmail's URL-safe base64 encoded content.
fn decode_base64url(data: &str) -> Option<String> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
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

        // Fetch full message details for each message ID.
        let mut messages = Vec::with_capacity(message_refs.len());
        for msg_ref in &message_refs {
            let msg_id = msg_ref["id"].as_str().unwrap_or_default();
            if msg_id.is_empty() {
                continue;
            }
            let full = self
                .http
                .get(format!("{GMAIL_API_BASE}/messages/{msg_id}?format=full"))
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
                .json::<serde_json::Value>()
                .await
                .map_err(|e| ProviderError::ParseError(e.to_string()))?;

            messages.push(Self::parse_message(&full)?);
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
        Self::parse_message(&resp)
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
}

impl GmailProvider {
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
