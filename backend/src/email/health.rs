//! Account health monitoring service (DDD-005: Account Management, Audit Item #37).
//!
//! `AccountHealthMonitor` tracks the health of connected email accounts
//! by checking token expiry, connection liveness, and error rates.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::AccountStatus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Health check result for a single account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountHealth {
    pub account_id: String,
    pub status: AccountStatus,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub token_expired: bool,
    pub error_count: u32,
    pub last_error: Option<String>,
    pub last_check_at: DateTime<Utc>,
    pub consecutive_failures: u32,
}

impl AccountHealth {
    /// Create a healthy default state for a new account.
    pub fn new(account_id: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            status: AccountStatus::Connected,
            token_expires_at: None,
            token_expired: false,
            error_count: 0,
            last_error: None,
            last_check_at: Utc::now(),
            consecutive_failures: 0,
        }
    }

    /// Check if the token will expire within the given duration.
    pub fn token_expiring_soon(&self, within: Duration) -> bool {
        match self.token_expires_at {
            Some(expires_at) => expires_at < Utc::now() + within,
            None => false,
        }
    }

    /// Record a successful health check.
    pub fn record_success(&mut self) {
        self.status = AccountStatus::Connected;
        self.consecutive_failures = 0;
        self.last_check_at = Utc::now();
    }

    /// Record a failed health check.
    pub fn record_failure(&mut self, error: String) {
        self.error_count += 1;
        self.consecutive_failures += 1;
        self.last_error = Some(error);
        self.last_check_at = Utc::now();

        // Transition status based on consecutive failures
        if self.consecutive_failures >= 5 {
            self.status = AccountStatus::Suspended;
        } else if self.consecutive_failures >= 1 {
            self.status = AccountStatus::Error;
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Account health monitor trait for testability.
#[async_trait]
pub trait AccountHealthService: Send + Sync {
    /// Get the health status for a specific account.
    async fn get_health(&self, account_id: &str) -> Option<AccountHealth>;

    /// Update the health record for an account after a check.
    async fn update_health(&self, account_id: &str, health: AccountHealth);

    /// Get all accounts with expiring tokens.
    async fn accounts_needing_refresh(&self, within: Duration) -> Vec<AccountHealth>;

    /// Get all accounts in an error or suspended state.
    async fn unhealthy_accounts(&self) -> Vec<AccountHealth>;

    /// Record a successful operation for the account.
    async fn record_success(&self, account_id: &str);

    /// Record a failed operation for the account.
    async fn record_failure(&self, account_id: &str, error: String);
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// In-memory account health monitor.
///
/// A production implementation would persist health state to the database
/// and run periodic background checks. This implementation tracks state
/// in memory for the current process lifetime.
pub struct AccountHealthMonitor {
    accounts: Arc<RwLock<HashMap<String, AccountHealth>>>,
}

impl AccountHealthMonitor {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for AccountHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AccountHealthService for AccountHealthMonitor {
    async fn get_health(&self, account_id: &str) -> Option<AccountHealth> {
        let accounts = self.accounts.read().await;
        accounts.get(account_id).cloned()
    }

    async fn update_health(&self, account_id: &str, health: AccountHealth) {
        let mut accounts = self.accounts.write().await;
        accounts.insert(account_id.to_string(), health);
    }

    async fn accounts_needing_refresh(&self, within: Duration) -> Vec<AccountHealth> {
        let accounts = self.accounts.read().await;
        accounts
            .values()
            .filter(|h| h.token_expiring_soon(within))
            .cloned()
            .collect()
    }

    async fn unhealthy_accounts(&self) -> Vec<AccountHealth> {
        let accounts = self.accounts.read().await;
        accounts
            .values()
            .filter(|h| h.status == AccountStatus::Error || h.status == AccountStatus::Suspended)
            .cloned()
            .collect()
    }

    async fn record_success(&self, account_id: &str) {
        let mut accounts = self.accounts.write().await;
        let health = accounts
            .entry(account_id.to_string())
            .or_insert_with(|| AccountHealth::new(account_id));
        health.record_success();
    }

    async fn record_failure(&self, account_id: &str, error: String) {
        let mut accounts = self.accounts.write().await;
        let health = accounts
            .entry(account_id.to_string())
            .or_insert_with(|| AccountHealth::new(account_id));
        health.record_failure(error);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_health_new() {
        let health = AccountHealth::new("acct-1");
        assert_eq!(health.status, AccountStatus::Connected);
        assert_eq!(health.error_count, 0);
        assert_eq!(health.consecutive_failures, 0);
        assert!(!health.token_expired);
    }

    #[test]
    fn test_record_failure_transitions() {
        let mut health = AccountHealth::new("acct-1");

        // First failure -> Error
        health.record_failure("timeout".into());
        assert_eq!(health.status, AccountStatus::Error);
        assert_eq!(health.consecutive_failures, 1);

        // Recover
        health.record_success();
        assert_eq!(health.status, AccountStatus::Connected);
        assert_eq!(health.consecutive_failures, 0);

        // 5 consecutive failures -> Suspended
        for i in 0..5 {
            health.record_failure(format!("error {i}"));
        }
        assert_eq!(health.status, AccountStatus::Suspended);
        assert_eq!(health.consecutive_failures, 5);
        assert_eq!(health.error_count, 6); // 1 from before + 5
    }

    #[test]
    fn test_token_expiring_soon() {
        let mut health = AccountHealth::new("acct-1");
        // No expiry set
        assert!(!health.token_expiring_soon(Duration::hours(1)));

        // Set expiry 30 minutes from now
        health.token_expires_at = Some(Utc::now() + Duration::minutes(30));
        // Should be expiring within 1 hour
        assert!(health.token_expiring_soon(Duration::hours(1)));
        // Should NOT be expiring within 10 minutes
        assert!(!health.token_expiring_soon(Duration::minutes(10)));
    }

    #[tokio::test]
    async fn test_monitor_record_and_get() {
        let monitor = AccountHealthMonitor::new();

        monitor.record_success("acct-1").await;
        let health = monitor.get_health("acct-1").await.unwrap();
        assert_eq!(health.status, AccountStatus::Connected);

        monitor
            .record_failure("acct-1", "connection refused".into())
            .await;
        let health = monitor.get_health("acct-1").await.unwrap();
        assert_eq!(health.status, AccountStatus::Error);
    }

    #[tokio::test]
    async fn test_unhealthy_accounts() {
        let monitor = AccountHealthMonitor::new();

        monitor.record_success("acct-1").await;
        monitor.record_failure("acct-2", "error".into()).await;

        let unhealthy = monitor.unhealthy_accounts().await;
        assert_eq!(unhealthy.len(), 1);
        assert_eq!(unhealthy[0].account_id, "acct-2");
    }

    #[tokio::test]
    async fn test_accounts_needing_refresh() {
        let monitor = AccountHealthMonitor::new();

        let mut health = AccountHealth::new("acct-1");
        health.token_expires_at = Some(Utc::now() + Duration::minutes(5));
        monitor.update_health("acct-1", health).await;

        let mut health2 = AccountHealth::new("acct-2");
        health2.token_expires_at = Some(Utc::now() + Duration::hours(12));
        monitor.update_health("acct-2", health2).await;

        // Within 1 hour: only acct-1 needs refresh
        let needing = monitor.accounts_needing_refresh(Duration::hours(1)).await;
        assert_eq!(needing.len(), 1);
        assert_eq!(needing[0].account_id, "acct-1");
    }
}
