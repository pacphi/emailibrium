//! Shared helpers for resolving email providers from account state.

use axum::http::StatusCode;

use crate::email::provider::EmailProvider;
use crate::email::types::{ProviderConfig, ProviderKind};
use crate::AppState;

pub fn resolve_gmail_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
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

pub fn resolve_outlook_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
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

/// Build a provider instance and get the access token for an account.
pub async fn resolve_provider_and_token(
    state: &AppState,
    account_id: &str,
) -> Result<(Box<dyn EmailProvider>, String, ProviderKind), (StatusCode, String)> {
    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    let access_token = state
        .oauth_manager
        .get_access_token(account_id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;

    let provider: Box<dyn EmailProvider> = match account.provider {
        ProviderKind::Gmail => {
            let config = resolve_gmail_config(state)?;
            Box::new(crate::email::gmail::GmailProvider::new(config))
        }
        ProviderKind::Outlook => {
            let config = resolve_outlook_config(state)?;
            Box::new(crate::email::outlook::OutlookProvider::new(config))
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Provider {} not supported for this operation", account.provider.as_str()),
            ));
        }
    };

    Ok((provider, access_token, account.provider))
}
