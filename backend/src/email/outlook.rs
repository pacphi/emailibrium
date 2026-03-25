//! Outlook/Microsoft 365 provider implementation (DDD-005 ACL).
//!
//! Wraps the Microsoft Graph API using reqwest, translating between
//! Graph-specific JSON responses and the domain EmailMessage model.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::provider::{EmailProvider, ProviderError};
use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens, ProviderConfig};

// ---------------------------------------------------------------------------
// Outlook Delta Sync Types (R-01)
// ---------------------------------------------------------------------------

/// High-level response from Outlook delta sync via Graph delta query.
///
/// Convenience wrapper providing a simpler interface than the lower-level
/// `OutlookDeltaResult` in `delta.rs`, including categorized message IDs
/// and the next delta link for subsequent calls.
#[derive(Debug, Clone, Default)]
pub struct DeltaResponse {
    /// Message IDs that were added or modified since the last delta.
    pub added_or_modified: Vec<String>,
    /// Message IDs that were deleted since the last delta.
    pub deleted: Vec<String>,
    /// The delta link to pass in the next call for incremental changes.
    /// `None` if more pages remain (use `next_link` to continue).
    pub delta_link: Option<String>,
    /// Pagination link for multi-page delta responses.
    pub next_link: Option<String>,
}

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0/me";

/// Outlook provider using the Microsoft Graph API.
pub struct OutlookProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

impl OutlookProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Fetch the authenticated user's email address from the Graph profile.
    pub async fn get_user_email(&self, access_token: &str) -> Result<String, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(GRAPH_API_BASE.to_string())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_graph_error(&resp)?;

        // Graph returns mail or userPrincipalName.
        resp["mail"]
            .as_str()
            .or_else(|| resp["userPrincipalName"].as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| ProviderError::ParseError("Missing mail in profile".into()))
    }

    /// Parse a Graph API message JSON into an EmailMessage.
    fn parse_message(msg: &serde_json::Value) -> Result<EmailMessage, ProviderError> {
        let id = msg["id"].as_str().unwrap_or_default().to_string();

        let conversation_id = msg["conversationId"].as_str().map(|s| s.to_string());

        let subject = msg["subject"].as_str().unwrap_or_default().to_string();

        let snippet = msg["bodyPreview"].as_str().unwrap_or_default().to_string();

        let from = msg["from"]["emailAddress"]["address"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        let to: Vec<String> = msg["toRecipients"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r["emailAddress"]["address"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let is_read = msg["isRead"].as_bool().unwrap_or(false);

        let date = msg["receivedDateTime"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        // Graph categories are the closest analog to labels.
        let labels: Vec<String> = msg["categories"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let body = msg["body"]["content"].as_str().map(|s| s.to_string());

        Ok(EmailMessage {
            id,
            thread_id: conversation_id,
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

#[async_trait]
impl EmailProvider for OutlookProvider {
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
                ("scope", &self.config.scopes.join(" ")),
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
                ("scope", &self.config.scopes.join(" ")),
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
            "{GRAPH_API_BASE}/messages?$top={}&$orderby=receivedDateTime desc",
            params.max_results
        );

        if let Some(ref q) = params.query {
            url.push_str(&format!("&$search=\"{}\"", urlencoding::encode(q)));
        }
        if let Some(ref pt) = params.page_token {
            // Graph uses full URLs for @odata.nextLink; if it's a full URL use it directly.
            if pt.starts_with("http") {
                url = pt.clone();
            } else {
                url.push_str(&format!("&$skip={pt}"));
            }
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

        check_graph_error(&resp)?;

        let values = resp["value"].as_array().cloned().unwrap_or_default();

        let mut messages = Vec::with_capacity(values.len());
        for msg in &values {
            messages.push(Self::parse_message(msg)?);
        }

        let next_page_token = resp["@odata.nextLink"].as_str().map(|s| s.to_string());

        Ok(EmailPage {
            messages,
            next_page_token,
            result_size_estimate: None,
        })
    }

    async fn get_message(
        &self,
        access_token: &str,
        id: &str,
    ) -> Result<EmailMessage, ProviderError> {
        let resp: serde_json::Value = self
            .http
            .get(format!("{GRAPH_API_BASE}/messages/{id}"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        check_graph_error(&resp)?;
        Self::parse_message(&resp)
    }

    async fn archive_message(&self, access_token: &str, id: &str) -> Result<(), ProviderError> {
        // Outlook archive = move to the Archive folder.
        // First, find the Archive folder ID.
        let folders: serde_json::Value = self
            .http
            .get(format!(
                "{GRAPH_API_BASE}/mailFolders?$filter=displayName eq 'Archive'"
            ))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let archive_id = folders["value"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|f| f["id"].as_str())
            .ok_or_else(|| ProviderError::RequestFailed("Archive folder not found".into()))?;

        let body = serde_json::json!({
            "destinationId": archive_id
        });

        let resp = self
            .http
            .post(format!("{GRAPH_API_BASE}/messages/{id}/move"))
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
        // Outlook uses categories instead of labels.
        // First, get existing categories, then merge.
        let msg: serde_json::Value = self
            .http
            .get(format!("{GRAPH_API_BASE}/messages/{id}?$select=categories"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let mut existing: Vec<String> = msg["categories"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        for label in labels {
            if !existing.contains(label) {
                existing.push(label.clone());
            }
        }

        let body = serde_json::json!({
            "categories": existing
        });

        let resp = self
            .http
            .patch(format!("{GRAPH_API_BASE}/messages/{id}"))
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
        let msg: serde_json::Value = self
            .http
            .get(format!("{GRAPH_API_BASE}/messages/{id}?$select=categories"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ProviderError::ParseError(e.to_string()))?;

        let existing: Vec<String> = msg["categories"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let filtered: Vec<String> = existing
            .into_iter()
            .filter(|c| !labels.contains(c))
            .collect();

        let body = serde_json::json!({
            "categories": filtered
        });

        let resp = self
            .http
            .patch(format!("{GRAPH_API_BASE}/messages/{id}"))
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

    async fn create_label(&self, _access_token: &str, name: &str) -> Result<String, ProviderError> {
        // Outlook categories don't need pre-creation via API for basic use.
        // They are created implicitly when assigned to a message.
        // Return the name itself as the "ID" since categories are name-based.
        Ok(name.to_string())
    }
}

impl OutlookProvider {
    // -----------------------------------------------------------------------
    // Outlook Delta Query (incremental sync)
    // -----------------------------------------------------------------------

    /// Fetch message deltas from the inbox using Microsoft Graph delta query.
    ///
    /// On the first call, pass `delta_link = None` to get the initial set.
    /// On subsequent calls, pass the `@odata.deltaLink` from the previous
    /// response to get only changes since then.
    ///
    /// Returns the raw JSON response which can be parsed with
    /// `delta::parse_outlook_delta`.
    pub async fn delta_messages(
        &self,
        access_token: &str,
        delta_link: Option<&str>,
    ) -> Result<serde_json::Value, ProviderError> {
        let url = match delta_link {
            Some(link) => link.to_string(),
            None => format!(
                "{GRAPH_API_BASE}/mailFolders/inbox/messages/delta?$top=50\
                 &$select=id,subject,receivedDateTime,isRead,categories,bodyPreview,from,toRecipients"
            ),
        };

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

        check_graph_error(&resp)?;
        Ok(resp)
    }

    /// Fetch message changes via Graph delta query, returning a typed
    /// `DeltaResponse`.
    ///
    /// On the first call, pass `delta_link = None`. On subsequent calls,
    /// pass the `delta_link` from the previous `DeltaResponse`.
    pub async fn delta_messages_typed(
        &self,
        access_token: &str,
        delta_link: Option<&str>,
    ) -> Result<DeltaResponse, ProviderError> {
        let resp = self.delta_messages(access_token, delta_link).await?;

        let result = super::delta::parse_outlook_delta(&resp).map_err(ProviderError::ParseError)?;

        let next_link = resp["@odata.nextLink"].as_str().map(|s| s.to_string());

        Ok(DeltaResponse {
            added_or_modified: result.added_or_modified_ids,
            deleted: result.deleted_ids,
            delta_link: result.delta_link,
            next_link,
        })
    }

    /// Fetch all delta pages until a `@odata.deltaLink` is obtained.
    ///
    /// Returns the aggregated delta result including the new delta link
    /// for subsequent calls.
    pub async fn delta_messages_all(
        &self,
        access_token: &str,
        delta_link: Option<&str>,
    ) -> Result<super::delta::OutlookDeltaResult, ProviderError> {
        let mut all_added = Vec::new();
        let mut all_deleted = Vec::new();
        let mut current_link: Option<String> = delta_link.map(|s| s.to_string());
        let mut final_delta_link = None;

        loop {
            let resp = self
                .delta_messages(access_token, current_link.as_deref())
                .await?;

            let page_result =
                super::delta::parse_outlook_delta(&resp).map_err(ProviderError::ParseError)?;

            all_added.extend(page_result.added_or_modified_ids);
            all_deleted.extend(page_result.deleted_ids);

            if page_result.delta_link.is_some() {
                final_delta_link = page_result.delta_link;
                break;
            }

            // Check for @odata.nextLink for pagination.
            match resp["@odata.nextLink"].as_str() {
                Some(next) => current_link = Some(next.to_string()),
                None => break,
            }
        }

        Ok(super::delta::OutlookDeltaResult {
            added_or_modified_ids: all_added,
            deleted_ids: all_deleted,
            delta_link: final_delta_link,
        })
    }
}

/// Check if a Graph API response contains an error object.
fn check_graph_error(resp: &serde_json::Value) -> Result<(), ProviderError> {
    if let Some(error) = resp["error"].as_object() {
        let code = error
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or("Unknown");
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");

        if code == "InvalidAuthenticationToken" || code == "AuthenticationError" {
            return Err(ProviderError::TokenExpired(message.to_string()));
        }
        if code == "ErrorItemNotFound" {
            return Err(ProviderError::NotFound(message.to_string()));
        }
        if code == "ErrorTooManyRequests" || code == "429" {
            return Err(ProviderError::RateLimited {
                retry_after_secs: 60,
            });
        }

        return Err(ProviderError::RequestFailed(format!(
            "Graph API error ({code}): {message}"
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
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
            scopes: vec!["Mail.Read".to_string()],
        }
    }

    #[test]
    fn test_delta_response_default() {
        let resp = DeltaResponse::default();
        assert!(resp.added_or_modified.is_empty());
        assert!(resp.deleted.is_empty());
        assert!(resp.delta_link.is_none());
        assert!(resp.next_link.is_none());
    }

    #[test]
    fn test_delta_response_fields() {
        let resp = DeltaResponse {
            added_or_modified: vec!["msg-1".to_string(), "msg-2".to_string()],
            deleted: vec!["msg-3".to_string()],
            delta_link: Some("https://graph.microsoft.com/delta?token=abc".to_string()),
            next_link: None,
        };
        assert_eq!(resp.added_or_modified.len(), 2);
        assert_eq!(resp.deleted.len(), 1);
        assert!(resp.delta_link.is_some());
    }

    #[test]
    fn test_check_graph_error_ok() {
        let resp = serde_json::json!({"value": []});
        assert!(check_graph_error(&resp).is_ok());
    }

    #[test]
    fn test_check_graph_error_auth() {
        let resp = serde_json::json!({
            "error": {
                "code": "InvalidAuthenticationToken",
                "message": "Token expired"
            }
        });
        let err = check_graph_error(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::TokenExpired(_)));
    }

    #[test]
    fn test_check_graph_error_not_found() {
        let resp = serde_json::json!({
            "error": {
                "code": "ErrorItemNotFound",
                "message": "Item not found"
            }
        });
        let err = check_graph_error(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)));
    }

    #[test]
    fn test_check_graph_error_rate_limited() {
        let resp = serde_json::json!({
            "error": {
                "code": "ErrorTooManyRequests",
                "message": "Too many requests"
            }
        });
        let err = check_graph_error(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::RateLimited { .. }));
    }

    #[test]
    fn test_check_graph_error_generic() {
        let resp = serde_json::json!({
            "error": {
                "code": "GeneralException",
                "message": "Something went wrong"
            }
        });
        let err = check_graph_error(&resp).unwrap_err();
        assert!(matches!(err, ProviderError::RequestFailed(_)));
        assert!(err.to_string().contains("GeneralException"));
    }

    #[test]
    fn test_parse_message_basic() {
        let msg = serde_json::json!({
            "id": "AAMkAG1",
            "conversationId": "conv-1",
            "subject": "Test Email",
            "bodyPreview": "Preview text",
            "from": {
                "emailAddress": { "address": "sender@test.com" }
            },
            "toRecipients": [
                { "emailAddress": { "address": "me@test.com" } },
                { "emailAddress": { "address": "other@test.com" } }
            ],
            "isRead": false,
            "receivedDateTime": "2024-01-15T10:30:00Z",
            "categories": ["Important", "Work"],
            "body": { "content": "<p>Full body</p>" }
        });
        let email = OutlookProvider::parse_message(&msg).unwrap();
        assert_eq!(email.id, "AAMkAG1");
        assert_eq!(email.thread_id, Some("conv-1".to_string()));
        assert_eq!(email.subject, "Test Email");
        assert_eq!(email.snippet, "Preview text");
        assert_eq!(email.from, "sender@test.com");
        assert_eq!(email.to.len(), 2);
        assert!(!email.is_read);
        assert_eq!(email.labels, vec!["Important", "Work"]);
        assert_eq!(email.body, Some("<p>Full body</p>".to_string()));
    }

    #[test]
    fn test_parse_message_read() {
        let msg = serde_json::json!({
            "id": "read-msg",
            "isRead": true,
            "receivedDateTime": "2024-01-15T10:30:00Z"
        });
        let email = OutlookProvider::parse_message(&msg).unwrap();
        assert!(email.is_read);
    }

    #[test]
    fn test_parse_message_missing_optional_fields() {
        let msg = serde_json::json!({
            "id": "minimal",
            "receivedDateTime": "2024-01-15T10:30:00Z"
        });
        let email = OutlookProvider::parse_message(&msg).unwrap();
        assert_eq!(email.id, "minimal");
        assert!(email.thread_id.is_none());
        assert!(email.from.is_empty());
        assert!(email.to.is_empty());
        assert!(email.labels.is_empty());
    }

    #[test]
    fn test_outlook_provider_new() {
        let config = test_config();
        let provider = OutlookProvider::new(config.clone());
        assert_eq!(provider.config.client_id, "test-client-id");
    }

    #[test]
    fn test_create_label_returns_name() {
        // Outlook categories are name-based; create_label just returns the name.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let provider = OutlookProvider::new(test_config());
        let result = rt.block_on(provider.create_label("token", "TestCategory"));
        assert_eq!(result.unwrap(), "TestCategory");
    }
}
