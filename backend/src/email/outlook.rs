//! Outlook/Microsoft 365 provider implementation (DDD-005 ACL).
//!
//! Wraps the Microsoft Graph API using reqwest, translating between
//! Graph-specific JSON responses and the domain EmailMessage model.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::provider::{EmailProvider, ProviderError};
use super::types::{EmailMessage, EmailPage, ListParams, OAuthTokens, ProviderConfig};

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
