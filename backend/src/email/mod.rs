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

pub mod archive;
pub mod gmail;
pub mod health;
pub mod labels;
pub mod oauth;
pub mod outlook;
pub mod provider;
pub mod sync;
pub mod types;

pub use provider::EmailProvider;
pub use types::{
    AccountStatus, ConnectedAccount, EmailMessage, ListParams, OAuthTokens, ProviderKind, SyncState,
};
