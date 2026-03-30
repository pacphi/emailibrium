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
use crate::vectors::yaml_config::SyncConfig;

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
    /// Sync completion config from app.yaml, exposed for API consumers
    /// that need to poll for sync completion.
    sync_completion: Arc<SyncCompletionConfig>,
}

/// Sync completion polling parameters derived from `app.yaml`.
#[derive(Debug, Clone)]
pub struct SyncCompletionConfig {
    /// Number of consecutive stable count checks before declaring sync complete.
    pub stable_checks: usize,
    /// Interval (ms) between sync completion stability checks.
    pub check_interval_ms: u64,
    /// Maximum polls before giving up on sync completion.
    pub max_wait_polls: usize,
}

impl Default for SyncCompletionConfig {
    fn default() -> Self {
        Self {
            stable_checks: 2,
            check_interval_ms: 3000,
            max_wait_polls: 120,
        }
    }
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
    fn new(sync_completion: SyncCompletionConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PollSchedulerInner {
                enabled: true,
                accounts: HashMap::new(),
                total_polls: 0,
                total_errors: 0,
            })),
            sync_completion: Arc::new(sync_completion),
        }
    }

    /// Access sync completion configuration from `app.yaml`.
    pub fn sync_completion_config(&self) -> &SyncCompletionConfig {
        &self.sync_completion
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
// Constants (fallback defaults when no config is provided)
// ---------------------------------------------------------------------------

const MIN_POLL_INTERVAL_SECS: u64 = 60;
const MAX_BACKOFF_SECS: u64 = 600;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;
const TICK_INTERVAL_SECS: u64 = 15;

/// Resolved configuration values for the poll scheduler, derived from
/// `SyncConfig` (app.yaml) with fallback to compile-time constants.
#[derive(Debug, Clone)]
struct PollConfig {
    tick_interval_secs: u64,
    default_poll_interval_secs: u64,
    /// Number of consecutive stable count checks before declaring sync complete.
    sync_completion_stable_checks: usize,
    /// Interval (ms) between sync completion stability checks.
    sync_completion_check_interval_ms: u64,
    /// Maximum polls before giving up on sync completion.
    max_sync_wait_polls: usize,
}

impl PollConfig {
    fn from_sync_config(sync: &SyncConfig) -> Self {
        Self {
            tick_interval_secs: if sync.poll_interval_secs > 0 {
                sync.poll_interval_secs
            } else {
                TICK_INTERVAL_SECS
            },
            default_poll_interval_secs: if sync.default_sync_frequency_minutes > 0 {
                sync.default_sync_frequency_minutes * 60
            } else {
                DEFAULT_POLL_INTERVAL_SECS
            },
            sync_completion_stable_checks: if sync.sync_completion_stable_checks > 0 {
                sync.sync_completion_stable_checks
            } else {
                2
            },
            sync_completion_check_interval_ms: if sync.sync_completion_check_interval_ms > 0 {
                sync.sync_completion_check_interval_ms
            } else {
                3000
            },
            max_sync_wait_polls: if sync.max_sync_wait_polls > 0 {
                sync.max_sync_wait_polls
            } else {
                120
            },
        }
    }
}

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
///
/// Reads `poll_interval_secs` and `default_sync_frequency_minutes` from
/// the YAML `SyncConfig` so that the tick interval and default per-account
/// poll frequency are configurable via `config/app.yaml`.
pub fn start(
    oauth_manager: Arc<OAuthManager>,
    sync_fn: SyncAccountFn,
    sync_config: &SyncConfig,
) -> PollSchedulerHandle {
    let poll_cfg = PollConfig::from_sync_config(sync_config);
    let sync_completion = SyncCompletionConfig {
        stable_checks: poll_cfg.sync_completion_stable_checks,
        check_interval_ms: poll_cfg.sync_completion_check_interval_ms,
        max_wait_polls: poll_cfg.max_sync_wait_polls,
    };
    let handle = PollSchedulerHandle::new(sync_completion);
    let handle_clone = handle.clone();

    tokio::spawn(async move {
        info!(
            "Background email poll scheduler started (tick every {}s, default poll interval {}s)",
            poll_cfg.tick_interval_secs, poll_cfg.default_poll_interval_secs
        );
        poll_loop(oauth_manager, sync_fn, handle_clone, &poll_cfg).await;
    });

    handle
}

async fn poll_loop(
    oauth_manager: Arc<OAuthManager>,
    sync_fn: SyncAccountFn,
    handle: PollSchedulerHandle,
    poll_cfg: &PollConfig,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(poll_cfg.tick_interval_secs));
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
                poll_cfg.default_poll_interval_secs
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- PollSchedulerHandle creation and defaults --------------------------

    #[tokio::test]
    async fn test_handle_creation_defaults() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        let status = handle.status().await;
        assert!(status.enabled);
        assert_eq!(status.total_polls, 0);
        assert_eq!(status.total_errors, 0);
        assert!(status.accounts.is_empty());
    }

    // -- Enable / disable ---------------------------------------------------

    #[tokio::test]
    async fn test_set_enabled_false() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        handle.set_enabled(false).await;
        let status = handle.status().await;
        assert!(!status.enabled);
    }

    #[tokio::test]
    async fn test_set_enabled_toggle() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        handle.set_enabled(false).await;
        assert!(!handle.status().await.enabled);
        handle.set_enabled(true).await;
        assert!(handle.status().await.enabled);
    }

    // -- AccountPollState defaults ------------------------------------------

    #[test]
    fn test_account_poll_state_defaults() {
        let state = AccountPollState::default();
        assert_eq!(state.backoff_secs, 0);
        assert!(!state.in_progress);
        assert_eq!(state.consecutive_failures, 0);
        // last_poll should be far in the past (86400 seconds ago)
        let elapsed = state.last_poll.elapsed().as_secs();
        assert!(
            elapsed >= 86300,
            "Expected last_poll to be ~24h ago, got {elapsed}s"
        );
    }

    // -- Account tracking via inner state -----------------------------------

    #[tokio::test]
    async fn test_add_account_tracking() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        {
            let mut inner = handle.inner.write().await;
            inner
                .accounts
                .insert("acct-1".to_string(), AccountPollState::default());
        }
        let status = handle.status().await;
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(status.accounts[0].account_id, "acct-1");
        assert_eq!(status.accounts[0].consecutive_failures, 0);
    }

    #[tokio::test]
    async fn test_remove_account_tracking() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        {
            let mut inner = handle.inner.write().await;
            inner
                .accounts
                .insert("acct-1".to_string(), AccountPollState::default());
            inner
                .accounts
                .insert("acct-2".to_string(), AccountPollState::default());
        }
        assert_eq!(handle.status().await.accounts.len(), 2);

        {
            let mut inner = handle.inner.write().await;
            inner.accounts.remove("acct-1");
        }
        let status = handle.status().await;
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(status.accounts[0].account_id, "acct-2");
    }

    // -- Backoff calculation ------------------------------------------------

    #[test]
    fn test_backoff_calculation_first_failure() {
        // First failure: 30 * 2^0 = 30
        let backoff = (30u64 * 2u64.saturating_pow(0)).min(MAX_BACKOFF_SECS);
        assert_eq!(backoff, 30);
    }

    #[test]
    fn test_backoff_calculation_escalation() {
        // Second failure: 30 * 2^1 = 60
        let backoff2 = (30u64 * 2u64.saturating_pow(1)).min(MAX_BACKOFF_SECS);
        assert_eq!(backoff2, 60);

        // Third failure: 30 * 2^2 = 120
        let backoff3 = (30u64 * 2u64.saturating_pow(2)).min(MAX_BACKOFF_SECS);
        assert_eq!(backoff3, 120);

        // Fifth failure: 30 * 2^4 = 480
        let backoff5 = (30u64 * 2u64.saturating_pow(4)).min(MAX_BACKOFF_SECS);
        assert_eq!(backoff5, 480);
    }

    #[test]
    fn test_backoff_capped_at_max() {
        // Sixth failure: 30 * 2^5 = 960, capped to MAX_BACKOFF_SECS (600)
        let backoff = (30u64 * 2u64.saturating_pow(5)).min(MAX_BACKOFF_SECS);
        assert_eq!(backoff, MAX_BACKOFF_SECS);
        assert_eq!(backoff, 600);
    }

    // -- Poll interval logic ------------------------------------------------

    #[test]
    fn test_poll_interval_minimum_enforced() {
        // A sync_frequency of 10 should be raised to MIN_POLL_INTERVAL_SECS (60)
        let configured_secs: u64 = 10;
        let poll_interval = configured_secs.max(MIN_POLL_INTERVAL_SECS);
        assert_eq!(poll_interval, 60);
    }

    #[test]
    fn test_poll_interval_uses_configured_when_above_min() {
        let configured_secs: u64 = 180;
        let poll_interval = configured_secs.max(MIN_POLL_INTERVAL_SECS);
        assert_eq!(poll_interval, 180);
    }

    #[test]
    fn test_poll_interval_default_when_zero() {
        // sync_frequency == 0 should use DEFAULT_POLL_INTERVAL_SECS
        let sync_frequency: i32 = 0;
        let configured_secs = if sync_frequency > 0 {
            sync_frequency as u64
        } else {
            DEFAULT_POLL_INTERVAL_SECS
        };
        let poll_interval = configured_secs.max(MIN_POLL_INTERVAL_SECS);
        assert_eq!(poll_interval, DEFAULT_POLL_INTERVAL_SECS);
        assert_eq!(poll_interval, 300);
    }

    // -- Poll counters via simulated sync results ---------------------------

    #[tokio::test]
    async fn test_poll_counters_on_success() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        {
            let mut inner = handle.inner.write().await;
            inner
                .accounts
                .insert("acct-1".to_string(), AccountPollState::default());
            inner.total_polls += 1;
            let state = inner.accounts.get_mut("acct-1").unwrap();
            state.last_poll = Instant::now();
            state.backoff_secs = 0;
            state.consecutive_failures = 0;
        }
        let status = handle.status().await;
        assert_eq!(status.total_polls, 1);
        assert_eq!(status.total_errors, 0);
        assert_eq!(status.accounts[0].consecutive_failures, 0);
        assert_eq!(status.accounts[0].backoff_secs, 0);
    }

    #[tokio::test]
    async fn test_poll_counters_on_failure() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig::default());
        {
            let mut inner = handle.inner.write().await;
            inner
                .accounts
                .insert("acct-1".to_string(), AccountPollState::default());
            inner.total_polls += 1;
            inner.total_errors += 1;
            let state = inner.accounts.get_mut("acct-1").unwrap();
            state.last_poll = Instant::now();
            state.consecutive_failures = 2;
            state.backoff_secs = (30u64 * 2u64.saturating_pow(1)).min(MAX_BACKOFF_SECS);
        }
        let status = handle.status().await;
        assert_eq!(status.total_polls, 1);
        assert_eq!(status.total_errors, 1);
        assert_eq!(status.accounts[0].consecutive_failures, 2);
        assert_eq!(status.accounts[0].backoff_secs, 60);
    }

    // -- Constants ----------------------------------------------------------

    #[test]
    fn test_constants() {
        assert_eq!(MIN_POLL_INTERVAL_SECS, 60);
        assert_eq!(MAX_BACKOFF_SECS, 600);
        assert_eq!(DEFAULT_POLL_INTERVAL_SECS, 300);
        assert_eq!(TICK_INTERVAL_SECS, 15);
    }

    // -- PollConfig from SyncConfig -----------------------------------------

    #[test]
    fn test_poll_config_from_sync_config_uses_yaml_values() {
        let sync = SyncConfig {
            poll_interval_secs: 20,
            default_sync_frequency_minutes: 10,
            sync_completion_stable_checks: 5,
            sync_completion_check_interval_ms: 5000,
            max_sync_wait_polls: 200,
            fetch_page_delay_ms: 200,
        };
        let cfg = PollConfig::from_sync_config(&sync);
        assert_eq!(cfg.tick_interval_secs, 20);
        assert_eq!(cfg.default_poll_interval_secs, 600); // 10 * 60
        assert_eq!(cfg.sync_completion_stable_checks, 5);
        assert_eq!(cfg.sync_completion_check_interval_ms, 5000);
        assert_eq!(cfg.max_sync_wait_polls, 200);
    }

    #[test]
    fn test_poll_config_from_sync_config_falls_back_on_zero() {
        let sync = SyncConfig {
            poll_interval_secs: 0,
            default_sync_frequency_minutes: 0,
            sync_completion_stable_checks: 0,
            sync_completion_check_interval_ms: 0,
            max_sync_wait_polls: 0,
            fetch_page_delay_ms: 0,
        };
        let cfg = PollConfig::from_sync_config(&sync);
        assert_eq!(cfg.tick_interval_secs, TICK_INTERVAL_SECS);
        assert_eq!(cfg.default_poll_interval_secs, DEFAULT_POLL_INTERVAL_SECS);
        assert_eq!(cfg.sync_completion_stable_checks, 2);
        assert_eq!(cfg.sync_completion_check_interval_ms, 3000);
        assert_eq!(cfg.max_sync_wait_polls, 120);
    }

    // -- SyncCompletionConfig accessible via handle -------------------------

    #[tokio::test]
    async fn test_sync_completion_config_from_handle() {
        let handle = PollSchedulerHandle::new(SyncCompletionConfig {
            stable_checks: 3,
            check_interval_ms: 4000,
            max_wait_polls: 100,
        });
        let scc = handle.sync_completion_config();
        assert_eq!(scc.stable_checks, 3);
        assert_eq!(scc.check_interval_ms, 4000);
        assert_eq!(scc.max_wait_polls, 100);
    }

    // -- Clone semantics for handle -----------------------------------------

    #[tokio::test]
    async fn test_handle_clone_shares_state() {
        let handle1 = PollSchedulerHandle::new(SyncCompletionConfig::default());
        let handle2 = handle1.clone();

        handle1.set_enabled(false).await;
        // handle2 should see the same state
        assert!(!handle2.status().await.enabled);
    }
}
