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
    ArchiveStrategy, OperationStatus, PlanAction, PlanSource, PlannedOperationPredicate,
    PlannedOperationRow, PredicateKind,
};
use crate::cleanup::domain::ports::{EmailRepository, RepoError, RuleEvalError, RuleEvaluator};

/// Maximum page size enforced by the expander itself (ADR-030 §C.1 step 6).
pub const MAX_PAGE_SIZE: u32 = 1000;

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error("repo: {0}")]
    Repo(#[from] RepoError),
    #[error("not implemented for predicate kind {0}")]
    NotImplemented(&'static str),
    #[error("rule: {0}")]
    Rule(#[from] RuleEvalError),
    #[error("invalid predicate: {0}")]
    InvalidPredicate(&'static str),
}

pub struct PredicateExpander {
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
    /// the starting value. `as_of` is the fixed UTC apply-time reference for
    /// all pages of one expansion; archive strategies use rolling 30/90/365-day
    /// age predicates at execution time, not the plan-preview timestamp.
    pub async fn expand_page(
        &self,
        as_of: chrono::DateTime<chrono::Utc>,
        predicate: &PlannedOperationPredicate,
        page: u32,
        page_size: u32,
    ) -> Result<Vec<PlannedOperationRow>, ExpandError> {
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
        let emails = match (&predicate.predicate_kind, &predicate.source) {
            (PredicateKind::ArchiveStrategy, PlanSource::ArchiveStrategy { strategy }) => {
                if predicate.action != PlanAction::Archive
                    || predicate.predicate_id != format!("{strategy:?}")
                {
                    return Err(ExpandError::InvalidPredicate(
                        "archive source, action and strategy disagree",
                    ));
                }
                let days = match strategy {
                    ArchiveStrategy::OlderThan30d => 30,
                    ArchiveStrategy::OlderThan90d => 90,
                    ArchiveStrategy::OlderThan1y => 365,
                    ArchiveStrategy::Custom => {
                        return Err(ExpandError::NotImplemented("custom archive strategy"))
                    }
                };
                self.emails
                    .archive_candidates(
                        &predicate.account_id,
                        as_of - chrono::Duration::days(days),
                        page,
                        page_size,
                    )
                    .await?
            }
            (
                PredicateKind::Rule,
                PlanSource::Rule {
                    rule_id,
                    match_basis,
                },
            ) if !rule_id.is_empty()
                && rule_id == &predicate.predicate_id
                && match_basis == "literal" =>
            {
                let expected = predicate.constraint_fingerprint.as_deref().ok_or(
                    ExpandError::InvalidPredicate(
                        "rule predicate has no approved constraint binding",
                    ),
                )?;
                let matched = self
                    .rules
                    .matching_page(&predicate.account_id, rule_id, page, page_size)
                    .await?;
                if matched.constraint_fingerprint != expected {
                    return Err(ExpandError::InvalidPredicate(
                        "selected rule constraints changed since plan approval",
                    ));
                }
                use crate::rules::types::RuleAction;
                // The planner records the first action. Do not reinterpret
                // unsupported or changed actions as an archive at apply time.
                let action_matches = match (matched.actions.first(), &predicate.action) {
                    (Some(RuleAction::Archive), PlanAction::Archive)
                    | (Some(RuleAction::MarkRead), PlanAction::MarkRead)
                    | (Some(RuleAction::MarkImportant), PlanAction::Star { on: true }) => true,
                    (
                        Some(RuleAction::Delete { permanent: rule }),
                        PlanAction::Delete { permanent: planned },
                    ) => rule == planned,
                    _ => false,
                };
                if !action_matches {
                    return Err(ExpandError::InvalidPredicate(
                        "selected rule action is unsupported or changed",
                    ));
                }
                matched.emails
            }
            (PredicateKind::LabelFilter, _) => {
                return Err(ExpandError::NotImplemented("label filter"))
            }
            _ => {
                return Err(ExpandError::InvalidPredicate(
                    "missing or inconsistent predicate constraints",
                ))
            }
        };
        if emails
            .iter()
            .any(|email| email.account_id != predicate.account_id)
        {
            return Err(ExpandError::InvalidPredicate(
                "eligibility query returned a different account",
            ));
        }

        let rows = emails
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
        async fn archive_candidates(
            &self,
            account_id: &str,
            _before: chrono::DateTime<chrono::Utc>,
            page: u32,
            page_size: u32,
        ) -> Result<Vec<EmailRef>, RepoError> {
            Ok(self
                .emails
                .iter()
                .filter(|email| email.account_id == account_id)
                .skip((u64::from(page) * u64::from(page_size)) as usize)
                .take(page_size as usize)
                .cloned()
                .collect())
        }

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
            constraint_fingerprint: None,
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
        let pred = safety_archive(ArchiveStrategy::OlderThan30d);
        let page = expander
            .expand_page(chrono::Utc::now(), &pred, 10, 100)
            .await
            .unwrap();
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
        let pred = safety_archive(ArchiveStrategy::OlderThan30d);
        let page = expander
            .expand_page(chrono::Utc::now(), &pred, 0, 100_000)
            .await
            .unwrap();
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
        let pred = safety_archive(ArchiveStrategy::OlderThan30d);
        let mut total_rows: u64 = 0;
        let page_size = 1000u32;
        for page in 0..20 {
            let rows = expander
                .expand_page(chrono::Utc::now(), &pred, page, page_size)
                .await
                .unwrap();
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
    async fn safety_fixture() -> (crate::db::Database, PredicateExpander) {
        let db = crate::db::test_sqlite_database().await;
        crate::db::apply_sqlite_migrations(
            &db.sea_orm(),
            &[
                include_str!("../../../migrations/sqlite/001_initial_schema.sql"),
                include_str!("../../../migrations/sqlite/004_accounts.sql"),
                include_str!("../../../migrations/sqlite/012_rules.sql"),
                include_str!("../../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
                include_str!("../../../migrations/sqlite/018_unsubscribe_headers.sql"),
                include_str!("../../../migrations/sqlite/021_thread_key.sql"),
                include_str!("../../../migrations/sqlite/026_rules_match_count.sql"),
                include_str!("../../../migrations/sqlite/027_is_archived.sql"),
            ],
        )
        .await
        .expect("migrate");
        let expander = PredicateExpander::new(
            Arc::new(crate::cleanup::repository::adapters::SeaOrmRuleEvaluator { db: db.clone() }),
            Arc::new(
                crate::cleanup::repository::adapters::SeaOrmEmailRepository { db: db.clone() },
            ),
        );
        (db, expander)
    }

    async fn safety_seed(
        db: &crate::db::Database,
        id: &str,
        account: &str,
        age_days: i64,
        read: bool,
        subject: &str,
    ) {
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        crate::db::entities::emails::ActiveModel {
            id: Set(id.into()),
            account_id: Set(account.into()),
            provider: Set("gmail".into()),
            subject: Set(subject.into()),
            from_addr: Set("sender@example.test".into()),
            to_addrs: Set("owner@example.test".into()),
            received_at: Set((chrono::Utc::now() - chrono::Duration::days(age_days)).naive_utc()),
            is_read: Set(Some(read)),
            is_archived: Set(false),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".into()),
            ..Default::default()
        }
        .insert(&db.sea_orm())
        .await
        .expect("seed");
    }

    fn safety_archive(
        strategy: crate::cleanup::domain::operation::ArchiveStrategy,
    ) -> PlannedOperationPredicate {
        let mut p = make_predicate("acct");
        p.source = PlanSource::ArchiveStrategy { strategy };
        p.predicate_id = format!("{strategy:?}");
        p
    }

    async fn safety_rule(
        db: &crate::db::Database,
        condition: crate::rules::types::RuleCondition,
        actions: Vec<crate::rules::types::RuleAction>,
    ) -> PlannedOperationPredicate {
        let now = chrono::Utc::now();
        let rule = crate::rules::types::Rule {
            id: "selected-rule".into(),
            name: "Selected".into(),
            description: String::new(),
            conditions: vec![condition],
            actions,
            priority: 0,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        crate::rules::rule_engine::RuleEngine::save_rule(db, &rule)
            .await
            .expect("rule");
        let mut p = make_predicate("acct");
        p.predicate_kind = PredicateKind::Rule;
        let bound = crate::cleanup::repository::adapters::SeaOrmRuleEvaluator { db: db.clone() }
            .evaluate_bound_scope(
                crate::rules::types::RuleExecutionMode::EvaluateOnly,
                crate::rules::types::EvaluationScope {
                    account_id: "acct".into(),
                    rule_ids: vec![rule.id.clone()],
                    sample_size: 20,
                },
            )
            .await
            .unwrap();
        p.constraint_fingerprint = Some(bound[0].constraint_fingerprint.clone());
        p.predicate_id = rule.id.clone();
        p.source = PlanSource::Rule {
            rule_id: rule.id,
            match_basis: "literal".into(),
        };
        p
    }

    #[tokio::test]
    async fn safety_archive_selects_actual_age_across_read_states_and_accounts() {
        use crate::cleanup::domain::operation::ArchiveStrategy::*;
        let (db, expander) = safety_fixture().await;
        for (id, age, read) in [
            ("recent", 10, false),
            ("old-read", 60, true),
            ("older-unread", 120, false),
            ("year-old", 400, true),
        ] {
            safety_seed(&db, id, "acct", age, read, "message").await;
        }
        safety_seed(&db, "other-account", "other", 400, false, "message").await;
        for (strategy, expected) in [
            (OlderThan30d, vec!["year-old", "older-unread", "old-read"]),
            (OlderThan90d, vec!["year-old", "older-unread"]),
            (OlderThan1y, vec!["year-old"]),
        ] {
            let rows = expander
                .expand_page(chrono::Utc::now(), &safety_archive(strategy), 0, 100)
                .await
                .expect("expand");
            let ids: Vec<_> = rows
                .iter()
                .map(|r| r.email_id.as_deref().unwrap())
                .collect();
            assert_eq!(ids, expected, "strategy {strategy:?}");
            assert!(rows.iter().all(|r| r.account_id == "acct"));
        }
    }

    #[tokio::test]
    async fn safety_archive_excludes_inactive_messages_and_paginates_eligible_rows() {
        use crate::cleanup::domain::operation::ArchiveStrategy;
        use crate::db::entities::emails;
        use sea_orm::sea_query::Expr;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let (db, expander) = safety_fixture().await;
        for id in [
            "old-0", "old-1", "old-2", "archived", "deleted", "trash", "spam",
        ] {
            safety_seed(&db, id, "acct", 60, false, "message").await;
        }
        safety_seed(&db, "recent", "acct", 1, true, "message").await;
        for (id, column, value) in [
            (
                "archived",
                emails::Column::IsArchived,
                sea_orm::Value::from(true),
            ),
            (
                "deleted",
                emails::Column::DeletedAt,
                sea_orm::Value::from("2026-01-01"),
            ),
            ("trash", emails::Column::IsTrash, sea_orm::Value::from(1)),
            ("spam", emails::Column::IsSpam, sea_orm::Value::from(1)),
        ] {
            emails::Entity::update_many()
                .col_expr(column, Expr::value(value))
                .filter(emails::Column::Id.eq(id))
                .exec(&db.sea_orm())
                .await
                .expect("mark inactive");
        }
        let pred = safety_archive(ArchiveStrategy::OlderThan30d);
        let mut ids = Vec::new();
        for page in 0..3 {
            ids.extend(
                expander
                    .expand_page(chrono::Utc::now(), &pred, page, 2)
                    .await
                    .expect("page")
                    .into_iter()
                    .map(|r| r.email_id.unwrap()),
            );
        }
        assert_eq!(ids, vec!["old-0", "old-1", "old-2"]);
    }

    #[tokio::test]
    async fn safety_rule_expands_full_matched_set_beyond_preview_sample() {
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        for i in 0..25 {
            safety_seed(
                &db,
                &format!("match-{i:02}"),
                "acct",
                60,
                i % 2 == 0,
                "promo message",
            )
            .await;
            if i % 5 == 0 {
                safety_seed(
                    &db,
                    &format!("nonmatch-{i}"),
                    "acct",
                    60,
                    false,
                    "personal message",
                )
                .await;
            }
        }
        safety_seed(&db, "other-account", "other", 60, false, "promo message").await;
        let pred = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "promo".into(),
            },
            vec![RuleAction::Archive],
        )
        .await;
        let mut ids = Vec::new();
        for page in 0..5 {
            let rows = expander
                .expand_page(chrono::Utc::now(), &pred, page, 7)
                .await
                .expect("page");
            assert!(rows.len() <= 7);
            ids.extend(rows.into_iter().map(|r| r.email_id.unwrap()));
        }
        assert_eq!(
            ids,
            (0..25).map(|i| format!("match-{i:02}")).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn safety_expansion_rejects_unsupported_or_missing_predicate_constraints() {
        use crate::cleanup::domain::operation::ArchiveStrategy;
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "must-not-expand", "acct", 400, false, "message").await;
        let mut label = make_predicate("acct");
        label.predicate_kind = PredicateKind::LabelFilter;
        let mut missing_rule = make_predicate("acct");
        missing_rule.predicate_kind = PredicateKind::Rule;
        for predicate in [
            make_predicate("acct"),
            label,
            missing_rule,
            safety_archive(ArchiveStrategy::Custom),
        ] {
            assert!(
                expander
                    .expand_page(chrono::Utc::now(), &predicate, 0, 100)
                    .await
                    .is_err(),
                "must reject {:?}",
                predicate
            );
        }
    }

    #[tokio::test]
    async fn safety_rule_rejects_semantic_negation_and_changed_action() {
        use crate::rules::types::{RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "must-not-expand", "acct", 60, false, "message").await;
        let pred = safety_rule(
            &db,
            RuleCondition::Not {
                condition: Box::new(RuleCondition::Semantic {
                    query: "sensitive".into(),
                    threshold: 0.75,
                }),
            },
            vec![RuleAction::Archive],
        )
        .await;
        assert!(
            expander
                .expand_page(chrono::Utc::now(), &pred, 0, 100)
                .await
                .is_err(),
            "semantic eligibility cannot be guessed offline"
        );
        let pred = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: crate::rules::types::EmailField::Subject,
                operator: crate::rules::types::MatchOperator::Contains,
                value: "message".into(),
            },
            vec![RuleAction::MarkRead],
        )
        .await;
        assert!(
            expander
                .expand_page(chrono::Utc::now(), &pred, 0, 100)
                .await
                .is_err(),
            "current rule no longer authorizes planned archive action"
        );
    }
    #[tokio::test]
    async fn safety_rule_rejects_invalid_negated_regex() {
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "must-not-expand", "acct", 60, false, "message").await;
        let pred = safety_rule(
            &db,
            RuleCondition::Not {
                condition: Box::new(RuleCondition::FieldMatch {
                    field: EmailField::Subject,
                    operator: MatchOperator::Regex,
                    value: "[".into(),
                }),
            },
            vec![RuleAction::Archive],
        )
        .await;
        assert!(
            expander
                .expand_page(chrono::Utc::now(), &pred, 0, 100)
                .await
                .is_err(),
            "an invalid regex under NOT cannot authorize an archive"
        );
    }
    async fn safety_check_cutoff_formats(db: &crate::db::Database, expander: &PredicateExpander) {
        use crate::cleanup::domain::operation::ArchiveStrategy;
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let cutoff = chrono::DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let conn = db.sea_orm();
        let backend = conn.get_database_backend();
        let account = format!("cutoff-{}", uuid::Uuid::now_v7());
        for (suffix, timestamp) in [
            ("naive-before", "2026-01-01 11:59:59"),
            ("rfc-before", "2026-01-01T11:59:59Z"),
            ("naive-equal", "2026-01-01 12:00:00"),
            ("rfc-equal", "2026-01-01T12:00:00Z"),
            ("naive-after", "2026-01-01 12:00:01"),
            ("rfc-after", "2026-01-01T12:00:01Z"),
        ] {
            let id = format!("{account}-{suffix}");
            safety_seed(db, &id, &account, 60, false, "message").await;
            let sql = if backend == DbBackend::Postgres {
                "UPDATE emails SET received_at = $1::timestamp WHERE id = $2"
            } else {
                "UPDATE emails SET received_at = ? WHERE id = ?"
            };
            conn.execute_raw(Statement::from_sql_and_values(
                backend,
                sql,
                [timestamp.into(), id.into()],
            ))
            .await
            .unwrap();
        }
        let mut predicate = safety_archive(ArchiveStrategy::OlderThan30d);
        predicate.account_id = account.clone();
        let rows = expander
            .expand_page(cutoff + chrono::Duration::days(30), &predicate, 0, 100)
            .await
            .unwrap();
        let ids: Vec<_> = rows
            .iter()
            .map(|row| row.email_id.clone().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                format!("{account}-naive-before"),
                format!("{account}-rfc-before")
            ],
            "compare instants, not timestamp encodings"
        );
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        crate::db::entities::emails::Entity::delete_many()
            .filter(crate::db::entities::emails::Column::AccountId.eq(account))
            .exec(&conn)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn safety_review_cutoff_handles_mixed_sqlite_timestamp_formats() {
        let (db, expander) = safety_fixture().await;
        safety_check_cutoff_formats(&db, &expander).await;
    }

    #[tokio::test]
    #[ignore = "requires isolated EMAILIBRIUM_TEST_PG_URL with migrated schema"]
    async fn safety_review_cutoff_handles_postgres_timestamp_formats() {
        let url = std::env::var("EMAILIBRIUM_TEST_PG_URL")
            .expect("isolated PostgreSQL test database required");
        let db = crate::db::Database::connect(&url).await.unwrap();
        let expander = PredicateExpander::new(
            Arc::new(crate::cleanup::repository::adapters::SeaOrmRuleEvaluator { db: db.clone() }),
            Arc::new(
                crate::cleanup::repository::adapters::SeaOrmEmailRepository { db: db.clone() },
            ),
        );
        safety_check_cutoff_formats(&db, &expander).await;
    }

    #[tokio::test]
    async fn safety_review_preserves_exact_protected_source_markers() {
        use crate::cleanup::domain::operation::ArchiveStrategy;
        use crate::db::entities::emails;
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        use sea_orm::sea_query::Expr;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let (db, expander) = safety_fixture().await;
        for (id, folder, labels) in [
            ("folder-sent", "Sent Items", None),
            ("imap-sent", "[Gmail]/Sent Mail", None),
            ("folder-drafts", "Drafts", None),
            ("folder-outbox", "Outbox", None),
            ("folder-chats", "Chats", None),
            ("label-sent", "INBOX", Some("[\"SENT\",\"Topic/Mail\"]")),
            ("label-draft", "INBOX", Some("DRAFT, Topic/Mail")),
            ("malformed-labels", "INBOX", Some("[not valid json")),
            (
                "near-label",
                "INBOX",
                Some("[\"Topic/Sentimental\",\"Chatbots\"]"),
            ),
        ] {
            safety_seed(&db, id, "acct", 60, false, "promo message").await;
            emails::Entity::update_many()
                .col_expr(emails::Column::Folder, Expr::value(folder))
                .col_expr(emails::Column::Labels, Expr::value(labels))
                .filter(emails::Column::Id.eq(id))
                .exec(&db.sea_orm())
                .await
                .unwrap();
        }
        let rule = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "promo".into(),
            },
            vec![RuleAction::Archive],
        )
        .await;
        for predicate in [safety_archive(ArchiveStrategy::OlderThan30d), rule] {
            let rows = expander
                .expand_page(chrono::Utc::now(), &predicate, 0, 100)
                .await
                .unwrap();
            let ids: Vec<_> = rows
                .iter()
                .map(|r| r.email_id.as_deref().unwrap())
                .collect();
            assert_eq!(
                ids,
                vec!["near-label"],
                "protected source markers apply to every candidate path"
            );
        }
    }
    #[tokio::test]
    async fn safety_review_rule_rejects_changed_conditions_with_same_action() {
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "approved-match", "acct", 60, false, "promo message").await;
        safety_seed(&db, "not-approved", "acct", 60, false, "personal message").await;
        let predicate = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "promo".into(),
            },
            vec![RuleAction::Archive],
        )
        .await;
        let before = expander
            .expand_page(chrono::Utc::now(), &predicate, 0, 100)
            .await
            .unwrap();
        assert_eq!(
            before
                .iter()
                .map(|r| r.email_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["approved-match"]
        );
        let mut edited = crate::rules::rule_engine::RuleEngine::get_rule(&db, "selected-rule")
            .await
            .unwrap()
            .unwrap();
        edited.conditions = vec![RuleCondition::FieldMatch {
            field: EmailField::Subject,
            operator: MatchOperator::Contains,
            value: "message".into(),
        }];
        crate::rules::rule_engine::RuleEngine::save_rule(&db, &edited)
            .await
            .unwrap();
        assert!(
            expander
                .expand_page(chrono::Utc::now(), &predicate, 0, 100)
                .await
                .is_err(),
            "same action does not authorize a broader condition set than the reviewed predicate"
        );
    }

    #[tokio::test]
    async fn safety_review_legacy_rule_without_constraint_binding_is_rejected() {
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "must-not-expand", "acct", 60, false, "promo message").await;
        let predicate = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "promo".into(),
            },
            vec![RuleAction::Archive],
        )
        .await;
        let mut stored = serde_json::to_value(predicate).unwrap();
        stored
            .as_object_mut()
            .unwrap()
            .remove("constraintFingerprint");
        let legacy = serde_json::from_value(stored).unwrap();
        assert!(expander
            .expand_page(chrono::Utc::now(), &legacy, 0, 100)
            .await
            .is_err());
    }
    #[tokio::test]
    async fn safety_review_builder_carries_bound_rule_snapshot_into_expansion() {
        use crate::cleanup::domain::builder::PlanBuilder;
        use crate::cleanup::domain::plan::{RuleSelection, WizardSelections};
        use crate::cleanup::repository::adapters::*;
        use crate::rules::types::{EmailField, MatchOperator, RuleAction, RuleCondition};
        let (db, expander) = safety_fixture().await;
        safety_seed(&db, "approved-match", "acct", 60, false, "promo message").await;
        let _ = safety_rule(
            &db,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "promo".into(),
            },
            vec![RuleAction::Archive],
        )
        .await;
        let builder = PlanBuilder {
            emails: Arc::new(SeaOrmEmailRepository { db: db.clone() }),
            subs: Arc::new(SeaOrmSubscriptionRepository { db: db.clone() }),
            clusters: Arc::new(SeaOrmClusterRepository { db: db.clone() }),
            rules: Arc::new(SeaOrmRuleEvaluator { db: db.clone() }),
            accounts: Arc::new(SeaOrmAccountStateProvider { db: db.clone() }),
            classifier: Arc::new(crate::cleanup::domain::classifier::RiskClassifier::new()),
            provider_for: Arc::new(|_| crate::cleanup::domain::operation::Provider::Gmail),
            plan_ttl_minutes: 30,
        };
        let plan = builder
            .build(
                "owner",
                WizardSelections {
                    account_ids: vec!["acct".into()],
                    rule_selections: vec![RuleSelection {
                        rule_id: "selected-rule".into(),
                        account_id: "acct".into(),
                    }],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let crate::cleanup::domain::operation::PlannedOperation::Predicate(predicate) =
            &plan.operations[0]
        else {
            panic!("rule predicate required");
        };
        assert!(predicate.constraint_fingerprint.is_some());
        let rows = expander
            .expand_page(chrono::Utc::now(), predicate, 0, 100)
            .await
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.email_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["approved-match"]
        );
    }
}
