//! OAuth flow management (DDD-005: OAuthManager domain service).
//!
//! Handles authorization URL generation, PKCE code verifier/challenge pairs,
//! token exchange, and encrypted token storage using the existing AES-256-GCM
//! encryption infrastructure.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use zeroize::Zeroizing;

use super::types::{AccountStatus, ConnectedAccount, OAuthTokens, ProviderConfig, ProviderKind};
use crate::db::entities::{connected_accounts as accounts, sync_state};
use crate::db::Database;

/// Fixed salt for token encryption key derivation (separate from vector encryption).
const TOKEN_KEY_SALT: &[u8] = b"emailibrium-token-encryption-v1";
const NONCE_SIZE: usize = 12;

/// The `'YYYY-MM-DD HH:MM:SS'` UTC string this table's TEXT timestamp columns hold.
///
/// `created_at`/`updated_at` are TEXT, not TIMESTAMPTZ, in both dialects (ADR-035 §2.5), so
/// the format is the application's to own. The two hand-written per-backend writes this
/// replaces — SQLite `datetime('now')` and PostgreSQL
/// `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')` — both produced exactly this,
/// so one Rust-side format string is the single code path for both (ADR-036).
fn now_text() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

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

/// Row tuple for the sync_state query. `emails_synced` is i32, not i64, matching the
/// actual INTEGER/INT4 column (ADR-035's note on real-4-byte-int columns).
type SyncStateRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    i32,
    Option<String>,
    String,
);

/// Row tuple for the IMAP config query: (imap_host, imap_port, imap_encryption,
/// smtp_host, smtp_port, encrypted_access_token, email_address). `imap_port`/
/// `smtp_port` are i32, not i64, matching the actual INTEGER/INT4 columns.
type ImapConfigRow = (
    Option<String>,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<Vec<u8>>,
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
    DatabaseError(#[from] sea_orm::DbErr),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Manages OAuth flows, token storage, and account persistence.
///
/// Persistence is single-code-path SeaORM (ADR-036): the `connected_accounts` and `sync_state`
/// entities own per-backend encode/decode, upserts go through `OnConflict`, and the former
/// `match Database::Sqlite/Postgres` arms with their hand-written per-backend SQL pairs are
/// gone — the same bodies run against SQLite and PostgreSQL.
pub struct OAuthManager {
    conn: DatabaseConnection,
    encryption_key: Option<Zeroizing<[u8; 32]>>,
    http: reqwest::Client,
}

impl OAuthManager {
    /// Create a new OAuthManager.
    ///
    /// If `master_password` is provided, tokens are encrypted at rest using
    /// AES-256-GCM with an Argon2id-derived key. If `None`, tokens are stored
    /// as plaintext (development only).
    ///
    /// `db.sea_orm()` only wraps the pool this `Database` already holds — no second pool, and
    /// no connection is opened here, so a lazily-connected pool stays lazy.
    pub fn new(db: Database, master_password: Option<&str>) -> Self {
        let encryption_key = master_password
            .and_then(|pw| crate::vectors::encryption::derive_key(pw, TOKEN_KEY_SALT).ok());

        Self {
            conn: db.sea_orm(),
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
        // `select_only()` + explicit columns, not a whole-entity find: these queries project
        // exactly the columns the old SQL named, so a column the entity declares but a given
        // deployment's schema hasn't reached yet can't turn a narrow read into a decode error.
        let id: Option<String> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::Id)
            .filter(accounts::Column::EmailAddress.eq(email))
            .into_tuple()
            .one(&self.conn)
            .await?;
        Ok(id)
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

        // One upsert for both backends: the conflict target is `email_address` (its UNIQUE
        // index), the DO UPDATE re-sets exactly the columns the old per-backend SQL pairs
        // re-set, and `updated_at` is an explicit conflict-side value so the INSERT path still
        // takes the DDL default, as before. `token_expires_at` stays RFC3339 — that is what
        // this file writes and parses; only the `%Y-%m-%d %H:%M:%S` columns are `now_text()`.
        accounts::Entity::insert(accounts::ActiveModel {
            id: Set(id.to_owned()),
            provider: Set(provider.as_str().to_owned()),
            email_address: Set(email.to_owned()),
            encrypted_access_token: Set(Some(enc_access)),
            encrypted_refresh_token: Set(enc_refresh),
            token_expires_at: Set(expires_at),
            status: Set("connected".to_owned()),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(accounts::Column::EmailAddress)
                .update_columns([
                    accounts::Column::EncryptedAccessToken,
                    accounts::Column::EncryptedRefreshToken,
                    accounts::Column::TokenExpiresAt,
                    accounts::Column::Status,
                ])
                .value(accounts::Column::UpdatedAt, Expr::value(now_text()))
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await?;

        self.ensure_sync_state(id).await
    }

    /// Create the account's `sync_state` row if it doesn't already have one.
    ///
    /// SQLite's `INSERT OR IGNORE` and PostgreSQL's `ON CONFLICT (account_id) DO NOTHING`
    /// collapse to one `OnConflict::do_nothing()`. `exec_without_returning` reports zero rows
    /// rather than raising `DbErr::RecordNotInserted`, so an existing row stays a silent no-op
    /// as both arms had it. This narrows one pre-existing divergence: `INSERT OR IGNORE`
    /// swallowed *any* constraint violation on SQLite, `DO NOTHING` only the primary-key
    /// conflict — the single path keeps the (stricter) PostgreSQL semantics the two arms
    /// already disagreed on. Only the PK conflict is reachable here: the caller has just
    /// written the parent `connected_accounts` row, and every other column is DEFAULT-filled.
    async fn ensure_sync_state(&self, account_id: &str) -> Result<(), OAuthError> {
        sync_state::Entity::insert(sync_state::ActiveModel {
            account_id: Set(account_id.to_owned()),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(sync_state::Column::AccountId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await?;
        Ok(())
    }

    /// Persist an IMAP-connected account.
    ///
    /// IMAP uses basic auth (app password) rather than OAuth tokens, so the
    /// password is encrypted into `encrypted_access_token` (reusing the OAuth
    /// token column) and the connection details are stored in the IMAP columns.
    /// `encrypted_refresh_token` / `token_expires_at` are left NULL.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_imap_account(
        &self,
        id: &str,
        email: &str,
        password: &str,
        imap_host: &str,
        imap_port: u16,
        encryption: super::imap::ImapEncryption,
        smtp_host: &str,
        smtp_port: u16,
    ) -> Result<(), OAuthError> {
        let enc_password = self.encrypt_token(password)?;
        let imap_port = imap_port as i32;
        let smtp_port = smtp_port as i32;

        // `encrypted_refresh_token`/`token_expires_at` are written as explicit NULLs rather
        // than left out of the INSERT (they have no DDL default, so the inserted row is
        // identical either way) — that is what lets the DO UPDATE clear them via `excluded`,
        // matching the old SQL's literal `= NULL` on the conflict path.
        accounts::Entity::insert(accounts::ActiveModel {
            id: Set(id.to_owned()),
            provider: Set("imap".to_owned()),
            email_address: Set(email.to_owned()),
            encrypted_access_token: Set(Some(enc_password)),
            encrypted_refresh_token: Set(None),
            token_expires_at: Set(None),
            status: Set("connected".to_owned()),
            imap_host: Set(Some(imap_host.to_owned())),
            imap_port: Set(Some(imap_port)),
            imap_encryption: Set(Some(encryption.as_str().to_owned())),
            smtp_host: Set(Some(smtp_host.to_owned())),
            smtp_port: Set(Some(smtp_port)),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(accounts::Column::EmailAddress)
                .update_columns([
                    accounts::Column::Provider,
                    accounts::Column::EncryptedAccessToken,
                    accounts::Column::EncryptedRefreshToken,
                    accounts::Column::TokenExpiresAt,
                    accounts::Column::Status,
                    accounts::Column::ImapHost,
                    accounts::Column::ImapPort,
                    accounts::Column::ImapEncryption,
                    accounts::Column::SmtpHost,
                    accounts::Column::SmtpPort,
                ])
                .value(accounts::Column::UpdatedAt, Expr::value(now_text()))
                .to_owned(),
        )
        .exec_without_returning(&self.conn)
        .await?;

        self.ensure_sync_state(id).await
    }

    /// Load the stored IMAP connection config for an account, decrypting the
    /// app password. Returns a `ValidationError` if the account is not an IMAP
    /// account (i.e., `imap_host` is NULL).
    pub async fn load_imap_config(
        &self,
        account_id: &str,
    ) -> Result<super::imap::ImapConfig, OAuthError> {
        let row: Option<ImapConfigRow> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::ImapHost)
            .column(accounts::Column::ImapPort)
            .column(accounts::Column::ImapEncryption)
            .column(accounts::Column::SmtpHost)
            .column(accounts::Column::SmtpPort)
            .column(accounts::Column::EncryptedAccessToken)
            .column(accounts::Column::EmailAddress)
            .filter(accounts::Column::Id.eq(account_id))
            .into_tuple()
            .one(&self.conn)
            .await?;
        let row = row.ok_or_else(|| OAuthError::AccountNotFound(account_id.to_string()))?;

        let host = row.0.ok_or_else(|| {
            OAuthError::ValidationError(format!("Account {account_id} is not an IMAP account"))
        })?;
        let enc_password = row.5.ok_or_else(|| {
            OAuthError::ValidationError(format!("IMAP account {account_id} has no stored password"))
        })?;
        let password = self.decrypt_token(&enc_password)?;
        let email_address = row.6;
        let encryption = row
            .2
            .as_deref()
            .map(super::imap::ImapEncryption::from_stored_str)
            .unwrap_or(super::imap::ImapEncryption::Ssl);

        Ok(super::imap::ImapConfig {
            host,
            port: row.1.unwrap_or(993) as u16,
            encryption,
            username: email_address,
            password,
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: row.3,
            smtp_port: row.4.unwrap_or(587) as u16,
            pinned_addr: None,
        })
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

        // `encrypted_refresh_token = COALESCE(?, encrypted_refresh_token)` kept the stored
        // token when the caller had none; omitting the column when `enc_refresh` is `None` is
        // the same write (a self-assignment vs. no assignment), and leaves the set of matched
        // rows — hence `rows_affected` and the AccountNotFound signal — unchanged.
        let mut update = accounts::Entity::update_many()
            .col_expr(
                accounts::Column::EncryptedAccessToken,
                Expr::value(enc_access),
            )
            .col_expr(accounts::Column::TokenExpiresAt, Expr::value(expires_at))
            .col_expr(accounts::Column::UpdatedAt, Expr::value(now_text()));
        if let Some(refresh) = enc_refresh {
            update = update.col_expr(
                accounts::Column::EncryptedRefreshToken,
                Expr::value(refresh),
            );
        }
        let affected = update
            .filter(accounts::Column::Id.eq(account_id))
            .exec(&self.conn)
            .await?
            .rows_affected;

        if affected == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Retrieve the decrypted access token for an account, auto-refreshing if expired.
    pub async fn get_access_token(&self, account_id: &str) -> Result<String, OAuthError> {
        // `Vec<u8>`, not `Option<Vec<u8>>`: a disconnected account (tokens cleared to NULL)
        // fails to decode here rather than reporting AccountNotFound — pre-existing behavior,
        // preserved deliberately.
        let row: Option<(Vec<u8>, Option<String>)> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::EncryptedAccessToken)
            .column(accounts::Column::TokenExpiresAt)
            .filter(accounts::Column::Id.eq(account_id))
            .into_tuple()
            .one(&self.conn)
            .await?;
        let row = row.ok_or_else(|| OAuthError::AccountNotFound(account_id.to_string()))?;

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

        let provider_row: Option<String> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::Provider)
            .filter(accounts::Column::Id.eq(account_id))
            .into_tuple()
            .one(&self.conn)
            .await
            .ok()?;

        let provider_str = provider_row?;
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
        // Outer `None` = no such account; inner `None` = the account has no refresh token.
        let row: Option<Option<Vec<u8>>> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::EncryptedRefreshToken)
            .filter(accounts::Column::Id.eq(account_id))
            .into_tuple()
            .one(&self.conn)
            .await?;
        let row = row.ok_or_else(|| OAuthError::AccountNotFound(account_id.to_string()))?;

        match row {
            Some(encrypted) => Ok(Some(self.decrypt_token(&encrypted)?)),
            None => Ok(None),
        }
    }

    /// List all connected accounts (without decrypted tokens).
    pub async fn list_accounts(&self) -> Result<Vec<ConnectedAccount>, OAuthError> {
        let rows: Vec<AccountRow> = accounts::Entity::find()
            .select_only()
            .column(accounts::Column::Id)
            .column(accounts::Column::Provider)
            .column(accounts::Column::EmailAddress)
            .column(accounts::Column::Status)
            .column(accounts::Column::ArchiveStrategy)
            .column(accounts::Column::LabelPrefix)
            .column(accounts::Column::CreatedAt)
            .column(accounts::Column::UpdatedAt)
            .column(accounts::Column::SyncDepth)
            .column(accounts::Column::SyncFrequency)
            .order_by_desc(accounts::Column::CreatedAt)
            .into_tuple()
            .all(&self.conn)
            .await?;

        // Two accepted timestamp formats, and an account that parses as neither is dropped
        // rather than failing the whole listing: `created_at`/`updated_at` are TEXT, and rows
        // carry whichever format wrote them — the DDL default's `'%Y-%m-%d %H:%M:%S'` or an
        // RFC3339 string from an earlier writer. Defensive by design; preserved verbatim.
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
            if !(60..=86400).contains(&f) {
                return Err(OAuthError::ValidationError(format!(
                    "sync_frequency must be between 60 and 86400 seconds, got: {f}"
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

        // Each `COALESCE(?, col)` was a per-column no-op when the caller passed NULL, so
        // setting only the `Some` columns is the same write. `updated_at` is set
        // unconditionally, as before — an all-`None` call still touches the row.
        let mut update = accounts::Entity::update_many()
            .col_expr(accounts::Column::UpdatedAt, Expr::value(now_text()));
        if let Some(v) = archive_strategy {
            update = update.col_expr(accounts::Column::ArchiveStrategy, Expr::value(v));
        }
        if let Some(v) = label_prefix {
            update = update.col_expr(accounts::Column::LabelPrefix, Expr::value(v));
        }
        if let Some(v) = sync_depth {
            update = update.col_expr(accounts::Column::SyncDepth, Expr::value(v));
        }
        if let Some(v) = sync_frequency {
            update = update.col_expr(accounts::Column::SyncFrequency, Expr::value(v));
        }
        let affected = update
            .filter(accounts::Column::Id.eq(account_id))
            .exec(&self.conn)
            .await?
            .rows_affected;

        if affected == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Disconnect an account (soft-delete: sets status to disconnected, clears tokens).
    pub async fn disconnect_account(&self, account_id: &str) -> Result<(), OAuthError> {
        let affected = accounts::Entity::update_many()
            .col_expr(accounts::Column::Status, Expr::value("disconnected"))
            .col_expr(
                accounts::Column::EncryptedAccessToken,
                Expr::value(Option::<Vec<u8>>::None),
            )
            .col_expr(
                accounts::Column::EncryptedRefreshToken,
                Expr::value(Option::<Vec<u8>>::None),
            )
            .col_expr(
                accounts::Column::TokenExpiresAt,
                Expr::value(Option::<String>::None),
            )
            .col_expr(accounts::Column::UpdatedAt, Expr::value(now_text()))
            .filter(accounts::Column::Id.eq(account_id))
            .exec(&self.conn)
            .await?
            .rows_affected;

        if affected == 0 {
            return Err(OAuthError::AccountNotFound(account_id.to_string()));
        }
        Ok(())
    }

    /// Get sync state for an account.
    pub async fn get_sync_state(
        &self,
        account_id: &str,
    ) -> Result<Option<super::SyncState>, OAuthError> {
        let row: Option<SyncStateRow> = sync_state::Entity::find()
            .select_only()
            .column(sync_state::Column::AccountId)
            .column(sync_state::Column::LastSyncAt)
            .column(sync_state::Column::HistoryId)
            .column(sync_state::Column::NextPageToken)
            .column(sync_state::Column::EmailsSynced)
            .column(sync_state::Column::SyncFailures)
            .column(sync_state::Column::LastError)
            .column(sync_state::Column::Status)
            .filter(sync_state::Column::AccountId.eq(account_id))
            .into_tuple()
            .one(&self.conn)
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
                rand::rng().fill_bytes(&mut nonce_bytes);
                let nonce = Nonce::try_from(nonce_bytes.as_slice())
                    .map_err(|e| OAuthError::EncryptionError(e.to_string()))?;

                let ciphertext = cipher
                    .encrypt(&nonce, plaintext.as_bytes())
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
                let nonce = Nonce::try_from(nonce_bytes)
                    .map_err(|e| OAuthError::DecryptionError(e.to_string()))?;

                let plaintext = cipher
                    .decrypt(&nonce, ciphertext)
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
    use sqlx::SqlitePool;

    #[tokio::test]
    async fn test_encrypt_decrypt_roundtrip_no_key() {
        // Without encryption key, tokens are base64 encoded.
        let mgr = OAuthManager {
            conn: Database::Sqlite(SqlitePool::connect_lazy("sqlite::memory:").unwrap()).sea_orm(),
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
            conn: Database::Sqlite(SqlitePool::connect_lazy("sqlite::memory:").unwrap()).sea_orm(),
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
    async fn test_save_and_load_imap_account_roundtrip() {
        // Single-connection in-memory pool so the schema persists across queries.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Apply migrations as raw multi-statement scripts.
        sqlx::raw_sql(include_str!("../../migrations/sqlite/004_accounts.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "ALTER TABLE connected_accounts ADD COLUMN sync_depth TEXT NOT NULL DEFAULT '30d';",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/sqlite/028_imap_accounts.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let key = crate::vectors::encryption::derive_key("test-password", TOKEN_KEY_SALT).unwrap();
        let mgr = OAuthManager {
            conn: Database::Sqlite(pool).sea_orm(),
            encryption_key: Some(key),
            http: reqwest::Client::new(),
        };

        mgr.save_imap_account(
            "imap-acct-1",
            "user@gmail.com",
            "app-password-1234",
            "imap.gmail.com",
            993,
            crate::email::imap::ImapEncryption::StartTls,
            "smtp.gmail.com",
            465,
        )
        .await
        .unwrap();

        // Account is findable by email.
        let found = mgr
            .find_account_id_by_email("user@gmail.com")
            .await
            .unwrap();
        assert_eq!(found.as_deref(), Some("imap-acct-1"));

        // Loaded config matches and the password decrypts.
        let cfg = mgr.load_imap_config("imap-acct-1").await.unwrap();
        assert_eq!(cfg.host, "imap.gmail.com");
        assert_eq!(cfg.port, 993);
        // Encryption mode round-trips through the DB (StartTls -> "starttls").
        assert_eq!(cfg.encryption, crate::email::imap::ImapEncryption::StartTls);
        assert_eq!(cfg.username, "user@gmail.com");
        assert_eq!(cfg.password, "app-password-1234");
        assert_eq!(cfg.smtp_host.as_deref(), Some("smtp.gmail.com"));
        assert_eq!(cfg.smtp_port, 465);

        // Loading a non-IMAP / missing account errors cleanly.
        let err = mgr.load_imap_config("does-not-exist").await;
        assert!(matches!(err, Err(OAuthError::AccountNotFound(_))));
    }

    /// Two-account scoping pin (the phase-2 mutation court proved this class of
    /// filter is otherwise unasserted): settings updates keyed by account id
    /// must not leak onto another account's row.
    #[tokio::test]
    async fn test_update_account_settings_scopes_to_the_target_account() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/sqlite/004_accounts.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "ALTER TABLE connected_accounts ADD COLUMN sync_depth TEXT NOT NULL DEFAULT '30d';",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(
            "ALTER TABLE connected_accounts ADD COLUMN sync_frequency INTEGER NOT NULL DEFAULT 5;",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/sqlite/028_imap_accounts.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let key = crate::vectors::encryption::derive_key("test-password", TOKEN_KEY_SALT).unwrap();
        let mgr = OAuthManager {
            conn: Database::Sqlite(pool).sea_orm(),
            encryption_key: Some(key),
            http: reqwest::Client::new(),
        };

        for (id, email) in [("acct-a", "a@example.com"), ("acct-b", "b@example.com")] {
            mgr.save_imap_account(
                id,
                email,
                "pw",
                "imap.example.com",
                993,
                crate::email::imap::ImapEncryption::Ssl,
                "smtp.example.com",
                465,
            )
            .await
            .unwrap();
        }

        mgr.update_account_settings(
            "acct-a",
            Some("instant"),
            Some("X/"),
            Some("90d"),
            Some(120),
        )
        .await
        .unwrap();

        let accounts = mgr.list_accounts().await.unwrap();
        let a = accounts.iter().find(|x| x.id == "acct-a").expect("acct-a");
        let b = accounts.iter().find(|x| x.id == "acct-b").expect("acct-b");
        assert_eq!(a.archive_strategy, "instant");
        assert_eq!(a.label_prefix, "X/");
        // The bystander keeps every default the save wrote.
        assert_eq!(b.archive_strategy, "delayed");
        assert_eq!(b.label_prefix, "EM/");
    }

    #[tokio::test]
    async fn test_authorization_url_contains_params() {
        let mgr = OAuthManager {
            conn: Database::Sqlite(SqlitePool::connect_lazy("sqlite::memory:").unwrap()).sea_orm(),
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
