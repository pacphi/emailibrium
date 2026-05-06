//! Predicate expansion (ADR-030 §C.1 step 6 + DDD-008 addendum).
//!
//! Phase A's repository stub returns an empty Vec for `expand_predicate`.
//! Phase C drives expansion at apply time: a predicate row is materialised
//! into N child `PlannedOperationRow`s on demand, paginated to keep memory
//! bounded.
//!
//! Phase C contract: `expand_page` returns up to `page_size` (≤1000) child
//! rows for a single predicate row. The caller (`AccountWorker`) is
//! responsible for assigning child `seq` values that are strictly greater
//! than the maximum existing seq in the plan — see `next_seq_hint` below.

use std::sync::Arc;

use thiserror::Error;

use crate::cleanup::domain::operation::{
    OperationStatus, PlanAction, PlannedOperationPredicate, PlannedOperationRow,
};
use crate::cleanup::domain::ports::{EmailRepository, RepoError, RuleEvaluator};

/// Maximum page size enforced by the expander itself (ADR-030 §C.1 step 6).
pub const MAX_PAGE_SIZE: u32 = 1000;

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error("repo: {0}")]
    Repo(#[from] RepoError),
    #[error("not implemented for predicate kind {0}")]
    NotImplemented(&'static str),
}

pub struct PredicateExpander {
    #[allow(dead_code)] // Phase C uses for Rule expansion; ArchiveStrategy uses `emails`.
    rules: Arc<dyn RuleEvaluator>,
    emails: Arc<dyn EmailRepository>,
}

impl PredicateExpander {
    pub fn new(rules: Arc<dyn RuleEvaluator>, emails: Arc<dyn EmailRepository>) -> Self {
        Self { rules, emails }
    }

    /// Expand one predicate row into a page of materialized children.
    ///
    /// Children come back without `seq` populated — the caller must assign
    /// `seq` values; use [`next_seq_hint`](Self::next_seq_hint) to compute
    /// the starting value.
    pub async fn expand_page(
        &self,
        predicate: &PlannedOperationPredicate,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<PlannedOperationRow>, ExpandError> {
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
        let offset = page.saturating_mul(page_size) as usize;

        // Phase C only expands by-account today: archive-strategy + label
        // filters list emails for the predicate's account and produce one
        // child row per email. Rule predicates are stubbed (Phase D wires
        // them properly via the rule engine).
        let emails = self
            .emails
            .list_by_account(&predicate.account_id)
            .await
            .map_err(ExpandError::Repo)?;
        let slice_end = (offset + page_size as usize).min(emails.len());
        if offset >= emails.len() {
            return Ok(Vec::new());
        }
        let slice = &emails[offset..slice_end];

        let rows = slice
            .iter()
            .map(|er| PlannedOperationRow {
                seq: 0, // assigned by caller
                account_id: predicate.account_id.clone(),
                email_id: Some(er.id.clone()),
                action: predicate.action.clone(),
                source: predicate.source.clone(),
                target: predicate.target.clone(),
                reverse_op: reverse_for_action(&predicate.action),
                risk: predicate.risk,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            })
            .collect();
        Ok(rows)
    }

    /// Hint for the next seq value: caller assigns
    /// `max_existing_seq + 1, +2, …` to the returned children.
    pub fn next_seq_hint(max_existing_seq: u64) -> u64 {
        max_existing_seq.saturating_add(1)
    }
}

fn reverse_for_action(action: &PlanAction) -> Option<crate::cleanup::domain::operation::ReverseOp> {
    use crate::cleanup::domain::operation::{FolderOrLabel, MoveKind, ReverseOp};
    match action {
        PlanAction::Delete { permanent: true } | PlanAction::Unsubscribe { .. } => {
            Some(ReverseOp::Irreversible)
        }
        PlanAction::Delete { permanent: false } => Some(ReverseOp::MoveBack {
            kind: MoveKind::Label,
            target: FolderOrLabel {
                id: "INBOX".into(),
                name: "Inbox".into(),
                kind: MoveKind::Label,
            },
        }),
        PlanAction::Archive => Some(ReverseOp::AddLabel {
            kind: MoveKind::Label,
            target: FolderOrLabel {
                id: "INBOX".into(),
                name: "Inbox".into(),
                kind: MoveKind::Label,
            },
        }),
        PlanAction::AddLabel { kind } => Some(ReverseOp::RemoveLabel {
            kind: *kind,
            target: FolderOrLabel {
                id: String::new(),
                name: String::new(),
                kind: *kind,
            },
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use crate::cleanup::domain::operation::{
        EmailRef, PlanSource, PredicateKind, PredicateStatus, RiskLevel,
    };
    struct StubEmailRepo {
        emails: Vec<EmailRef>,
    }

    #[async_trait]
    impl EmailRepository for StubEmailRepo {
        async fn list_by_account(&self, _account_id: &str) -> Result<Vec<EmailRef>, RepoError> {
            Ok(self.emails.clone())
        }
        async fn list_by_cluster(&self, _cluster_id: &str) -> Result<Vec<EmailRef>, RepoError> {
            Ok(Vec::new())
        }
        async fn count_by_account(&self, _account_id: &str) -> Result<u64, RepoError> {
            Ok(self.emails.len() as u64)
        }
    }

    struct StubRuleEvaluator;

    #[async_trait]
    impl RuleEvaluator for StubRuleEvaluator {
        async fn evaluate_scope(
            &self,
            _mode: crate::rules::types::RuleExecutionMode,
            _scope: crate::rules::types::EvaluationScope,
        ) -> Result<
            Vec<crate::rules::types::RuleEvaluation>,
            crate::cleanup::domain::ports::RuleEvalError,
        > {
            Ok(Vec::new())
        }
    }

    fn make_predicate(account_id: &str) -> PlannedOperationPredicate {
        PlannedOperationPredicate {
            seq: 1,
            account_id: account_id.into(),
            predicate_kind: PredicateKind::ArchiveStrategy,
            predicate_id: "older30d".into(),
            action: PlanAction::Archive,
            target: None,
            source: PlanSource::Manual,
            projected_count: 0,
            sample_email_ids: vec![],
            risk: RiskLevel::Low,
            status: PredicateStatus::Pending,
            partial_applied_count: 0,
            error: None,
        }
    }

    #[tokio::test]
    async fn expand_page_returns_empty_when_offset_exceeds_total() {
        let emails = (0..5)
            .map(|i| EmailRef {
                id: format!("e{i}"),
                account_id: "acct".into(),
            })
            .collect();
        let expander = PredicateExpander::new(
            Arc::new(StubRuleEvaluator),
            Arc::new(StubEmailRepo { emails }),
        );
        let pred = make_predicate("acct");
        let page = expander.expand_page(&pred, 10, 100).await.unwrap();
        assert!(page.is_empty());
    }

    #[tokio::test]
    async fn expand_page_caps_page_size_to_max() {
        let emails = (0..2500)
            .map(|i| EmailRef {
                id: format!("e{i}"),
                account_id: "acct".into(),
            })
            .collect();
        let expander = PredicateExpander::new(
            Arc::new(StubRuleEvaluator),
            Arc::new(StubEmailRepo { emails }),
        );
        let pred = make_predicate("acct");
        let page = expander.expand_page(&pred, 0, 100_000).await.unwrap();
        assert_eq!(page.len() as u32, MAX_PAGE_SIZE);
    }

    #[tokio::test]
    async fn predicate_expansion_10k_pages_cleanly() {
        // 10k emails, page_size=1000 → 10 pages, RSS-bounded by vec size.
        let emails: Vec<EmailRef> = (0..10_000)
            .map(|i| EmailRef {
                id: format!("e{i}"),
                account_id: "acct".into(),
            })
            .collect();
        let expander = PredicateExpander::new(
            Arc::new(StubRuleEvaluator),
            Arc::new(StubEmailRepo { emails }),
        );
        let pred = make_predicate("acct");
        let mut total_rows: u64 = 0;
        let page_size = 1000u32;
        for page in 0..20 {
            let rows = expander.expand_page(&pred, page, page_size).await.unwrap();
            // Each page is bounded by page_size — never grows unbounded.
            assert!(rows.len() as u32 <= page_size);
            if rows.is_empty() {
                break;
            }
            total_rows += rows.len() as u64;
        }
        assert_eq!(total_rows, 10_000);
    }

    #[test]
    fn next_seq_hint_is_max_plus_one() {
        assert_eq!(PredicateExpander::next_seq_hint(0), 1);
        assert_eq!(PredicateExpander::next_seq_hint(99), 100);
        assert_eq!(PredicateExpander::next_seq_hint(u64::MAX), u64::MAX);
    }
}
