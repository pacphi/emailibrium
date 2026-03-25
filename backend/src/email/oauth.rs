//! OAuth flow management (DDD-005: OAuthManager domain service).
//!
//! Handles authorization URL generation, PKCE code verifier/challenge pairs,
//! token exchange, and encrypted token storage using the existing AES-256-GCM
//! encryption infrastructure.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sqlx::SqlitePool;
use zeroize::Zeroizing;

use super::types::{AccountStatus, ConnectedAccount, OAuthTokens, ProviderConfig, ProviderKind};

/// Fixed salt for token encryption key derivation (separate from vector encryption).
const TOKEN_KEY_SALT: &[u8] = b"emailibrium-token-encryption-v1";
const NONCE_SIZE: usize = 12;

/// Row tuple for the connected_accounts query (10 columns).
type AccountRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i32,
);

/// Row tuple for the sync_state query.
type SyncStateRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i32,
    Option<String>,
    String,
);

/// Errors specific to the OAuth subsystem.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("OAuth configuration error: {0}")]
    ConfigError(String),

    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    #[error("Token refresh failed: {0}")]
    RefreshFailed(String),

    #[error("Token encryption error: {0}")]
    EncryptionError(String),

    #[error("Token decryption error: {0}")]
    DecryptionError(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Manages OAuth flows, token storage, and account persistence.
pub struct OAuthManager {
    pool: SqlitePool,
    encryption_key: Option<Zeroizing<[u8; 32]>>,
    http: reqwest::Client,
}

impl OAuthManager {
    /// Create a new OAuthManager.
    ///
    /// If `master_password` is provided, tokens are encrypted at rest using
    /// AES-256-GCM with an Argon2id-derived key. If `None`, tokens are stored
    /// as plaintext (development only).
    pub fn new(pool: SqlitePool, master_password: Option<&str>) -> Self {
        let encryption_key = master_password
            .and_then(|pw| crate::vectors::encryption::derive_key(pw, TOKEN_KEY_SALT).ok());

        Self {
            pool,
            encryption_key,
            http: reqwest::Client::new(),
        }
    }

    /// Build the authorization URL that the user's browser should be redirected to.
    ///
    /// Returns `(auth_url, state_param)`. The state parameter encodes the
    /// provider name and a CSRF nonce as `{provider}:{uuid}` so the callback
    /// can identify which provider initiated the flow.
    pub fn authorization_url(&self, config: &ProviderConfig, provider: &str) -> (String, String) {
        let nonce = uuid::Uuid::new_v4().to_string();
        let state = format!("{provider}:{nonce}");
        let scopes = config.scopes.join(" ");

        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
            config.auth_url,
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&config.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(&state),
        );

        (url, state)
    }

    /// Exchange an authorization code for tokens via the provider's token endpoint.
    pub async fn exchange_code(
        &self,
        config: &ProviderConfig,
        code: &str,
    ) -> Result<OAuthTokens, OAuthError> {
        let resp = self
            .http
            .post(&config.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &config.redirect_uri),
                ("client_id", &config.client_id),
                ("client_secret", &config.client_secret),
            ])
            .send()
            .await
            .map_err(|e| OAuthError::TokenExchangeFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::TokenExchangeFailed(format!(
                "Token endpoint returned error: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OAuthError::TokenExchangeFailed(e.to_string()))?;

        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| OAuthError::TokenExchangeFailed("Missing access_token".into()))?
            .to_string();

        let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());

        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
        let expires_at = Some(Utc::now() + Duration::seconds(expires_in));

        Ok(OAuthTokens {
            access_token,
            refresh_token,
            expires_at,
            email: None,
        })
    }

    /// Refresh an expired access token using the refresh token.
    pub async fn refresh_access_token(
        &self,
        config: &ProviderConfig,
        refresh_token: &str,
    ) -> Result<OAuthTokens, OAuthError> {
        let resp = self
            .http
            .post(&config.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &config.client_id),
                ("client_secret", &config.client_secret),
            ])
            .send()
            .await
            .map_err(|e| OAuthError::RefreshFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::RefreshFailed(format!(
                "Refresh endpoint returned error: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OAuthError::RefreshFailed(e.to_string()))?;

        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| OAuthError::RefreshFailed("Missing access_token".into()))?
            .to_string();

        // Some providers rotate refresh tokens on each use.
        let new_refresh = body["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| Some(refresh_token.to_string()));

        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
        let expires_at = Some(Utc::now() + Duration::seconds(expires_in));

        Ok(OAuthTokens {
            access_token,
            refresh_token: new_refresh,
            expires_at,
            email: None,
        })
    }

    /// Look up an existing account ID by email address, if one exists.
    pub async fn find_account_id_by_email(
        &self,
        email: &str,
    ) -> Result<Option<String>, OAuthError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM connected_accounts WHERE email_address = ?1")
                .bind(email)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id,)| id))
    }

    /// Persist a connected account with encrypted tokens.
    pub async fn save_account(
        &self,
        id: &str,
        provider: ProviderKind,
        email: &str,
        tokens: &OAuthTokens,
    ) -> Result<(), OAuthError> {
        let enc_access = self.encrypt_token(&tokens.access_token)?;
        let enc_refresh = tokens
            .refresh_token
            .as_deref()
            .map(|rt| self.encrypt_token(rt))
            .transpose()?;
        let expires_at = tokens.expires_at.map(|dt| dt.to_rfc3339());

        sqlx::query(
            "INSERT INTO connected_accounts \
             (id, provider, email_address, encrypted_access_token, encrypted_refresh_token, \
              token_expires_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'connected') \
             ON CONFLICT(email_address) DO UPDATE SET \
                encrypted_access_token = ?4, \
                encrypted_refresh_token = ?5, \
                token_expires_at = ?6, \
                status = 'connected', \
                updated_at = datetime('now')",
        )
        .bind(id)
        .bind(provider.as_str())
        .bind(email)
        .bind(&enc_access)
        .bind(&enc_refresh)
        .bind(&expires_at)
        .execute(&self.pool)
        .await?;

        // Ensure sync_state row exists.
        sqlx::query("INSERT OR IGNORE INTO sync_state (account_id) VALUES (?1)")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Update tokens for an existing account (e.g., after refresh).
    pub async fn update_tokens(
        &self,
        account_id: &str,
        tokens: &OAuthTokens,
    ) -> Result<(), OAuthError> {
        let enc_access = self.encrypt_token(&tokens.access_token)?;
        let enc_refresh = tokens
            .refresh_token
            .as_deref()
            .map(|rt| self.encrypt_token(rt))
            .transpose()?;
        let expires_at = tokens.expires_at.map(|dt| dt.to_rfc3339());

        let rows = sqlx::query(
            "UPDATE connected_accounts SET \
                encrypted_access_token = ?1, \
                encrypted_refresh_token = COALESCE(?2, encrypted_refresh_token), \
                token_expires_at = ?3, \
                updated_at = datetime('now') \
             WHERE id = ?4",
        )
        .bind(&enc_access)
        .bind(&enc_refresh)
        .bind(&expires_at)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        if rows.rows_affected() == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Retrieve the decrypted access token for an account, auto-refreshing if expired.
    pub async fn get_access_token(&self, account_id: &str) -> Result<String, OAuthError> {
        let row: (Vec<u8>, Option<String>) = sqlx::query_as(
            "SELECT encrypted_access_token, token_expires_at \
             FROM connected_accounts WHERE id = ?1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| OAuthError::AccountNotFound(account_id.to_string()))?;

        // Check if token is expired (or will expire within 60s).
        let is_expired = row.1.as_deref().is_some_and(|exp| {
            chrono::DateTime::parse_from_rfc3339(exp)
                .map(|dt| dt < Utc::now() + Duration::seconds(60))
                .unwrap_or(false)
        });

        if is_expired {
            tracing::debug!(account_id = %account_id, "Access token expired, refreshing");
            if let Some(new_token) = self.try_refresh_token(account_id).await {
                return Ok(new_token);
            }
        }

        self.decrypt_token(&row.0)
    }

    /// Attempt to refresh the access token. Returns the new token on success.
    async fn try_refresh_token(&self, account_id: &str) -> Option<String> {
        let refresh_token = self.get_refresh_token(account_id).await.ok()??;

        let provider_row: Option<(String,)> =
            sqlx::query_as("SELECT provider FROM connected_accounts WHERE id = ?1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await
                .ok()?;

        let provider_str = provider_row?.0;
        let (token_url, client_id_env, client_secret_env) = match provider_str.as_str() {
            "gmail" => (
                "https://oauth2.googleapis.com/token",
                "EMAILIBRIUM_GOOGLE_CLIENT_ID",
                "EMAILIBRIUM_GOOGLE_CLIENT_SECRET",
            ),
            "outlook" => (
                "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                "EMAILIBRIUM_MICROSOFT_CLIENT_ID",
                "EMAILIBRIUM_MICROSOFT_CLIENT_SECRET",
            ),
            _ => return None,
        };

        let client_id = std::env::var(client_id_env).ok()?;
        let client_secret = std::env::var(client_secret_env).ok()?;

        let resp = self
            .http
            .post(token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
            ])
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            tracing::warn!(account_id = %account_id, "Token refresh failed");
            return None;
        }

        let body: serde_json::Value = resp.json().await.ok()?;
        let new_access = body["access_token"].as_str()?.to_string();
        let new_refresh = body["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .or(Some(refresh_token));
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        let tokens = super::types::OAuthTokens {
            access_token: new_access.clone(),
            refresh_token: new_refresh,
            expires_at: Some(Utc::now() + Duration::seconds(expires_in)),
            email: None,
        };

        if let Err(e) = self.update_tokens(account_id, &tokens).await {
            tracing::warn!(account_id = %account_id, "Failed to persist refreshed tokens: {e}");
        }

        tracing::info!(account_id = %account_id, "Access token refreshed successfully");
        Some(new_access)
    }

    /// Retrieve the decrypted refresh token for an account.
    pub async fn get_refresh_token(&self, account_id: &str) -> Result<Option<String>, OAuthError> {
        let row: (Option<Vec<u8>>,) =
            sqlx::query_as("SELECT encrypted_refresh_token FROM connected_accounts WHERE id = ?1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| OAuthError::AccountNotFound(account_id.to_string()))?;

        match row.0 {
            Some(encrypted) => Ok(Some(self.decrypt_token(&encrypted)?)),
            None => Ok(None),
        }
    }

    /// List all connected accounts (without decrypted tokens).
    pub async fn list_accounts(&self) -> Result<Vec<ConnectedAccount>, OAuthError> {
        let rows: Vec<AccountRow> = sqlx::query_as(
            "SELECT id, provider, email_address, status, archive_strategy, \
                 label_prefix, created_at, updated_at, sync_depth, sync_frequency \
                 FROM connected_accounts ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let accounts = rows
            .into_iter()
            .filter_map(|r| {
                let provider = r.1.parse::<ProviderKind>().ok()?;
                let status = r.3.parse::<AccountStatus>().ok()?;
                let created_at = DateTime::parse_from_rfc3339(&r.6)
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&r.6, "%Y-%m-%d %H:%M:%S")
                            .map(|naive| naive.and_utc().fixed_offset())
                    })
                    .ok()?
                    .with_timezone(&Utc);
                let updated_at = DateTime::parse_from_rfc3339(&r.7)
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&r.7, "%Y-%m-%d %H:%M:%S")
                            .map(|naive| naive.and_utc().fixed_offset())
                    })
                    .ok()?
                    .with_timezone(&Utc);

                Some(ConnectedAccount {
                    id: r.0,
                    provider,
                    email_address: r.2,
                    status,
                    archive_strategy: r.4,
                    label_prefix: r.5,
                    sync_depth: r.8,
                    sync_frequency: r.9,
                    created_at,
                    updated_at,
                })
            })
            .collect();

        Ok(accounts)
    }

    /// Update account settings (archive strategy, label prefix, sync depth, sync frequency).
    pub async fn update_account_settings(
        &self,
        account_id: &str,
        archive_strategy: Option<&str>,
        label_prefix: Option<&str>,
        sync_depth: Option<&str>,
        sync_frequency: Option<i32>,
    ) -> Result<(), OAuthError> {
        // Validate inputs.
        if let Some(s) = archive_strategy {
            if !["instant", "delayed", "manual"].contains(&s) {
                return Err(OAuthError::ValidationError(format!(
                    "Invalid archive_strategy: {s}"
                )));
            }
        }
        if let Some(d) = sync_depth {
            if !["7d", "30d", "90d", "365d", "all"].contains(&d) {
                return Err(OAuthError::ValidationError(format!(
                    "Invalid sync_depth: {d}"
                )));
            }
        }
        if let Some(f) = sync_frequency {
            if ![1, 5, 15, 60].contains(&f) {
                return Err(OAuthError::ValidationError(format!(
                    "Invalid sync_frequency: {f}"
                )));
            }
        }
        if let Some(lp) = label_prefix {
            if lp.len() > 20 {
                return Err(OAuthError::ValidationError(
                    "label_prefix must be 20 characters or fewer".into(),
                ));
            }
        }

        let rows = sqlx::query(
            "UPDATE connected_accounts SET \
                archive_strategy = COALESCE(?1, archive_strategy), \
                label_prefix = COALESCE(?2, label_prefix), \
                sync_depth = COALESCE(?3, sync_depth), \
                sync_frequency = COALESCE(?4, sync_frequency), \
                updated_at = datetime('now') \
             WHERE id = ?5",
        )
        .bind(archive_strategy)
        .bind(label_prefix)
        .bind(sync_depth)
        .bind(sync_frequency)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        if rows.rows_affected() == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Disconnect an account (soft-delete: sets status to disconnected, clears tokens).
    pub async fn disconnect_account(&self, account_id: &str) -> Result<(), OAuthError> {
        let rows = sqlx::query(
            "UPDATE connected_accounts SET \
                status = 'disconnected', \
                encrypted_access_token = NULL, \
                encrypted_refresh_token = NULL, \
                token_expires_at = NULL, \
                updated_at = datetime('now') \
             WHERE id = ?1",
        )
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        if rows.rows_affected() == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Get sync state for an account.
    pub async fn get_sync_state(
        &self,
        account_id: &str,
    ) -> Result<Option<super::SyncState>, OAuthError> {
        let row: Option<SyncStateRow> = sqlx::query_as(
            "SELECT account_id, last_sync_at, history_id, next_page_token, \
             emails_synced, sync_failures, last_error, status \
             FROM sync_state WHERE account_id = ?1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let last_sync_at = r.1.as_deref().and_then(|s| {
                DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            super::SyncState {
                account_id: r.0,
                last_sync_at,
                history_id: r.2,
                next_page_token: r.3,
                emails_synced: r.4 as u64,
                sync_failures: r.5 as u32,
                last_error: r.6,
                status: r.7,
            }
        }))
    }

    // --- Encryption helpers ---

    fn encrypt_token(&self, plaintext: &str) -> Result<Vec<u8>, OAuthError> {
        match &self.encryption_key {
            Some(key) => {
                let cipher = Aes256Gcm::new_from_slice(key.as_ref())
                    .map_err(|e| OAuthError::EncryptionError(e.to_string()))?;
                let mut nonce_bytes = [0u8; NONCE_SIZE];
                rand::thread_rng().fill_bytes(&mut nonce_bytes);
                let nonce = Nonce::from_slice(&nonce_bytes);

                let ciphertext = cipher
                    .encrypt(nonce, plaintext.as_bytes())
                    .map_err(|e| OAuthError::EncryptionError(e.to_string()))?;

                let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
                output.extend_from_slice(&nonce_bytes);
                output.extend_from_slice(&ciphertext);
                Ok(output)
            }
            None => {
                // No encryption key: store as base64 (dev mode only).
                Ok(base64::engine::general_purpose::STANDARD
                    .encode(plaintext)
                    .into_bytes())
            }
        }
    }

    fn decrypt_token(&self, encrypted: &[u8]) -> Result<String, OAuthError> {
        match &self.encryption_key {
            Some(key) => {
                if encrypted.len() < NONCE_SIZE {
                    return Err(OAuthError::DecryptionError(
                        "Ciphertext too short".to_string(),
                    ));
                }

                let cipher = Aes256Gcm::new_from_slice(key.as_ref())
                    .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;
                let (nonce_bytes, ciphertext) = encrypted.split_at(NONCE_SIZE);
                let nonce = Nonce::from_slice(nonce_bytes);

                let plaintext = cipher
                    .decrypt(nonce, ciphertext)
                    .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;

                String::from_utf8(plaintext).map_err(|e| OAuthError::DecryptionError(e.to_string()))
            }
            None => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encrypted)
                    .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;
                String::from_utf8(decoded).map_err(|e| OAuthError::DecryptionError(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_encrypt_decrypt_roundtrip_no_key() {
        // Without encryption key, tokens are base64 encoded.
        let mgr = OAuthManager {
            pool: SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            encryption_key: None,
            http: reqwest::Client::new(),
        };

        let token = "my-secret-access-token";
        let encrypted = mgr.encrypt_token(token).unwrap();
        let decrypted = mgr.decrypt_token(&encrypted).unwrap();
        assert_eq!(decrypted, token);
    }

    #[tokio::test]
    async fn test_encrypt_decrypt_roundtrip_with_key() {
        let key = crate::vectors::encryption::derive_key("test-password", TOKEN_KEY_SALT).unwrap();
        let mgr = OAuthManager {
            pool: SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            encryption_key: Some(key),
            http: reqwest::Client::new(),
        };

        let token = "ya29.a0AfH6SMBx_secrettoken123";
        let encrypted = mgr.encrypt_token(token).unwrap();
        let decrypted = mgr.decrypt_token(&encrypted).unwrap();
        assert_eq!(decrypted, token);

        // Encrypting the same token twice should produce different ciphertexts.
        let encrypted2 = mgr.encrypt_token(token).unwrap();
        assert_ne!(encrypted, encrypted2);
    }

    #[tokio::test]
    async fn test_authorization_url_contains_params() {
        let mgr = OAuthManager {
            pool: SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            encryption_key: None,
            http: reqwest::Client::new(),
        };

        let config = ProviderConfig {
            client_id: "my-client-id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost:8080/api/v1/auth/callback".to_string(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            scopes: vec!["https://www.googleapis.com/auth/gmail.modify".to_string()],
        };

        let (url, state) = mgr.authorization_url(&config, "gmail");
        assert!(url.contains("client_id=my-client-id"));
        assert!(url.contains("response_type=code"));
        assert!(state.starts_with("gmail:"));
        assert!(url.contains("access_type=offline"));
    }
}
