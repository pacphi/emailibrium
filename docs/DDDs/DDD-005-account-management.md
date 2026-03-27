# DDD-005: Account Management Domain

| Field   | Value              |
| ------- | ------------------ |
| Status  | Accepted           |
| Date    | 2026-03-23         |
| Type    | Supporting Domain  |
| Context | Account Management |

## Overview

The Account Management bounded context handles email account connections, OAuth flows, credential storage, synchronization state, archive strategies, and label management. It abstracts the differences between email providers (Gmail, Outlook, IMAP, POP3) behind a unified interface and publishes account lifecycle events consumed by the Ingestion context.

## Aggregates

### 1. AccountAggregate

Manages email account connections and credentials.

**Root Entity: EmailAccount**

| Field            | Type               | Description                               |
| ---------------- | ------------------ | ----------------------------------------- |
| id               | AccountId          | Unique account identifier                 |
| provider         | Provider           | Gmail, Outlook, Imap, or Pop3             |
| email_address    | EmailAddress       | The account's email address               |
| auth_type        | AuthType           | OAuth2 or Credentials                     |
| sync_config      | SyncConfig         | Synchronization configuration             |
| archive_strategy | ArchiveStrategy    | How archived emails are handled           |
| status           | AccountStatus      | Connected, Disconnected, Error, Suspended |
| last_sync        | Option\<DateTime\> | Timestamp of last successful sync         |
| created_at       | DateTime           | When the account was connected            |

**Invariants:**

- Email address must be unique per user (no duplicate account connections).
- OAuth2 accounts must have a valid refresh token; expired access tokens are refreshed automatically.
- Credential-based accounts (IMAP/POP3) must have encrypted passwords (never stored in plaintext).
- Account status transitions: Connected --> Disconnected, Connected --> Error, Error --> Connected (on successful reconnect), Suspended --> Connected (on reauthorization).
- Deleting an account cascades: all sync state, tokens, and provider-specific metadata are purged. Emails and embeddings are NOT deleted (they belong to other contexts).

**Commands:**

- `ConnectAccount { provider, auth_type, email_address }` -- initiates account connection
- `DisconnectAccount { account_id, reason }` -- disconnects an account
- `UpdateSyncConfig { account_id, sync_config }` -- changes sync settings
- `ChangeArchiveStrategy { account_id, strategy }` -- changes archive behavior
- `RefreshToken { account_id }` -- forces OAuth token refresh
- `ReconnectAccount { account_id }` -- attempts to reconnect an errored account

### 2. SyncAggregate

Manages per-account synchronization state.

**Root Entity: SyncState**

| Field           | Type               | Description                                                              |
| --------------- | ------------------ | ------------------------------------------------------------------------ |
| account_id      | AccountId          | The account this sync state belongs to                                   |
| last_history_id | Option\<String\>   | Provider-specific history/delta token (Gmail historyId, Graph deltaLink) |
| last_sync_at    | Option\<DateTime\> | Timestamp of last sync                                                   |
| sync_depth      | SyncDepth          | How far back to sync                                                     |
| emails_synced   | u64                | Total emails synced for this account                                     |
| sync_failures   | u32                | Consecutive sync failure count                                           |
| last_error      | Option\<String\>   | Last sync error message                                                  |

**Invariants:**

- Sync state is per-account; each account has exactly one SyncState.
- Consecutive sync failures > 5 triggers account status change to Error.
- Incremental sync uses `last_history_id`; full sync ignores it and fetches all emails within `sync_depth`.
- Sync depth cannot be reduced below the current sync watermark without triggering a re-sync.

**Commands:**

- `StartSync { account_id, sync_type }` -- begins a full or incremental sync
- `CompleteSync { account_id, new_history_id, new_emails_count }` -- records sync completion
- `RecordSyncFailure { account_id, error }` -- records a sync failure
- `ResetSyncState { account_id }` -- clears sync state for a full re-sync

## Domain Events

| Event                  | Fields                                   | Published When                             |
| ---------------------- | ---------------------------------------- | ------------------------------------------ |
| AccountConnected       | account_id, provider, email_address      | Account successfully connected             |
| AccountDisconnected    | account_id, reason                       | Account disconnected by user or system     |
| SyncStarted            | account_id, sync_type (full/incremental) | Sync begins for an account                 |
| SyncCompleted          | account_id, new_emails, sync_duration    | Sync finishes successfully                 |
| SyncFailed             | account_id, error, failure_count         | A sync attempt fails                       |
| ArchiveStrategyChanged | account_id, old_strategy, new_strategy   | User changes archive behavior              |
| TokenRefreshed         | account_id, provider                     | OAuth token successfully refreshed         |
| TokenExpired           | account_id, provider                     | OAuth token expired and refresh failed     |
| AccountSuspended       | account_id, reason                       | Account suspended due to repeated failures |

### Event Consumers

| Event            | Consumed By           | Purpose                                      |
| ---------------- | --------------------- | -------------------------------------------- |
| AccountConnected | Ingestion             | Triggers initial full ingestion              |
| SyncCompleted    | Ingestion             | Triggers incremental ingestion of new emails |
| TokenExpired     | Monitoring / Alerting | Alerts user to reauthorize                   |
| AccountSuspended | Monitoring / Alerting | Alerts user to account issues                |

## Value Objects

### Provider

```rust
enum Provider {
    Gmail,    -- Google Gmail via Gmail API
    Outlook,  -- Microsoft Outlook/365 via Graph API
    Imap,     -- Generic IMAP server
    Pop3,     -- Generic POP3 server
}
```

### AuthType

```rust
enum AuthType {
    OAuth2 {
        access_token: EncryptedString,
        refresh_token: EncryptedString,
        expires_at: DateTime,
        scopes: Vec<String>,
    },
    Credentials {
        username: String,
        encrypted_password: EncryptedString,
        imap_server: HostPort,
        smtp_server: Option<HostPort>,
    },
}
```

### ArchiveStrategy

```rust
enum ArchiveStrategy {
    Instant,                    -- Archive immediately on classification
    Delayed { delay_secs: u32 }, -- Archive after a delay (default: 300s / 5 min)
    Manual,                     -- Never auto-archive; user archives manually
}
```

### SyncDepth

```rust
enum SyncDepth {
    All,                -- Sync all available emails
    LastNDays(u32),     -- Sync only the last N days (default: 90)
}
```

### LabelPrefix

```rust
LabelPrefix(String) -- default "EM/"
```

All Emailibrium-managed labels/categories in the email provider are prefixed with this string to avoid colliding with user-created labels.

### SyncConfig

| Field              | Type          | Description                                            |
| ------------------ | ------------- | ------------------------------------------------------ |
| sync_depth         | SyncDepth     | How far back to sync                                   |
| auto_sync_interval | Duration      | How often to check for new emails (default: 5 minutes) |
| batch_size         | u32           | Number of emails to fetch per API call                 |
| exclude_labels     | Vec\<String\> | Provider labels to exclude from sync                   |

### AccountStatus

```rust
enum AccountStatus {
    Connected,     -- Active and syncing
    Disconnected,  -- User-initiated disconnect
    Error,         -- Sync failures exceeded threshold
    Suspended,     -- System-suspended (token expired, quota exceeded)
}
```

## Domain Services

### OAuthManager

Manages OAuth2 PKCE flows for Gmail and Outlook.

**Responsibilities:**

- Generates PKCE code challenge/verifier pairs.
- Handles OAuth2 authorization code exchange.
- Stores encrypted tokens (access + refresh).
- Automatically refreshes expired access tokens.
- Handles refresh token rotation (some providers rotate refresh tokens on use).
- Emits TokenRefreshed or TokenExpired events.

### ProviderSync

Per-provider synchronization logic.

**Provider Implementations:**

| Provider | API                 | Incremental Mechanism                      |
| -------- | ------------------- | ------------------------------------------ |
| Gmail    | Gmail API v1        | `history.list` with `historyId`            |
| Outlook  | Microsoft Graph API | `delta` query with `deltaLink`             |
| IMAP     | IMAP4rev1 protocol  | `UIDNEXT` / `HIGHESTMODSEQ`                |
| POP3     | POP3 protocol       | Message UID tracking (no true incremental) |

**Responsibilities:**

- Implements full and incremental sync per provider.
- Handles pagination (Gmail pageTokens, Graph @odata.nextLink).
- Respects rate limits (exponential backoff on 429 responses).
- Converts provider-specific email format to Emailibrium's RawEmail model.
- Emits SyncStarted, SyncCompleted, or SyncFailed events.

### ArchiveExecutor

Applies archive strategy per-account.

**Responsibilities:**

- Listens for classification events (from Email Intelligence context) to trigger archive.
- Applies the account's ArchiveStrategy (Instant, Delayed, Manual).
- Executes archive action in the email provider (Gmail: add label + remove INBOX; Outlook: move to Archive folder; IMAP: move to archive folder).
- Respects undo window for Delayed strategy.

### LabelManager

Creates and manages provider-specific labels.

**Responsibilities:**

- Creates Emailibrium labels in the email provider (prefixed with LabelPrefix).
- Maps Emailibrium categories to provider labels (Gmail labels, Outlook categories, IMAP folders).
- Handles label creation idempotently (no duplicates on re-sync).
- Cleans up orphaned labels when categories are removed.

### AccountHealthMonitor

Tracks account health metrics.

**Monitored Metrics:**

| Metric | Threshold | Action |
|--------|-----------|--------|
| Consecutive sync failures | > 5 | Suspend account, emit AccountSuspended |
| Token expiry approaching | < 1 hour | Preemptive refresh |
| API quota usage | > 80% | Reduce sync frequency |
| Sync latency | > 5 minutes | Log warning |

## Anti-Corruption Layers

All email providers are wrapped behind a unified trait to isolate the domain from provider-specific APIs.

### EmailProvider Trait

```rust
trait EmailProvider {
    fn list_emails(since: Option<DateTime>, page_token: Option<String>) -> Result<EmailPage>;
    fn get_email(provider_id: &str) -> Result<RawEmail>;
    fn get_history(history_id: &str) -> Result<HistoryPage>;
    fn apply_label(email_id: &str, label: &str) -> Result<()>;
    fn remove_label(email_id: &str, label: &str) -> Result<()>;
    fn move_to_folder(email_id: &str, folder: &str) -> Result<()>;
    fn create_label(name: &str) -> Result<LabelId>;
}
```

### Provider Implementations

| Implementation  | Wraps                                    |
| --------------- | ---------------------------------------- |
| GmailProvider   | Gmail API v1 (google-gmail1 crate)       |
| OutlookProvider | Microsoft Graph API (reqwest + graph-rs) |
| ImapProvider    | IMAP4rev1 (async-imap crate)             |
| Pop3Provider    | POP3 (async-pop3 crate)                  |

Each implementation translates provider-specific responses into the domain model, ensuring that no provider-specific types leak into the core domain.

## Context Map

### Downstream Consumers

| Context   | Relationship       | What Account Management Publishes                        |
| --------- | ------------------ | -------------------------------------------------------- |
| Ingestion | Published Language | AccountConnected, SyncCompleted events trigger ingestion |

### Independence

Account Management has no upstream dependencies on other Emailibrium bounded contexts. It is a self-contained context that only depends on external email provider APIs (wrapped in ACLs).

## Ubiquitous Language

| Term                   | Definition                                                                         |
| ---------------------- | ---------------------------------------------------------------------------------- |
| **Account**            | A connected email account (Gmail, Outlook, IMAP, POP3)                             |
| **Provider**           | The email service (Gmail, Outlook, etc.)                                           |
| **Sync**               | The process of fetching new emails from a provider                                 |
| **Incremental sync**   | Fetching only emails newer than the last sync point                                |
| **Full sync**          | Fetching all emails within the configured sync depth                               |
| **History ID**         | A provider-specific cursor for incremental sync (Gmail historyId, Graph deltaLink) |
| **Archive strategy**   | The policy for how classified emails are archived in the provider                  |
| **Label prefix**       | The string prefix ("EM/") for all Emailibrium-managed labels                       |
| **Token refresh**      | The OAuth2 process of obtaining a new access token using a refresh token           |
| **Account suspension** | System-initiated disconnect due to repeated failures or expired tokens             |

## Boundaries

- This context does NOT process email content or generate embeddings (that is Ingestion / Email Intelligence).
- This context does NOT manage search or user queries (that is Search).
- This context does NOT handle adaptive learning (that is Learning).
- This context DOES own account connections, OAuth flows, credential storage, sync state, archive execution, and label management.
- Credentials (tokens, passwords) never leave this context in plaintext. Other contexts receive only account IDs and metadata.
