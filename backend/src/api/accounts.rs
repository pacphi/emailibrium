//! Account management API endpoints (DDD-005).
//!
//! - POST   /api/v1/auth/gmail/connect    -- initiate Gmail OAuth flow (redirect)
//! - POST   /api/v1/auth/outlook/connect  -- initiate Outlook OAuth flow (redirect)
//! - GET    /api/v1/auth/callback         -- OAuth callback handler
//! - GET    /api/v1/auth/accounts         -- list connected accounts
//! - PATCH  /api/v1/auth/accounts/:id     -- update account settings
//! - DELETE /api/v1/auth/accounts/:id     -- disconnect account
//! - GET    /api/v1/auth/accounts/:id/status -- account sync status
//! - POST   /api/v1/auth/accounts/:id/remove-labels -- remove all app labels
//! - POST   /api/v1/auth/accounts/:id/unarchive     -- unarchive all messages

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Redirect,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::email::provider::EmailProvider;
use crate::email::types::{ProviderConfig, ProviderKind};
use crate::AppState;

/// Build account/auth API routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/gmail/connect", post(connect_gmail).get(connect_gmail))
        .route(
            "/outlook/connect",
            post(connect_outlook).get(connect_outlook),
        )
        .route("/callback", get(oauth_callback))
        .route("/accounts", get(list_accounts))
        .route(
            "/accounts/{id}",
            delete(disconnect_account).patch(patch_account),
        )
        .route("/accounts/{id}/status", get(account_status))
        .route(
            "/accounts/{id}/remove-labels",
            post(remove_labels_handler),
        )
        .route("/accounts/{id}/unarchive", post(unarchive_handler))
}

// --- Response types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    pub id: String,
    pub provider: String,
    pub email_address: String,
    #[serde(rename = "isActive")]
    pub is_active: bool,
    pub status: String,
    pub email_count: u64,
    pub last_sync_at: Option<String>,
    pub archive_strategy: String,
    pub label_prefix: String,
    pub sync_depth: String,
    pub sync_frequency: i32,
}

#[derive(Debug, Serialize)]
pub struct AccountStatusResponse {
    pub account_id: String,
    pub status: String,
    pub last_sync_at: Option<String>,
    pub emails_synced: u64,
    pub sync_failures: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountRequest {
    pub archive_strategy: Option<String>,
    pub label_prefix: Option<String>,
    pub sync_depth: Option<String>,
    pub sync_frequency: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DangerZoneResponse {
    pub messages_processed: u64,
    pub labels_deleted: u64,
}

// --- Helpers ---

/// Resolve provider config from the VectorConfig's OAuth section.
fn resolve_gmail_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
    let oauth = &state.vector_service.config.oauth;
    let gmail = &oauth.gmail;

    let client_id = std::env::var(&gmail.client_id_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Gmail OAuth not configured: missing env var {}",
                gmail.client_id_env
            ),
        )
    })?;

    let client_secret = std::env::var(&gmail.client_secret_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Gmail OAuth not configured: missing env var {}",
                gmail.client_secret_env
            ),
        )
    })?;

    Ok(ProviderConfig {
        client_id,
        client_secret,
        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
        auth_url: gmail.auth_url.clone(),
        token_url: gmail.token_url.clone(),
        scopes: gmail.scopes.clone(),
    })
}

fn resolve_outlook_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
    let oauth = &state.vector_service.config.oauth;
    let outlook = &oauth.outlook;

    let client_id = std::env::var(&outlook.client_id_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Outlook OAuth not configured: missing env var {}",
                outlook.client_id_env
            ),
        )
    })?;

    let client_secret = std::env::var(&outlook.client_secret_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Outlook OAuth not configured: missing env var {}",
                outlook.client_secret_env
            ),
        )
    })?;

    Ok(ProviderConfig {
        client_id,
        client_secret,
        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
        auth_url: outlook.auth_url(),
        token_url: outlook.token_url(),
        scopes: outlook.scopes.clone(),
    })
}

// --- Handlers ---

/// POST|GET /api/v1/auth/gmail/connect
///
/// Generates a Gmail OAuth authorization URL and redirects the user's browser.
async fn connect_gmail(State(state): State<AppState>) -> Result<Redirect, (StatusCode, String)> {
    let config = resolve_gmail_config(&state)?;
    let (auth_url, _csrf_state) = state.oauth_manager.authorization_url(&config, "gmail");

    Ok(Redirect::temporary(&auth_url))
}

/// POST|GET /api/v1/auth/outlook/connect
///
/// Generates an Outlook OAuth authorization URL and redirects the user's browser.
async fn connect_outlook(State(state): State<AppState>) -> Result<Redirect, (StatusCode, String)> {
    let config = resolve_outlook_config(&state)?;
    let (auth_url, _csrf_state) = state.oauth_manager.authorization_url(&config, "outlook");

    Ok(Redirect::temporary(&auth_url))
}

/// GET /api/v1/auth/callback
///
/// Handles the OAuth callback from the provider. Exchanges the authorization
/// code for tokens, fetches the user's email, and persists the account.
async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Result<Redirect, (StatusCode, String)> {
    let frontend_url = &state.vector_service.config.oauth.frontend_url;

    // Check for OAuth errors from the provider.
    if let Some(ref error) = params.error {
        let desc = params.error_description.as_deref().unwrap_or("Unknown");
        tracing::warn!("OAuth callback error: {error} - {desc}");
        return Ok(Redirect::temporary(&format!(
            "{frontend_url}/?error=oauth_denied&message={}",
            urlencoding::encode(desc)
        )));
    }

    let code = params.code.as_deref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Missing authorization code".to_string(),
        )
    })?;

    // Extract provider from the state parameter (format: "{provider}:{nonce}").
    let state_param = params.state.as_deref().unwrap_or("");
    let provider_str = state_param.split(':').next().unwrap_or("");

    let (provider, config) = match provider_str {
        "gmail" => (ProviderKind::Gmail, resolve_gmail_config(&state)?),
        "outlook" => (ProviderKind::Outlook, resolve_outlook_config(&state)?),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unknown provider in state parameter: {provider_str}"),
            ));
        }
    };

    // Exchange authorization code for tokens.
    let tokens = state
        .oauth_manager
        .exchange_code(&config, code)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    // Fetch the user's email address from the provider.
    let email = match provider {
        ProviderKind::Gmail => {
            let gmail = crate::email::gmail::GmailProvider::new(config);
            gmail
                .get_user_email(&tokens.access_token)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        ProviderKind::Outlook => {
            let outlook = crate::email::outlook::OutlookProvider::new(config);
            outlook
                .get_user_email(&tokens.access_token)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported provider for OAuth".to_string(),
            ));
        }
    };

    // Reuse existing account ID if re-authenticating, otherwise create a new one.
    let account_id = state
        .oauth_manager
        .find_account_id_by_email(&email)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    state
        .oauth_manager
        .save_account(&account_id, provider, &email, &tokens)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tracing::info!(
        "Account connected: {} ({}) as {}",
        email,
        provider.as_str(),
        account_id
    );

    // Publish AccountConnected domain event (Audit Item #20).
    state
        .event_bus
        .emit(
            &account_id,
            crate::events::DomainEvent::AccountConnected {
                account_id: account_id.clone(),
                provider: provider.as_str().to_string(),
                email_address: email.clone(),
            },
        )
        .await;

    // Redirect back to the frontend with success params.
    Ok(Redirect::temporary(&format!(
        "{frontend_url}/?provider={}&status=connected",
        provider.as_str()
    )))
}

/// GET /api/v1/auth/accounts
///
/// Returns all connected accounts (matching the frontend's `getAccounts()` call).
async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountResponse>>, (StatusCode, String)> {
    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut responses = Vec::with_capacity(accounts.len());
    for account in accounts {
        let sync = state
            .oauth_manager
            .get_sync_state(&account.id)
            .await
            .ok()
            .flatten();

        responses.push(AccountResponse {
            id: account.id,
            provider: account.provider.as_str().to_string(),
            email_address: account.email_address,
            is_active: account.status == crate::email::AccountStatus::Connected,
            status: account.status.as_str().to_string(),
            email_count: sync.as_ref().map(|s| s.emails_synced).unwrap_or(0),
            last_sync_at: sync.and_then(|s| s.last_sync_at.map(|dt| dt.to_rfc3339())),
            archive_strategy: account.archive_strategy,
            label_prefix: account.label_prefix,
            sync_depth: account.sync_depth,
            sync_frequency: account.sync_frequency,
        });
    }

    Ok(Json(responses))
}

/// DELETE /api/v1/auth/accounts/:id
///
/// Disconnects an account, clearing tokens and setting status to disconnected.
async fn disconnect_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Validate UUID format.
    Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    state
        .oauth_manager
        .disconnect_account(&id)
        .await
        .map_err(|e| match e {
            crate::email::oauth::OAuthError::AccountNotFound(_) => {
                (StatusCode::NOT_FOUND, "Account not found".to_string())
            }
            other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        })?;

    tracing::info!("Account disconnected: {}", id);
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/auth/accounts/:id/status
///
/// Returns the sync/health status for a connected account.
async fn account_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AccountStatusResponse>, (StatusCode, String)> {
    Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    let sync = state
        .oauth_manager
        .get_sync_state(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    Ok(Json(AccountStatusResponse {
        account_id: sync.account_id,
        status: sync.status,
        last_sync_at: sync.last_sync_at.map(|dt| dt.to_rfc3339()),
        emails_synced: sync.emails_synced,
        sync_failures: sync.sync_failures,
        last_error: sync.last_error,
    }))
}

/// PATCH /api/v1/auth/accounts/:id
///
/// Update account settings (archive strategy, label prefix, sync depth, sync frequency).
async fn patch_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    state
        .oauth_manager
        .update_account_settings(
            &id,
            body.archive_strategy.as_deref(),
            body.label_prefix.as_deref(),
            body.sync_depth.as_deref(),
            body.sync_frequency,
        )
        .await
        .map_err(|e| match e {
            crate::email::oauth::OAuthError::ValidationError(msg) => {
                (StatusCode::UNPROCESSABLE_ENTITY, msg)
            }
            crate::email::oauth::OAuthError::AccountNotFound(_) => {
                (StatusCode::NOT_FOUND, "Account not found".to_string())
            }
            other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/auth/accounts/:id/remove-labels
///
/// Remove all Emailibrium-created labels from messages and delete label definitions.
async fn remove_labels_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DangerZoneResponse>, (StatusCode, String)> {
    Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    // Get account info and access token.
    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    let access_token = state
        .oauth_manager
        .get_access_token(&id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let prefix = &account.label_prefix;
    let config = match account.provider {
        ProviderKind::Gmail => resolve_gmail_config(&state)?,
        ProviderKind::Outlook => resolve_outlook_config(&state)?,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported provider for label operations".to_string(),
            ))
        }
    };

    // Build provider and list labels.
    let labels: Vec<(String, String)> = match account.provider {
        ProviderKind::Gmail => {
            let gmail = crate::email::gmail::GmailProvider::new(config);
            gmail
                .list_labels(&access_token)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        ProviderKind::Outlook => {
            let outlook = crate::email::outlook::OutlookProvider::new(config);
            outlook
                .list_labels(&access_token)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        _ => Vec::new(),
    };

    // Filter to labels matching the prefix and delete them.
    let matching: Vec<_> = labels
        .into_iter()
        .filter(|(_, name)| name.starts_with(prefix))
        .collect();
    let mut labels_deleted = 0u64;

    for (label_id, _name) in &matching {
        let result = match account.provider {
            ProviderKind::Gmail => {
                let gmail =
                    crate::email::gmail::GmailProvider::new(resolve_gmail_config(&state)?);
                gmail.delete_label(&access_token, label_id).await
            }
            ProviderKind::Outlook => {
                let outlook =
                    crate::email::outlook::OutlookProvider::new(resolve_outlook_config(&state)?);
                outlook.delete_label(&access_token, label_id).await
            }
            _ => Ok(()),
        };
        if result.is_ok() {
            labels_deleted += 1;
        }
    }

    tracing::info!(
        "Removed {labels_deleted} labels with prefix '{prefix}' for account {id}"
    );

    Ok(Json(DangerZoneResponse {
        messages_processed: 0,
        labels_deleted,
    }))
}

/// POST /api/v1/auth/accounts/:id/unarchive
///
/// Move all archived messages back to inbox.
async fn unarchive_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DangerZoneResponse>, (StatusCode, String)> {
    Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid account ID format".to_string(),
        )
    })?;

    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    let access_token = state
        .oauth_manager
        .get_access_token(&id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let config = match account.provider {
        ProviderKind::Gmail => resolve_gmail_config(&state)?,
        ProviderKind::Outlook => resolve_outlook_config(&state)?,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported provider for unarchive".to_string(),
            ))
        }
    };

    // List messages not in inbox (archived), then unarchive each.
    let params = crate::email::types::ListParams {
        query: Some("-in:inbox".to_string()),
        page_token: None,
        max_results: 500,
        label: None,
    };

    let mut messages_processed = 0u64;
    match account.provider {
        ProviderKind::Gmail => {
            let gmail = crate::email::gmail::GmailProvider::new(config);
            if let Ok(page) = gmail.list_messages(&access_token, &params).await {
                for msg in &page.messages {
                    if gmail
                        .unarchive_message(&access_token, &msg.id)
                        .await
                        .is_ok()
                    {
                        messages_processed += 1;
                    }
                }
            }
        }
        ProviderKind::Outlook => {
            let outlook = crate::email::outlook::OutlookProvider::new(config);
            if let Ok(page) = outlook.list_messages(&access_token, &params).await {
                for msg in &page.messages {
                    if outlook
                        .unarchive_message(&access_token, &msg.id)
                        .await
                        .is_ok()
                    {
                        messages_processed += 1;
                    }
                }
            }
        }
        _ => {}
    }

    tracing::info!("Unarchived {messages_processed} messages for account {id}");

    Ok(Json(DangerZoneResponse {
        messages_processed,
        labels_deleted: 0,
    }))
}
