//! Rule processor -- evaluates rules against emails and generates pending actions (R-03).

use super::types::{EmailField, MatchOperator, PendingAction, Rule, RuleAction, RuleCondition};
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

/// Evaluate all rules (sorted by descending priority) against an email,
/// collecting pending actions.
pub fn process_email(rules: &[Rule], email: &EmailMessage) -> Vec<PendingAction> {
    let mut sorted: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
    sorted.sort_by(|a, b| b.priority.cmp(&a.priority));

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
            labels: vec!["inbox".to_string(), "important".to_string()],
            date: Utc::now(),
            is_read: false,
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
