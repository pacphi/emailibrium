//! Per-account pipeline locking to prevent concurrent sync/ingestion runs.
//!
//! `AccountLockMap` is stored in `AppState` and checked before spawning any
//! sync or ingestion background task. Each lock carries a `PipelineActivity`
//! describing who holds the lock and what phase it's in, so the frontend can
//! display a meaningful "pipeline busy" message.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Describes the currently-active pipeline operation for an account.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineActivity {
    pub job_id: String,
    pub account_id: String,
    pub phase: String,
    pub started_at: DateTime<Utc>,
    /// Who started this pipeline: "onboarding", "manual_sync", "inbox_clean", "poll".
    pub source: String,
}

/// In-memory per-account lock preventing concurrent pipeline runs.
///
/// Process-scoped: automatically cleared on server restart (no stale DB locks).
#[derive(Clone, Default)]
pub struct AccountLockMap {
    inner: Arc<RwLock<HashMap<String, PipelineActivity>>>,
}

impl AccountLockMap {
    /// Try to acquire the lock for an account.
    ///
    /// Returns `Ok(())` if acquired, or `Err(existing_activity)` if something
    /// is already running for this account.
    pub async fn try_acquire(
        &self,
        account_id: &str,
        activity: PipelineActivity,
    ) -> Result<(), PipelineActivity> {
        let mut map = self.inner.write().await;
        if let Some(existing) = map.get(account_id) {
            return Err(existing.clone());
        }
        map.insert(account_id.to_string(), activity);
        Ok(())
    }

    /// Release the lock when the pipeline finishes (success or failure).
    pub async fn release(&self, account_id: &str) {
        self.inner.write().await.remove(account_id);
    }

    /// Update the phase of an existing lock (e.g. syncing → embedding).
    pub async fn update_phase(&self, account_id: &str, phase: &str) {
        let mut map = self.inner.write().await;
        if let Some(activity) = map.get_mut(account_id) {
            activity.phase = phase.to_string();
        }
    }

    /// Read-only check — for status queries without acquiring.
    pub async fn get_activity(&self, account_id: &str) -> Option<PipelineActivity> {
        self.inner.read().await.get(account_id).cloned()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_activity(account_id: &str, source: &str) -> PipelineActivity {
        PipelineActivity {
            job_id: "test-job".to_string(),
            account_id: account_id.to_string(),
            phase: "syncing".to_string(),
            started_at: Utc::now(),
            source: source.to_string(),
        }
    }

    #[tokio::test]
    async fn test_acquire_and_release() {
        let locks = AccountLockMap::default();
        let activity = make_activity("acct-1", "manual_sync");

        // First acquire succeeds.
        assert!(locks.try_acquire("acct-1", activity).await.is_ok());

        // Second acquire for the same account fails.
        let activity2 = make_activity("acct-1", "inbox_clean");
        let err = locks.try_acquire("acct-1", activity2).await.unwrap_err();
        assert_eq!(err.source, "manual_sync");

        // Different account succeeds.
        let activity3 = make_activity("acct-2", "poll");
        assert!(locks.try_acquire("acct-2", activity3).await.is_ok());

        // Release and re-acquire.
        locks.release("acct-1").await;
        let activity4 = make_activity("acct-1", "inbox_clean");
        assert!(locks.try_acquire("acct-1", activity4).await.is_ok());
    }

    #[tokio::test]
    async fn test_update_phase() {
        let locks = AccountLockMap::default();
        let activity = make_activity("acct-1", "manual_sync");
        locks.try_acquire("acct-1", activity).await.unwrap();

        locks.update_phase("acct-1", "embedding").await;

        let a = locks.get_activity("acct-1").await.unwrap();
        assert_eq!(a.phase, "embedding");
    }

    #[tokio::test]
    async fn test_get_activity_none_when_empty() {
        let locks = AccountLockMap::default();
        assert!(locks.get_activity("acct-1").await.is_none());
    }
}
