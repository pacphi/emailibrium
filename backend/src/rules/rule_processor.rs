//! Rule processor -- evaluates rules against emails and generates pending actions (R-03).

use super::types::{
    EmailField, EvaluationScope, MatchOperator, PendingAction, Rule, RuleAction, RuleCondition,
    RuleEvaluation, RuleExecutionMode,
};
use crate::email::types::EmailMessage;
use regex::Regex;

/// Evaluate whether a single rule matches a given email.
///
/// Returns `true` when ALL top-level conditions are satisfied (implicit AND).
/// Semantic conditions always return `false` in this offline evaluator because
/// they require a vector service; the engine layer handles semantic matching.
pub fn evaluate_rule(rule: &Rule, email: &EmailMessage) -> bool {
    if !rule.enabled {
        return false;
    }
    rule.conditions.iter().all(|c| evaluate_condition(c, email))
}

/// Generate pending actions from a list of rule actions without executing them.
pub fn apply_actions(
    actions: &[RuleAction],
    email_id: &str,
    rule_id: &str,
    rule_name: &str,
) -> Vec<PendingAction> {
    actions
        .iter()
        .map(|action| PendingAction {
            email_id: email_id.to_string(),
            rule_id: rule_id.to_string(),
            rule_name: rule_name.to_string(),
            action: action.clone(),
        })
        .collect()
}

/// Non-mutating, scope-based evaluation used by the cleanup PlanBuilder
/// (DDD-007 addendum, ADR-030 Phase A).
///
/// Runs the same matcher as `process_email` but, in `EvaluateOnly` mode,
/// returns one `RuleEvaluation` per matched rule **without emitting any
/// commands**. In `Apply` mode the function still returns the same
/// `RuleEvaluation` shape (used by integration tests that need to assert
/// "EvaluateOnly is equivalent to Apply minus side-effects"); production
/// `Apply` callers continue to use `process_email`.
///
/// Determinism contract for `matched_email_ids` (required for `plan_hash`):
/// emails are first sorted by `(date_asc, id_asc)`, then sampled as
/// `head 5 + tail 5 + 10 stratified by date index`, deduplicated while
/// preserving order, and finally capped at `min(scope.sample_size, 20)`.
pub fn evaluate_rules(
    mode: RuleExecutionMode,
    rules: &[Rule],
    emails: &[EmailMessage],
    scope: &EvaluationScope,
) -> Vec<RuleEvaluation> {
    // Filter to enabled rules; if scope.rule_ids is non-empty, intersect.
    let selected: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.enabled)
        .filter(|r| scope.rule_ids.is_empty() || scope.rule_ids.iter().any(|id| id == &r.id))
        .collect();

    let cap = scope.sample_size.min(20) as usize;

    // Pre-sort emails deterministically. We do not mutate the caller's slice.
    let mut sorted: Vec<&EmailMessage> = emails.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));

    let mut evaluations = Vec::with_capacity(selected.len());

    for rule in selected {
        let matched: Vec<&EmailMessage> = sorted
            .iter()
            .copied()
            .filter(|e| evaluate_rule(rule, e))
            .collect();

        let projected_count = matched.len() as u64;
        let sampled = sample_deterministic(&matched, cap);
        let basis = RuleEvaluation::basis_for(&rule.conditions);

        // Side-effect contract: `Apply` is identical in this function (it does
        // NOT emit commands). EvaluateOnly is the same path. Emission of
        // `PendingAction`s in `Apply` mode is the responsibility of the
        // existing `process_email` pipeline, which Plan never invokes.
        let _ = mode; // both modes produce the same evaluation; mode kept for API symmetry

        evaluations.push(RuleEvaluation {
            rule_id: rule.id.clone(),
            matched_email_ids: sampled,
            projected_count,
            intended_actions: rule.actions.clone(),
            match_basis: basis,
        });
    }

    evaluations
}

/// Deterministic sampling: head 5 + tail 5 + 10 stratified by index across the
/// middle, deduplicated, capped at `cap` (≤ 20).
fn sample_deterministic(matched: &[&EmailMessage], cap: usize) -> Vec<String> {
    if matched.is_empty() || cap == 0 {
        return Vec::new();
    }
    let n = matched.len();
    if n <= cap {
        return matched.iter().map(|e| e.id.clone()).collect();
    }

    let mut picked: Vec<usize> = Vec::with_capacity(cap);
    let head = 5.min(cap);
    let tail = 5.min(cap.saturating_sub(head));
    let middle = cap.saturating_sub(head + tail);

    for i in 0..head {
        picked.push(i);
    }
    if middle > 0 && n > head + tail {
        // stratified: evenly spaced indices in (head .. n - tail)
        let lo = head;
        let hi = n - tail;
        let span = hi - lo;
        for k in 0..middle {
            // distribute k across [lo, hi)
            let idx = lo + (k * span) / middle;
            picked.push(idx);
        }
    }
    for i in 0..tail {
        picked.push(n - 1 - i);
    }

    // Sort + dedupe while preserving index order.
    picked.sort_unstable();
    picked.dedup();
    picked.iter().map(|i| matched[*i].id.clone()).collect()
}

/// Evaluate all rules (sorted by descending priority) against an email,
/// collecting pending actions.
pub fn process_email(rules: &[Rule], email: &EmailMessage) -> Vec<PendingAction> {
    let mut sorted: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.priority));

    let mut pending = Vec::new();
    for rule in sorted {
        if evaluate_rule(rule, email) {
            let actions = apply_actions(&rule.actions, &email.id, &rule.id, &rule.name);
            pending.extend(actions);
        }
    }
    pending
}

// ---------------------------------------------------------------------------
// Condition evaluation
// ---------------------------------------------------------------------------

fn evaluate_condition(cond: &RuleCondition, email: &EmailMessage) -> bool {
    match cond {
        RuleCondition::FieldMatch {
            field,
            operator,
            value,
        } => evaluate_field_match(field, operator, value, email),

        // Semantic conditions need a vector service -- skip in offline evaluation.
        RuleCondition::Semantic { .. } => false,

        RuleCondition::And { conditions } => {
            conditions.iter().all(|c| evaluate_condition(c, email))
        }
        RuleCondition::Or { conditions } => conditions.iter().any(|c| evaluate_condition(c, email)),
        RuleCondition::Not { condition } => !evaluate_condition(condition, email),
    }
}

fn evaluate_field_match(
    field: &EmailField,
    operator: &MatchOperator,
    value: &str,
    email: &EmailMessage,
) -> bool {
    let field_value = get_field_value(field, email);
    apply_operator(operator, &field_value, value)
}

fn get_field_value(field: &EmailField, email: &EmailMessage) -> String {
    match field {
        EmailField::From => email.from.clone(),
        EmailField::To => email.to.join(", "),
        EmailField::Subject => email.subject.clone(),
        EmailField::Body => email.body.clone().unwrap_or_default(),
        EmailField::Labels => email.labels.join(", "),
        EmailField::Date => email.date.to_rfc3339(),
    }
}

fn apply_operator(op: &MatchOperator, field_value: &str, match_value: &str) -> bool {
    let fv = field_value.to_lowercase();
    let mv = match_value.to_lowercase();

    match op {
        MatchOperator::Contains => fv.contains(&mv),
        MatchOperator::Equals => fv == mv,
        MatchOperator::StartsWith => fv.starts_with(&mv),
        MatchOperator::EndsWith => fv.ends_with(&mv),
        MatchOperator::Regex => Regex::new(match_value)
            .map(|re| re.is_match(field_value))
            .unwrap_or(false),
        MatchOperator::GreaterThan => fv > mv,
        MatchOperator::LessThan => fv < mv,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_email() -> EmailMessage {
        EmailMessage {
            id: "msg-001".to_string(),
            thread_id: None,
            from: "boss@company.com".to_string(),
            to: vec!["me@company.com".to_string()],
            subject: "Quarterly Budget Review".to_string(),
            snippet: "Please review...".to_string(),
            body: Some("Please review the attached budget.".to_string()),
            body_html: None,
            labels: vec!["inbox".to_string(), "important".to_string()],
            date: Utc::now(),
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        }
    }

    fn make_rule(id: &str, conditions: Vec<RuleCondition>, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: id.to_string(),
            name: format!("Rule {id}"),
            description: String::new(),
            conditions,
            actions,
            priority: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn evaluate_from_contains() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_from_not_matching() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "stranger".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(!evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_subject_equals() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Equals,
                value: "quarterly budget review".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_regex() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Regex,
                value: r"(?i)budget\s+review".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_labels_contains() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Contains,
                value: "important".to_string(),
            }],
            vec![RuleAction::Archive],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_or_condition() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::Or {
                conditions: vec![
                    RuleCondition::FieldMatch {
                        field: EmailField::From,
                        operator: MatchOperator::Contains,
                        value: "stranger".to_string(),
                    },
                    RuleCondition::FieldMatch {
                        field: EmailField::Subject,
                        operator: MatchOperator::Contains,
                        value: "budget".to_string(),
                    },
                ],
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn evaluate_not_condition() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::Not {
                condition: Box::new(RuleCondition::FieldMatch {
                    field: EmailField::From,
                    operator: MatchOperator::Contains,
                    value: "spam".to_string(),
                }),
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn semantic_condition_returns_false_offline() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::Semantic {
                query: "budget".to_string(),
                threshold: 0.5,
            }],
            vec![RuleAction::MarkRead],
        );
        assert!(!evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn disabled_rule_not_evaluated() {
        let mut rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        rule.enabled = false;
        assert!(!evaluate_rule(&rule, &sample_email()));
    }

    #[test]
    fn apply_actions_generates_pending() {
        let actions = vec![
            RuleAction::AddLabel {
                label: "work".to_string(),
            },
            RuleAction::MarkRead,
        ];
        let pending = apply_actions(&actions, "msg-001", "r1", "Rule r1");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].email_id, "msg-001");
        assert_eq!(pending[0].rule_id, "r1");
    }

    fn email_at(id: &str, date: chrono::DateTime<Utc>, from: &str, subject: &str) -> EmailMessage {
        EmailMessage {
            id: id.to_string(),
            thread_id: None,
            from: from.to_string(),
            to: vec!["me@x.com".to_string()],
            subject: subject.to_string(),
            snippet: String::new(),
            body: None,
            body_html: None,
            labels: vec![],
            date,
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        }
    }

    #[test]
    fn evaluate_rules_evaluate_only_returns_same_matched_set_as_apply() {
        // EvaluateOnly MUST be equivalent to Apply minus the command emission.
        let rules = vec![make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::Archive],
        )];
        let now = Utc::now();
        let emails = vec![
            email_at("a", now, "boss@x.com", "hi"),
            email_at("b", now, "noise@x.com", "hi"),
        ];
        let scope = EvaluationScope {
            account_id: "acct1".to_string(),
            rule_ids: vec![],
            sample_size: 20,
        };

        let evals_apply = evaluate_rules(RuleExecutionMode::Apply, &rules, &emails, &scope);
        let evals_eval = evaluate_rules(RuleExecutionMode::EvaluateOnly, &rules, &emails, &scope);

        assert_eq!(evals_apply.len(), evals_eval.len());
        assert_eq!(
            evals_apply[0].matched_email_ids,
            evals_eval[0].matched_email_ids
        );
        assert_eq!(evals_apply[0].projected_count, 1);
        assert_eq!(evals_eval[0].projected_count, 1);
        assert_eq!(evals_eval[0].matched_email_ids, vec!["a".to_string()]);
    }

    #[test]
    fn evaluate_rules_caps_sample_at_20_and_is_deterministic() {
        let rules = vec![make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::MarkRead],
        )];
        let base = chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let emails: Vec<EmailMessage> = (0..100)
            .map(|i| {
                let d = base + chrono::Duration::seconds(i);
                email_at(&format!("e{i:03}"), d, "boss@x.com", "x")
            })
            .collect();
        let scope = EvaluationScope {
            account_id: "acct".to_string(),
            rule_ids: vec![],
            sample_size: 20,
        };

        let r1 = evaluate_rules(RuleExecutionMode::EvaluateOnly, &rules, &emails, &scope);
        let r2 = evaluate_rules(RuleExecutionMode::EvaluateOnly, &rules, &emails, &scope);
        assert_eq!(r1[0].matched_email_ids, r2[0].matched_email_ids);
        assert!(r1[0].matched_email_ids.len() <= 20);
        assert_eq!(r1[0].projected_count, 100);
        // Head + tail anchored.
        assert_eq!(r1[0].matched_email_ids.first().unwrap(), "e000");
        assert_eq!(r1[0].matched_email_ids.last().unwrap(), "e099");
    }

    #[test]
    fn evaluate_rules_no_command_emission() {
        // Smoke: in EvaluateOnly mode, `intended_actions` is populated but no
        // `PendingAction` is produced (this function returns RuleEvaluations,
        // never PendingActions).
        let rules = vec![make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::Delete],
        )];
        let emails = vec![email_at("a", Utc::now(), "boss@x.com", "hi")];
        let scope = EvaluationScope {
            account_id: "acct".to_string(),
            rule_ids: vec![],
            sample_size: 20,
        };
        let evals = evaluate_rules(RuleExecutionMode::EvaluateOnly, &rules, &emails, &scope);
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0].intended_actions.len(), 1);
    }

    #[test]
    fn evaluate_rules_filters_by_scope_rule_ids() {
        let rules = vec![
            make_rule(
                "keep",
                vec![RuleCondition::FieldMatch {
                    field: EmailField::From,
                    operator: MatchOperator::Contains,
                    value: "boss".to_string(),
                }],
                vec![RuleAction::Archive],
            ),
            make_rule(
                "skip",
                vec![RuleCondition::FieldMatch {
                    field: EmailField::From,
                    operator: MatchOperator::Contains,
                    value: "boss".to_string(),
                }],
                vec![RuleAction::Delete],
            ),
        ];
        let emails = vec![email_at("a", Utc::now(), "boss@x.com", "hi")];
        let scope = EvaluationScope {
            account_id: "acct".to_string(),
            rule_ids: vec!["keep".to_string()],
            sample_size: 20,
        };
        let evals = evaluate_rules(RuleExecutionMode::EvaluateOnly, &rules, &emails, &scope);
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0].rule_id, "keep");
    }

    #[test]
    fn process_email_respects_priority() {
        let email = sample_email();
        let mut low_priority = make_rule(
            "low",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        low_priority.priority = 1;

        let mut high_priority = make_rule(
            "high",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "boss".to_string(),
            }],
            vec![RuleAction::MarkImportant],
        );
        high_priority.priority = 10;

        let rules = vec![low_priority, high_priority];
        let pending = process_email(&rules, &email);

        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].rule_id, "high");
        assert_eq!(pending[1].rule_id, "low");
    }
}
