//! Background email polling scheduler.
//!
//! Periodically checks each connected account for new mail using
//! provider-appropriate incremental sync, then triggers the ingestion
//! pipeline (embed → categorize → cluster → analyze).
//!
//! Respects per-account `sync_frequency` and uses exponential backoff
//! on transient failures to avoid hammering providers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::oauth::OAuthManager;
use super::types::AccountStatus;

// ---------------------------------------------------------------------------
// Per-account tracking state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AccountPollState {
    last_poll: Instant,
    backoff_secs: u64,
    in_progress: bool,
    consecutive_failures: u32,
}

impl Default for AccountPollState {
    fn default() -> Self {
        Self {
            last_poll: Instant::now() - Duration::from_secs(86400),
            backoff_secs: 0,
            in_progress: false,
            consecutive_failures: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Shared handle to the poll scheduler, stored in AppState for API access.
#[derive(Clone)]
pub struct PollSchedulerHandle {
    inner: Arc<RwLock<PollSchedulerInner>>,
}

struct PollSchedulerInner {
    enabled: bool,
    accounts: HashMap<String, AccountPollState>,
    total_polls: u64,
    total_errors: u64,
}

/// Status snapshot returned by the API.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollStatus {
    pub enabled: bool,
    pub total_polls: u64,
    pub total_errors: u64,
    pub accounts: Vec<AccountPollStatus>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPollStatus {
    pub account_id: String,
    pub seconds_since_last_poll: u64,
    pub backoff_secs: u64,
    pub in_progress: bool,
    pub consecutive_failures: u32,
}

impl PollSchedulerHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PollSchedulerInner {
                enabled: true,
                accounts: HashMap::new(),
                total_polls: 0,
                total_errors: 0,
            })),
        }
    }

    pub async fn status(&self) -> PollStatus {
        let inner = self.inner.read().await;
        let now = Instant::now();
        PollStatus {
            enabled: inner.enabled,
            total_polls: inner.total_polls,
            total_errors: inner.total_errors,
            accounts: inner
                .accounts
                .iter()
                .map(|(id, s)| AccountPollStatus {
                    account_id: id.clone(),
                    seconds_since_last_poll: now.duration_since(s.last_poll).as_secs(),
                    backoff_secs: s.backoff_secs,
                    in_progress: s.in_progress,
                    consecutive_failures: s.consecutive_failures,
                })
                .collect(),
        }
    }

    pub async fn set_enabled(&self, enabled: bool) {
        self.inner.write().await.enabled = enabled;
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MIN_POLL_INTERVAL_SECS: u64 = 60;
const MAX_BACKOFF_SECS: u64 = 600;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;
const TICK_INTERVAL_SECS: u64 = 15;

// ---------------------------------------------------------------------------
// Sync callback type
// ---------------------------------------------------------------------------

/// Closure that performs the actual sync+ingestion for one account.
///
/// This decouples the poll scheduler (in lib crate) from the binary-crate
/// ingestion code. The closure is created in `main.rs` where both
/// `AppState` and `api::ingestion` are visible.
pub type SyncAccountFn =
    Arc<dyn Fn(String) -> futures::future::BoxFuture<'static, Result<u64, String>> + Send + Sync>;

// ---------------------------------------------------------------------------
// Scheduler loop
// ---------------------------------------------------------------------------

/// Start the background poll scheduler. Returns a handle for status/control.
pub fn start(oauth_manager: Arc<OAuthManager>, sync_fn: SyncAccountFn) -> PollSchedulerHandle {
    let handle = PollSchedulerHandle::new();
    let handle_clone = handle.clone();

    tokio::spawn(async move {
        info!("Background email poll scheduler started (tick every {TICK_INTERVAL_SECS}s)");
        poll_loop(oauth_manager, sync_fn, handle_clone).await;
    });

    handle
}

async fn poll_loop(
    oauth_manager: Arc<OAuthManager>,
    sync_fn: SyncAccountFn,
    handle: PollSchedulerHandle,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(TICK_INTERVAL_SECS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;

        if !handle.inner.read().await.enabled {
            continue;
        }

        let accounts = match oauth_manager.list_accounts().await {
            Ok(accts) => accts,
            Err(e) => {
                warn!("Poll scheduler: failed to list accounts: {e}");
                continue;
            }
        };

        let now = Instant::now();

        for account in &accounts {
            if account.status != AccountStatus::Connected {
                continue;
            }

            let configured_secs = if account.sync_frequency > 0 {
                account.sync_frequency as u64
            } else {
                DEFAULT_POLL_INTERVAL_SECS
            };
            let poll_interval = configured_secs.max(MIN_POLL_INTERVAL_SECS);

            let should_poll = {
                let inner = handle.inner.read().await;
                if let Some(acct_state) = inner.accounts.get(&account.id) {
                    if acct_state.in_progress {
                        false
                    } else {
                        let effective_interval = poll_interval + acct_state.backoff_secs;
                        now.duration_since(acct_state.last_poll).as_secs() >= effective_interval
                    }
                } else {
                    true
                }
            };

            if !should_poll {
                continue;
            }

            {
                let mut inner = handle.inner.write().await;
                inner
                    .accounts
                    .entry(account.id.clone())
                    .or_default()
                    .in_progress = true;
            }

            let bg_sync_fn = sync_fn.clone();
            let bg_handle = handle.clone();
            let account_id = account.id.clone();
            let account_email = account.email_address.clone();

            tokio::spawn(async move {
                debug!(
                    account_id = %account_id,
                    email = %account_email,
                    "Poll scheduler: syncing account"
                );

                let result = bg_sync_fn(account_id.clone()).await;

                let mut inner = bg_handle.inner.write().await;
                inner.total_polls += 1;

                let is_err = result.is_err();
                if is_err {
                    inner.total_errors += 1;
                }

                let acct_state = inner.accounts.entry(account_id.clone()).or_default();
                acct_state.last_poll = Instant::now();
                acct_state.in_progress = false;

                match result {
                    Ok(synced) => {
                        acct_state.backoff_secs = 0;
                        acct_state.consecutive_failures = 0;
                        if synced > 0 {
                            info!(
                                account_id = %account_id,
                                new_emails = synced,
                                "Poll scheduler: new emails synced and queued for ingestion"
                            );
                        } else {
                            debug!(account_id = %account_id, "Poll scheduler: no new emails");
                        }
                    }
                    Err(e) => {
                        acct_state.consecutive_failures += 1;
                        acct_state.backoff_secs = (30u64
                            * 2u64.saturating_pow(acct_state.consecutive_failures - 1))
                        .min(MAX_BACKOFF_SECS);
                        warn!(
                            account_id = %account_id,
                            failures = acct_state.consecutive_failures,
                            backoff_secs = acct_state.backoff_secs,
                            "Poll scheduler: sync failed: {e}"
                        );
                    }
                }
            });
        }
    }
}
