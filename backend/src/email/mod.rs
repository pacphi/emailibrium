//! Email provider connectivity (DDD-005: Account Management).
//!
//! This module implements the Account Management bounded context, handling
//! OAuth flows, credential storage, and email provider abstraction for
//! Gmail (via Gmail REST API) and Outlook (via Microsoft Graph API).
//!
//! Services added for Audit Item #37:
//! - `sync` -- ProviderSync for email sync scheduling and delta detection
//! - `archive` -- ArchiveExecutor for archive strategy execution
//! - `labels` -- LabelManager for label CRUD via provider APIs
//! - `health` -- AccountHealthMonitor for connection health and token expiry
//!
//! Services added for R-02 / R-06:
//! - `checkpoint` -- ProcessingCheckpoint for crash recovery (R-06)
//! - `offline_queue` -- OfflineQueue for buffered operations (R-02)
//! - `conflict_resolution` -- ConflictResolver for sync conflicts (R-02)
//! - `sync_scheduler` -- SyncScheduler for background queue drain (R-02)

pub mod archive;
pub mod checkpoint;
pub mod conflict_resolution;
pub mod delta;
pub mod gmail;
pub mod health;
pub mod imap;
pub mod labels;
pub mod oauth;
pub mod offline_queue;
pub mod outlook;
pub mod provider;
pub mod sync;
pub mod sync_scheduler;
pub mod types;
pub mod unsubscribe;

pub use provider::EmailProvider;
pub use types::{
    AccountStatus, ConnectedAccount, EmailMessage, ListParams, OAuthTokens, ProviderKind, SyncState,
};
