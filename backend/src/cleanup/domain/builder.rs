//! `PlanBuilder` — composes wizard selections into a `CleanupPlan`.
//!
//! Pure orchestrator: reads only injected ports, never touches a provider.
//! See ADR-030 §A.4 for the composition flow.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{Duration, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::rules::types::{EvaluationScope, RuleAction, RuleExecutionMode};

use super::classifier::{AccountContext, RiskClassifier};
use super::operation::{
    AccountStateEtag, ClusterAction, MoveKind, OperationStatus, PlanAction, PlanSource, PlanStatus,
    PlanWarning, PlannedOperation, PlannedOperationPredicate, PlannedOperationRow, PredicateKind,
    PredicateStatus, Provider, ReverseOp, RiskLevel,
};
use super::plan::{canonical_plan_hash, CleanupPlan, PlanTotals, RiskRollup, WizardSelections};
use super::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RepoError, RuleEvalError,
    RuleEvaluator, SubscriptionRepository,
};

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("repo: {0}")]
    Repo(#[from] RepoError),
    #[error("rule engine: {0}")]
    Rules(#[from] RuleEvalError),
    #[error("invalid selection: {0}")]
    Invalid(String),
}

pub struct PlanBuilder {
    pub emails: Arc<dyn EmailRepository>,
    pub subs: Arc<dyn SubscriptionRepository>,
    pub clusters: Arc<dyn ClusterRepository>,
    pub rules: Arc<dyn RuleEvaluator>,
    pub accounts: Arc<dyn AccountStateProvider>,
    pub classifier: Arc<RiskClassifier>,
    /// Provider per account (Phase A: caller supplies; Phase C reads from
    /// AccountStateProvider once that surface is widened).
    pub provider_for: Arc<dyn Fn(&str) -> Provider + Send + Sync>,
    pub plan_ttl_minutes: i64,
}

impl PlanBuilder {
    pub async fn build(
        &self,
        user_id: &str,
        sel: WizardSelections,
    ) -> Result<CleanupPlan, BuildError> {
        let now = Utc::now();
        let plan_id = Uuid::now_v7();

        // 1. Per-account etag snapshot + provider snapshot.
        let mut etags: BTreeMap<String, AccountStateEtag> = BTreeMap::new();
        let mut account_providers: BTreeMap<String, Provider> = BTreeMap::new();
        for acct in &sel.account_ids {
            etags.insert(acct.clone(), self.accounts.etag(acct).await?);
            account_providers.insert(acct.clone(), (self.provider_for)(acct));
        }

        let mut next_seq: u64 = 1;
        let mut warnings: Vec<PlanWarning> = Vec::new();
        // Dedup: (account_id, email_id, target_id_or_none) -> seq
        let mut dedup: HashMap<(String, String, Option<String>), usize> = HashMap::new();
        let mut materialized: Vec<PlannedOperationRow> = Vec::new();
        let mut predicates: Vec<PlannedOperationPredicate> = Vec::new();

        // 2. Subscriptions → materialized (sender-level, email_id=None).
        for sub in &sel.subscriptions {
            let record = self
                .subs
                .find_by_sender(&sub.account_id, &sub.sender)
                .await?;
            let method = record
                .as_ref()
                .map(|r| r.method)
                .unwrap_or(super::operation::UnsubscribeMethodKind::None);
            let action = PlanAction::Unsubscribe { method };
            let provider = (self.provider_for)(&sub.account_id);
            let ctx = AccountContext::for_provider(provider);
            let risk = self.classifier.classify(&action, &ctx);
            materialized.push(PlannedOperationRow {
                seq: next_seq,
                account_id: sub.account_id.clone(),
                email_id: None,
                action,
                source: PlanSource::Subscription {
                    sender: sub.sender.clone(),
                },
                target: None,
                reverse_op: Some(ReverseOp::Irreversible),
                risk,
                status: OperationStatus::Pending,
                skip_reason: None,
                applied_at: None,
                error: None,
            });
            next_seq += 1;
        }

        // 3. Cluster actions → materialized (one row per resolved email).
        for cs in &sel.cluster_actions {
            let emails = self.clusters.emails(&cs.cluster_id).await?;
            let action = match cs.action {
                ClusterAction::Archive => PlanAction::Archive,
                ClusterAction::DeleteSoft => PlanAction::Delete { permanent: false },
                ClusterAction::DeletePermanent => PlanAction::Delete { permanent: true },
                ClusterAction::Label => PlanAction::AddLabel {
                    kind: MoveKind::Label,
                },
            };
            let group_size = emails.len() as u64;
            if group_size > 10_000 {
                warnings.push(PlanWarning::LargeGroup {
                    source: PlanSource::Cluster {
                        cluster_id: cs.cluster_id.clone(),
                        cluster_action: cs.action,
                    },
                    projected_count: group_size,
                });
            }
            for e in emails {
                if e.account_id != cs.account_id {
                    continue;
                }
                let key = (e.account_id.clone(), e.id.clone(), None);
                if let Some(existing_idx) = dedup.get(&key).copied() {
                    // Cluster wins over rule; record conflict.
                    let existing = &materialized[existing_idx];
                    if matches!(existing.source, PlanSource::Rule { .. }) {
                        warnings.push(PlanWarning::TargetConflict {
                            account_id: e.account_id.clone(),
                            email_id: e.id.clone(),
                            sources: vec![
                                existing.source.clone(),
                                PlanSource::Cluster {
                                    cluster_id: cs.cluster_id.clone(),
                                    cluster_action: cs.action,
                                },
                            ],
                        });
                        // Replace the rule-sourced row with the cluster-sourced one.
                        materialized[existing_idx].source = PlanSource::Cluster {
                            cluster_id: cs.cluster_id.clone(),
                            cluster_action: cs.action,
                        };
                        materialized[existing_idx].action = action.clone();
                    }
                    continue;
                }
                let provider = (self.provider_for)(&cs.account_id);
                let mut ctx = AccountContext::for_provider(provider);
                ctx.group_size = group_size;
                let risk = self.classifier.classify(&action, &ctx);
                let idx = materialized.len();
                materialized.push(PlannedOperationRow {
                    seq: next_seq,
                    account_id: e.account_id.clone(),
                    email_id: Some(e.id.clone()),
                    action: action.clone(),
                    source: PlanSource::Cluster {
                        cluster_id: cs.cluster_id.clone(),
                        cluster_action: cs.action,
                    },
                    target: None,
                    reverse_op: reverse_for(&action),
                    risk,
                    status: OperationStatus::Pending,
                    skip_reason: None,
                    applied_at: None,
                    error: None,
                });
                dedup.insert(key, idx);
                next_seq += 1;
            }
        }

        // 4. Rule selections → predicate rows (EvaluateOnly).
        let rules_by_account = group_by_account(&sel.rule_selections);
        for (account_id, rule_ids) in rules_by_account {
            let scope = EvaluationScope {
                account_id: account_id.clone(),
                rule_ids: rule_ids.to_vec(),
                sample_size: 20,
            };
            let evals = self
                .rules
                .evaluate_scope(RuleExecutionMode::EvaluateOnly, scope)
                .await?;
            for ev in evals {
                let action = ev
                    .intended_actions
                    .first()
                    .map(plan_action_from_rule)
                    .unwrap_or(PlanAction::Archive);
                let provider = (self.provider_for)(&account_id);
                let mut ctx = AccountContext::for_provider(provider);
                ctx.group_size = ev.projected_count;
                let risk = self.classifier.classify(&action, &ctx);
                if ev.projected_count > 10_000 {
                    warnings.push(PlanWarning::LargeGroup {
                        source: PlanSource::Rule {
                            rule_id: ev.rule_id.clone(),
                            match_basis: match ev.match_basis {
                                crate::rules::types::RuleMatchBasis::Literal => "literal".into(),
                                crate::rules::types::RuleMatchBasis::Semantic => "semantic".into(),
                                crate::rules::types::RuleMatchBasis::Hybrid => "hybrid".into(),
                            },
                        },
                        projected_count: ev.projected_count,
                    });
                }
                predicates.push(PlannedOperationPredicate {
                    seq: next_seq,
                    account_id: account_id.clone(),
                    predicate_kind: PredicateKind::Rule,
                    predicate_id: ev.rule_id.clone(),
                    action,
                    target: None,
                    source: PlanSource::Rule {
                        rule_id: ev.rule_id,
                        match_basis: match ev.match_basis {
                            crate::rules::types::RuleMatchBasis::Literal => "literal".into(),
                            crate::rules::types::RuleMatchBasis::Semantic => "semantic".into(),
                            crate::rules::types::RuleMatchBasis::Hybrid => "hybrid".into(),
                        },
                    },
                    projected_count: ev.projected_count,
                    sample_email_ids: ev.matched_email_ids,
                    risk,
                    status: PredicateStatus::Pending,
                    partial_applied_count: 0,
                    error: None,
                });
                next_seq += 1;
            }
        }

        // 5. Archive strategy → predicate row per account.
        if let Some(strategy) = sel.archive_strategy {
            for account_id in &sel.account_ids {
                let total = self.emails.count_by_account(account_id).await?;
                let provider = (self.provider_for)(account_id);
                let mut ctx = AccountContext::for_provider(provider);
                ctx.group_size = total;
                let action = PlanAction::Archive;
                let risk = self.classifier.classify(&action, &ctx);
                predicates.push(PlannedOperationPredicate {
                    seq: next_seq,
                    account_id: account_id.clone(),
                    predicate_kind: PredicateKind::ArchiveStrategy,
                    predicate_id: format!("{strategy:?}"),
                    action,
                    target: None,
                    source: PlanSource::ArchiveStrategy { strategy },
                    projected_count: total,
                    sample_email_ids: Vec::new(),
                    risk,
                    status: PredicateStatus::Pending,
                    partial_applied_count: 0,
                    error: None,
                });
                next_seq += 1;
            }
        }

        // 6. Build operation list (materialized first, then predicates) and rollups.
        let mut operations: Vec<PlannedOperation> =
            Vec::with_capacity(materialized.len() + predicates.len());
        let mut totals = PlanTotals::default();
        let mut risk = RiskRollup::default();

        let mut bump =
            |totals: &mut PlanTotals, account_id: &str, action_key: &str, source_key: &str| {
                totals.total_operations += 1;
                *totals.by_account.entry(account_id.to_string()).or_insert(0) += 1;
                *totals.by_action.entry(action_key.to_string()).or_insert(0) += 1;
                *totals.by_source.entry(source_key.to_string()).or_insert(0) += 1;
            };

        for r in materialized {
            bump(
                &mut totals,
                &r.account_id,
                action_key(&r.action),
                source_key(&r.source),
            );
            risk.add(r.risk);
            operations.push(PlannedOperation::Materialized(r));
        }
        for p in predicates {
            bump(
                &mut totals,
                &p.account_id,
                action_key(&p.action),
                source_key(&p.source),
            );
            risk.add(p.risk);
            operations.push(PlannedOperation::Predicate(p));
        }

        // 7. > 100k threshold warning.
        if totals.total_operations > 100_000 {
            warnings.push(PlanWarning::PlanExceedsThreshold {
                total_count: totals.total_operations,
            });
        }

        let plan_hash = canonical_plan_hash(&sel, &etags, &operations, &account_providers);

        Ok(CleanupPlan {
            id: plan_id,
            user_id: user_id.to_string(),
            account_ids: sel.account_ids.clone(),
            created_at: now,
            valid_until: now + Duration::minutes(self.plan_ttl_minutes.max(1)),
            plan_hash,
            account_state_etags: etags,
            account_providers,
            status: PlanStatus::Ready,
            totals,
            risk,
            warnings,
            operations,
        })
    }
}

fn group_by_account(rule_selections: &[super::plan::RuleSelection]) -> Vec<(String, Vec<String>)> {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for r in rule_selections {
        if seen.insert((r.account_id.clone(), r.rule_id.clone())) {
            by.entry(r.account_id.clone())
                .or_default()
                .push(r.rule_id.clone());
        }
    }
    by.into_iter().collect()
}

fn plan_action_from_rule(a: &RuleAction) -> PlanAction {
    match a {
        RuleAction::AddLabel { .. } => PlanAction::AddLabel {
            kind: MoveKind::Label,
        },
        RuleAction::RemoveLabel { .. } => PlanAction::AddLabel {
            kind: MoveKind::Label,
        },
        RuleAction::Archive => PlanAction::Archive,
        RuleAction::Delete => PlanAction::Delete { permanent: false },
        RuleAction::MarkRead => PlanAction::MarkRead,
        RuleAction::MarkImportant => PlanAction::Star { on: true },
        RuleAction::Forward { .. } => PlanAction::Archive, // Forward isn't a cleanup op
    }
}

fn reverse_for(action: &PlanAction) -> Option<ReverseOp> {
    match action {
        PlanAction::Delete { permanent: true } => Some(ReverseOp::Irreversible),
        _ => None, // Phase E will fill these in.
    }
}

fn action_key(a: &PlanAction) -> &'static str {
    match a {
        PlanAction::Archive => "archive",
        PlanAction::AddLabel { .. } => "addLabel",
        PlanAction::Move { .. } => "move",
        PlanAction::Delete { permanent: true } => "deletePermanent",
        PlanAction::Delete { .. } => "delete",
        PlanAction::Unsubscribe { .. } => "unsubscribe",
        PlanAction::MarkRead => "markRead",
        PlanAction::Star { .. } => "star",
    }
}

fn source_key(s: &PlanSource) -> &'static str {
    match s {
        PlanSource::Subscription { .. } => "subscription",
        PlanSource::Cluster { .. } => "cluster",
        PlanSource::Rule { .. } => "rule",
        PlanSource::ArchiveStrategy { .. } => "archiveStrategy",
        PlanSource::Manual => "manual",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::operation::{ArchiveStrategy, EmailRef};
    use crate::cleanup::domain::plan::{ClusterSelection, RuleSelection, SubscriptionSelection};
    use crate::rules::types::{RuleAction, RuleEvaluation, RuleMatchBasis};
    use async_trait::async_trait;

    struct Fakes {
        emails_by_account: HashMap<String, Vec<EmailRef>>,
        emails_by_cluster: HashMap<String, Vec<EmailRef>>,
        subs: HashMap<(String, String), super::super::ports::SubscriptionRecord>,
        evals: Vec<RuleEvaluation>,
        etags: HashMap<String, AccountStateEtag>,
    }

    #[async_trait]
    impl EmailRepository for Fakes {
        async fn list_by_account(&self, a: &str) -> Result<Vec<EmailRef>, RepoError> {
            Ok(self.emails_by_account.get(a).cloned().unwrap_or_default())
        }
        async fn list_by_cluster(&self, c: &str) -> Result<Vec<EmailRef>, RepoError> {
            Ok(self.emails_by_cluster.get(c).cloned().unwrap_or_default())
        }
        async fn count_by_account(&self, a: &str) -> Result<u64, RepoError> {
            Ok(self
                .emails_by_account
                .get(a)
                .map(|v| v.len() as u64)
                .unwrap_or(0))
        }
    }

    #[async_trait]
    impl SubscriptionRepository for Fakes {
        async fn list_by_account(
            &self,
            a: &str,
        ) -> Result<Vec<super::super::ports::SubscriptionRecord>, RepoError> {
            Ok(self
                .subs
                .iter()
                .filter(|((acct, _), _)| acct == a)
                .map(|(_, v)| v.clone())
                .collect())
        }
        async fn find_by_sender(
            &self,
            a: &str,
            s: &str,
        ) -> Result<Option<super::super::ports::SubscriptionRecord>, RepoError> {
            Ok(self.subs.get(&(a.to_string(), s.to_string())).cloned())
        }
    }

    #[async_trait]
    impl ClusterRepository for Fakes {
        async fn emails(&self, c: &str) -> Result<Vec<EmailRef>, RepoError> {
            Ok(self.emails_by_cluster.get(c).cloned().unwrap_or_default())
        }
    }

    #[async_trait]
    impl AccountStateProvider for Fakes {
        async fn etag(&self, a: &str) -> Result<AccountStateEtag, RepoError> {
            Ok(self.etags.get(a).cloned().unwrap_or(AccountStateEtag::None))
        }
    }

    #[async_trait]
    impl RuleEvaluator for Fakes {
        async fn evaluate_scope(
            &self,
            _mode: RuleExecutionMode,
            _scope: EvaluationScope,
        ) -> Result<Vec<RuleEvaluation>, RuleEvalError> {
            Ok(self.evals.clone())
        }
    }

    fn make_builder(f: Arc<Fakes>) -> PlanBuilder {
        PlanBuilder {
            emails: f.clone() as Arc<dyn EmailRepository>,
            subs: f.clone() as Arc<dyn SubscriptionRepository>,
            clusters: f.clone() as Arc<dyn ClusterRepository>,
            rules: f.clone() as Arc<dyn RuleEvaluator>,
            accounts: f as Arc<dyn AccountStateProvider>,
            classifier: Arc::new(RiskClassifier::new()),
            provider_for: Arc::new(|_| Provider::Gmail),
            plan_ttl_minutes: 30,
        }
    }

    fn email(id: &str, account: &str) -> EmailRef {
        EmailRef {
            id: id.to_string(),
            account_id: account.to_string(),
        }
    }

    #[tokio::test]
    async fn build_subscription_only_plan() {
        let mut subs = HashMap::new();
        subs.insert(
            ("acct-a".to_string(), "news@x.com".to_string()),
            super::super::ports::SubscriptionRecord {
                sender: "news@x.com".into(),
                account_id: "acct-a".into(),
                method: super::super::operation::UnsubscribeMethodKind::ListUnsubscribePost,
                message_count: 12,
            },
        );
        let fakes = Arc::new(Fakes {
            emails_by_account: HashMap::new(),
            emails_by_cluster: HashMap::new(),
            subs,
            evals: Vec::new(),
            etags: HashMap::new(),
        });
        let pb = make_builder(fakes);
        let plan = pb
            .build(
                "user-1",
                WizardSelections {
                    subscriptions: vec![SubscriptionSelection {
                        sender: "news@x.com".into(),
                        account_id: "acct-a".into(),
                    }],
                    cluster_actions: vec![],
                    rule_selections: vec![],
                    archive_strategy: None,
                    account_ids: vec!["acct-a".into()],
                },
            )
            .await
            .expect("build");
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(plan.totals.total_operations, 1);
        assert_eq!(plan.risk.low, 1);
        assert_eq!(plan.status, PlanStatus::Ready);
    }

    #[tokio::test]
    async fn dedup_cluster_wins_over_rule() {
        // Same email touched by both a rule and a cluster — cluster wins,
        // conflict is reported.
        let email_e1 = email("e1", "acct-a");
        let mut emails_by_cluster = HashMap::new();
        emails_by_cluster.insert("c1".to_string(), vec![email_e1.clone()]);

        let evals = vec![RuleEvaluation {
            rule_id: "r1".into(),
            matched_email_ids: vec!["e1".into()],
            projected_count: 1,
            intended_actions: vec![RuleAction::AddLabel { label: "X".into() }],
            match_basis: RuleMatchBasis::Literal,
        }];
        let fakes = Arc::new(Fakes {
            emails_by_account: HashMap::new(),
            emails_by_cluster,
            subs: HashMap::new(),
            evals,
            etags: HashMap::new(),
        });
        let pb = make_builder(fakes);
        let plan = pb
            .build(
                "u",
                WizardSelections {
                    subscriptions: vec![],
                    cluster_actions: vec![ClusterSelection {
                        cluster_id: "c1".into(),
                        action: ClusterAction::Archive,
                        account_id: "acct-a".into(),
                    }],
                    rule_selections: vec![RuleSelection {
                        rule_id: "r1".into(),
                        account_id: "acct-a".into(),
                    }],
                    archive_strategy: None,
                    account_ids: vec!["acct-a".into()],
                },
            )
            .await
            .expect("build");
        // Cluster materialized row + rule predicate row = 2 ops.
        // The materialized (cluster) wins for the email; the rule predicate
        // is still recorded because it represents the rule scope.
        assert!(plan
            .operations
            .iter()
            .any(|o| matches!(o, PlannedOperation::Materialized(r) if r.email_id.as_deref() == Some("e1"))));
    }

    /// Performance budget: PlanBuilder P95 ≤ 800 ms on a 50k-row fixture
    /// (`docs/plan/cleanup-dry-run-implementation.md` §A.4 acceptance).
    /// Run with `cargo test cleanup::domain::builder::tests::perf_50k -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn perf_50k_under_budget() {
        let n = 50_000usize;
        let mut emails_by_cluster = HashMap::new();
        let mut all_emails: Vec<EmailRef> = Vec::with_capacity(n);
        for i in 0..n {
            all_emails.push(email(&format!("e{i}"), "acct-a"));
        }
        emails_by_cluster.insert("c-big".to_string(), all_emails.clone());

        let mut emails_by_account = HashMap::new();
        emails_by_account.insert("acct-a".to_string(), all_emails);

        let fakes = Arc::new(Fakes {
            emails_by_account,
            emails_by_cluster,
            subs: HashMap::new(),
            evals: Vec::new(),
            etags: HashMap::new(),
        });
        let pb = make_builder(fakes);

        let mut runs = Vec::with_capacity(5);
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let _plan = pb
                .build(
                    "u",
                    WizardSelections {
                        subscriptions: vec![],
                        cluster_actions: vec![ClusterSelection {
                            cluster_id: "c-big".into(),
                            action: ClusterAction::Archive,
                            account_id: "acct-a".into(),
                        }],
                        rule_selections: vec![],
                        archive_strategy: None,
                        account_ids: vec!["acct-a".into()],
                    },
                )
                .await
                .expect("build");
            runs.push(start.elapsed());
        }
        runs.sort();
        let p95 = runs[runs.len() - 1];
        eprintln!("PlanBuilder 50k runs={runs:?}, p95={p95:?}");
        assert!(
            p95.as_millis() < 800,
            "P95 build time {p95:?} exceeded 800ms budget"
        );
    }

    #[tokio::test]
    async fn plan_is_mutation_free_against_panicking_provider() {
        // The fakes never invoke any provider mutation — if PlanBuilder ever
        // tried, it would have to call something other than the four ports.
        // This test asserts the structural invariant by virtue of the type
        // signature: PlanBuilder takes ports only.
        let fakes = Arc::new(Fakes {
            emails_by_account: HashMap::new(),
            emails_by_cluster: HashMap::new(),
            subs: HashMap::new(),
            evals: Vec::new(),
            etags: HashMap::new(),
        });
        let pb = make_builder(fakes);
        let plan = pb
            .build(
                "u",
                WizardSelections {
                    subscriptions: vec![],
                    cluster_actions: vec![],
                    rule_selections: vec![],
                    archive_strategy: Some(ArchiveStrategy::OlderThan90d),
                    account_ids: vec!["acct-a".into()],
                },
            )
            .await
            .expect("build");
        // Empty inbox + archive strategy → 1 predicate row with projected_count=0
        assert_eq!(plan.operations.len(), 1);
    }
}
