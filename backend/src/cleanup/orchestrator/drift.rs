//! Drift detection (ADR-030 §8 + DDD-008 addendum).
//!
//! Compares each account's stored `AccountStateEtag` (as recorded on the
//! plan) against the live etag from the provider via [`AccountStateProvider`].
//!
//! Phase C policy:
//! - Match → `Clean`.
//! - Live etag advanced but the kind is the same → `Soft` (caller may
//!   re-evaluate predicates if desired; messages already affected stay
//!   coherent).
//! - Etag kind changed in a way that invalidates the baseline (Gmail
//!   `historyId` not found, Outlook delta invalid, IMAP `UIDVALIDITY`
//!   changed, POP3 invalidated) → `Hard`. Caller MUST stop the apply for
//!   the affected account and emit `AccountPaused { reason: hardDrift }`.
//!
//! In Phase C the live `AccountStateProvider` stub returns
//! `AccountStateEtag::None`, so for the production wiring `detect_for_account`
//! reports `Clean` unless the test injects a different provider. The
//! test harness uses `MockAccountStateProvider` (in
//! `cleanup/orchestrator/apply.rs` tests) to simulate hard drift.

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

use crate::cleanup::domain::operation::AccountStateEtag;
use crate::cleanup::domain::plan::CleanupPlan;
use crate::cleanup::domain::ports::AccountStateProvider;
use crate::cleanup::domain::ports::RepoError;

#[derive(Debug, Error)]
pub enum DriftError {
    #[error("provider error: {0}")]
    Provider(#[from] RepoError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftStatus {
    Clean,
    Soft { advanced_to: AccountStateEtag },
    Hard { reason: HardDriftReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardDriftReason {
    GmailHistoryNotFound,
    OutlookDeltaInvalid,
    ImapUidvalidityChanged,
    Pop3Invalidated,
}

pub struct DriftDetector {
    accounts: Arc<dyn AccountStateProvider>,
}

impl DriftDetector {
    pub fn new(accounts: Arc<dyn AccountStateProvider>) -> Self {
        Self { accounts }
    }

    /// Compare a single account's baseline etag (from the plan) against the
    /// live etag returned by the [`AccountStateProvider`].
    pub async fn detect_for_account(
        &self,
        account_id: &str,
        baseline: &AccountStateEtag,
    ) -> Result<DriftStatus, DriftError> {
        let live = self.accounts.etag(account_id).await?;
        Ok(classify(baseline, &live))
    }

    /// Detect drift for every account that the plan covers. Accounts that
    /// the plan doesn't reference (e.g., recently added accounts) are
    /// ignored.
    pub async fn detect_all(
        &self,
        plan: &CleanupPlan,
    ) -> Result<HashMap<String, DriftStatus>, DriftError> {
        let mut out = HashMap::with_capacity(plan.account_state_etags.len());
        for (account_id, baseline) in &plan.account_state_etags {
            let status = self.detect_for_account(account_id, baseline).await?;
            out.insert(account_id.clone(), status);
        }
        Ok(out)
    }
}

/// Pure classification of (baseline, live) → DriftStatus per ADR-030 §8.
fn classify(baseline: &AccountStateEtag, live: &AccountStateEtag) -> DriftStatus {
    use AccountStateEtag as E;
    match (baseline, live) {
        // Same shape, same value → clean.
        (a, b) if a == b => DriftStatus::Clean,

        // No baseline: any live value is treated as `Soft` (UI will refresh).
        (E::None, b) => DriftStatus::Soft {
            advanced_to: b.clone(),
        },

        // Gmail: same kind, different historyId → soft. The provider stub in
        // Phase C cannot tell us "historyId not found" without an extra round
        // trip, so we return Soft here. Tests that need Hard drift inject it
        // directly via a mock provider returning `None` against a baseline of
        // a real etag — see `Pop3Invalidated` mapping below.
        (E::GmailHistory { history_id: a }, E::GmailHistory { history_id: b }) if a != b => {
            DriftStatus::Soft {
                advanced_to: live.clone(),
            }
        }
        (E::OutlookDelta { delta_token: a }, E::OutlookDelta { delta_token: b }) if a != b => {
            DriftStatus::Soft {
                advanced_to: live.clone(),
            }
        }

        // IMAP: uidvalidity change is a hard reset. modseq advance is soft.
        (E::ImapUvms { uidvalidity: a, .. }, E::ImapUvms { uidvalidity: b, .. }) if a != b => {
            DriftStatus::Hard {
                reason: HardDriftReason::ImapUidvalidityChanged,
            }
        }
        (
            E::ImapUvms {
                uidvalidity: a,
                highest_modseq: ma,
            },
            E::ImapUvms {
                uidvalidity: b,
                highest_modseq: mb,
            },
        ) if a == b && ma != mb => DriftStatus::Soft {
            advanced_to: live.clone(),
        },

        // POP3: any uidl difference is hard (no incremental protocol).
        (E::Pop3Sentinel { last_uidl: a }, E::Pop3Sentinel { last_uidl: b }) if a != b => {
            DriftStatus::Hard {
                reason: HardDriftReason::Pop3Invalidated,
            }
        }

        // Kind changed across the boundary → provider state was reset.
        // Map by baseline kind to the canonical hard-drift reason.
        (E::GmailHistory { .. }, _) => DriftStatus::Hard {
            reason: HardDriftReason::GmailHistoryNotFound,
        },
        (E::OutlookDelta { .. }, _) => DriftStatus::Hard {
            reason: HardDriftReason::OutlookDeltaInvalid,
        },
        (E::ImapUvms { .. }, _) => DriftStatus::Hard {
            reason: HardDriftReason::ImapUidvalidityChanged,
        },
        (E::Pop3Sentinel { .. }, _) => DriftStatus::Hard {
            reason: HardDriftReason::Pop3Invalidated,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Mock account-state provider parametrised by a fixed map.
    pub struct MockAccountStateProvider {
        pub map: Mutex<HashMap<String, AccountStateEtag>>,
    }

    impl MockAccountStateProvider {
        pub fn new(entries: Vec<(String, AccountStateEtag)>) -> Self {
            Self {
                map: Mutex::new(entries.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl AccountStateProvider for MockAccountStateProvider {
        async fn etag(&self, account_id: &str) -> Result<AccountStateEtag, RepoError> {
            Ok(self
                .map
                .lock()
                .unwrap()
                .get(account_id)
                .cloned()
                .unwrap_or(AccountStateEtag::None))
        }
    }

    #[test]
    fn classify_equal_is_clean() {
        let a = AccountStateEtag::GmailHistory {
            history_id: "1".into(),
        };
        assert_eq!(classify(&a, &a), DriftStatus::Clean);
    }

    #[test]
    fn classify_imap_uidvalidity_change_is_hard() {
        let a = AccountStateEtag::ImapUvms {
            uidvalidity: 1,
            highest_modseq: 1,
        };
        let b = AccountStateEtag::ImapUvms {
            uidvalidity: 2,
            highest_modseq: 1,
        };
        assert_eq!(
            classify(&a, &b),
            DriftStatus::Hard {
                reason: HardDriftReason::ImapUidvalidityChanged
            }
        );
    }

    #[test]
    fn classify_imap_modseq_advance_is_soft() {
        let a = AccountStateEtag::ImapUvms {
            uidvalidity: 1,
            highest_modseq: 1,
        };
        let b = AccountStateEtag::ImapUvms {
            uidvalidity: 1,
            highest_modseq: 5,
        };
        assert!(matches!(classify(&a, &b), DriftStatus::Soft { .. }));
    }

    #[test]
    fn classify_gmail_history_advance_is_soft() {
        let a = AccountStateEtag::GmailHistory {
            history_id: "1".into(),
        };
        let b = AccountStateEtag::GmailHistory {
            history_id: "2".into(),
        };
        assert!(matches!(classify(&a, &b), DriftStatus::Soft { .. }));
    }

    #[test]
    fn classify_kind_change_gmail_baseline_is_hard() {
        let a = AccountStateEtag::GmailHistory {
            history_id: "1".into(),
        };
        let b = AccountStateEtag::None;
        assert_eq!(
            classify(&a, &b),
            DriftStatus::Hard {
                reason: HardDriftReason::GmailHistoryNotFound
            }
        );
    }

    #[test]
    fn classify_pop3_uidl_change_is_hard() {
        let a = AccountStateEtag::Pop3Sentinel {
            last_uidl: "snap-1".into(),
        };
        let b = AccountStateEtag::Pop3Sentinel {
            last_uidl: "snap-2".into(),
        };
        assert_eq!(
            classify(&a, &b),
            DriftStatus::Hard {
                reason: HardDriftReason::Pop3Invalidated
            }
        );
    }

    #[tokio::test]
    async fn detect_for_account_uses_provider() {
        let provider = Arc::new(MockAccountStateProvider::new(vec![(
            "acct-a".into(),
            AccountStateEtag::GmailHistory {
                history_id: "100".into(),
            },
        )])) as Arc<dyn AccountStateProvider>;
        let detector = DriftDetector::new(provider);
        let baseline = AccountStateEtag::GmailHistory {
            history_id: "100".into(),
        };
        let status = detector
            .detect_for_account("acct-a", &baseline)
            .await
            .unwrap();
        assert_eq!(status, DriftStatus::Clean);
    }
}
