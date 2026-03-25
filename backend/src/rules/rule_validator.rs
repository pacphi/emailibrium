//! Rule validator for pre-execution safety checks (R-03).
//!
//! Detects:
//! - Contradictory actions (e.g. Archive + Delete on the same rule)
//! - Potential infinite loops between rules
//! - Invalid field/operator combinations
//! - Non-fatal warnings (e.g. duplicate actions)

use super::types::{EmailField, MatchOperator, Rule, RuleAction, RuleCondition};

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "ERROR"),
            Self::Warning => write!(f, "WARNING"),
        }
    }
}

/// A single validation finding.
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub severity: Severity,
    pub rule_id: String,
    pub message: String,
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] rule '{}': {}", self.severity, self.rule_id, self.message)
    }
}

/// Validate a single rule and return all findings.
pub fn validate_rule(rule: &Rule) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    validate_basic(rule, &mut warnings);
    validate_contradictions(rule, &mut warnings);
    validate_conditions(rule, &mut warnings);
    validate_duplicate_actions(rule, &mut warnings);

    warnings
}

/// Validate a set of rules, including cross-rule checks (infinite loops).
pub fn validate_rules(rules: &[Rule]) -> Vec<ValidationWarning> {
    let mut warnings: Vec<ValidationWarning> = rules.iter().flat_map(validate_rule).collect();
    validate_loops(rules, &mut warnings);
    warnings
}

/// Return only errors (not warnings).
pub fn has_errors(warnings: &[ValidationWarning]) -> bool {
    warnings.iter().any(|w| w.severity == Severity::Error)
}

// ---------------------------------------------------------------------------
// Internal validators
// ---------------------------------------------------------------------------

fn validate_basic(rule: &Rule, warnings: &mut Vec<ValidationWarning>) {
    if rule.name.trim().is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Error,
            rule_id: rule.id.clone(),
            message: "Rule name must not be empty".to_string(),
        });
    }
    if rule.conditions.is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Error,
            rule_id: rule.id.clone(),
            message: "Rule must have at least one condition".to_string(),
        });
    }
    if rule.actions.is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Error,
            rule_id: rule.id.clone(),
            message: "Rule must have at least one action".to_string(),
        });
    }
}

fn validate_contradictions(rule: &Rule, warnings: &mut Vec<ValidationWarning>) {
    let has_archive = rule.actions.iter().any(|a| matches!(a, RuleAction::Archive));
    let has_delete = rule.actions.iter().any(|a| matches!(a, RuleAction::Delete));

    if has_archive && has_delete {
        warnings.push(ValidationWarning {
            severity: Severity::Error,
            rule_id: rule.id.clone(),
            message: "Contradictory actions: Archive and Delete cannot both be applied".to_string(),
        });
    }

    // Check for add + remove of the same label.
    let added_labels: Vec<&str> = rule
        .actions
        .iter()
        .filter_map(|a| match a {
            RuleAction::AddLabel { label } => Some(label.as_str()),
            _ => None,
        })
        .collect();

    let removed_labels: Vec<&str> = rule
        .actions
        .iter()
        .filter_map(|a| match a {
            RuleAction::RemoveLabel { label } => Some(label.as_str()),
            _ => None,
        })
        .collect();

    for label in &added_labels {
        if removed_labels.contains(label) {
            warnings.push(ValidationWarning {
                severity: Severity::Error,
                rule_id: rule.id.clone(),
                message: format!(
                    "Contradictory actions: AddLabel and RemoveLabel for '{label}'"
                ),
            });
        }
    }
}

fn validate_conditions(rule: &Rule, warnings: &mut Vec<ValidationWarning>) {
    for cond in &rule.conditions {
        validate_condition_tree(cond, &rule.id, warnings, 0);
    }
}

fn validate_condition_tree(
    cond: &RuleCondition,
    rule_id: &str,
    warnings: &mut Vec<ValidationWarning>,
    depth: usize,
) {
    const MAX_DEPTH: usize = 10;
    if depth > MAX_DEPTH {
        warnings.push(ValidationWarning {
            severity: Severity::Error,
            rule_id: rule_id.to_string(),
            message: format!("Condition nesting exceeds maximum depth of {MAX_DEPTH}"),
        });
        return;
    }

    match cond {
        RuleCondition::FieldMatch { field, operator, value } => {
            // Validate that numeric operators are only used with Date field.
            if matches!(operator, MatchOperator::GreaterThan | MatchOperator::LessThan)
                && !matches!(field, EmailField::Date)
            {
                warnings.push(ValidationWarning {
                    severity: Severity::Warning,
                    rule_id: rule_id.to_string(),
                    message: format!(
                        "Operator '{}' is typically used with Date field, not {}",
                        operator.as_str(),
                        field.as_str()
                    ),
                });
            }

            // Validate regex compiles if operator is Regex.
            if matches!(operator, MatchOperator::Regex) {
                if let Err(e) = regex::Regex::new(value) {
                    warnings.push(ValidationWarning {
                        severity: Severity::Error,
                        rule_id: rule_id.to_string(),
                        message: format!("Invalid regex pattern '{value}': {e}"),
                    });
                }
            }

            if value.is_empty() {
                warnings.push(ValidationWarning {
                    severity: Severity::Warning,
                    rule_id: rule_id.to_string(),
                    message: format!(
                        "Empty value for field match on '{}'",
                        field.as_str()
                    ),
                });
            }
        }
        RuleCondition::Semantic { query, threshold } => {
            if query.trim().is_empty() {
                warnings.push(ValidationWarning {
                    severity: Severity::Error,
                    rule_id: rule_id.to_string(),
                    message: "Semantic condition has empty query".to_string(),
                });
            }
            if *threshold < 0.0 || *threshold > 1.0 {
                warnings.push(ValidationWarning {
                    severity: Severity::Error,
                    rule_id: rule_id.to_string(),
                    message: format!(
                        "Semantic threshold {threshold} must be between 0.0 and 1.0"
                    ),
                });
            }
        }
        RuleCondition::And { conditions } | RuleCondition::Or { conditions } => {
            if conditions.is_empty() {
                warnings.push(ValidationWarning {
                    severity: Severity::Error,
                    rule_id: rule_id.to_string(),
                    message: "Boolean combinator has no sub-conditions".to_string(),
                });
            }
            for child in conditions {
                validate_condition_tree(child, rule_id, warnings, depth + 1);
            }
        }
        RuleCondition::Not { condition } => {
            validate_condition_tree(condition, rule_id, warnings, depth + 1);
        }
    }
}

fn validate_duplicate_actions(rule: &Rule, warnings: &mut Vec<ValidationWarning>) {
    let types: Vec<&str> = rule.actions.iter().map(|a| a.action_type()).collect();
    let mut seen = std::collections::HashSet::new();
    for t in &types {
        if !seen.insert(t) {
            warnings.push(ValidationWarning {
                severity: Severity::Warning,
                rule_id: rule.id.clone(),
                message: format!("Duplicate action type: {t}"),
            });
        }
    }
}

/// Detect potential infinite loops between rules.
///
/// Heuristic: if rule A adds label X and rule B removes label X (or vice-versa),
/// and both rules trigger on label conditions, they could loop.
fn validate_loops(rules: &[Rule], warnings: &mut Vec<ValidationWarning>) {
    for (i, a) in rules.iter().enumerate() {
        if !a.enabled {
            continue;
        }
        for b in rules.iter().skip(i + 1) {
            if !b.enabled {
                continue;
            }
            // Check: A adds label that triggers B, and B removes (or adds) a label
            // that triggers A.
            let a_adds = labels_added(a);
            let b_triggers_on = labels_in_conditions(b);
            let b_adds = labels_added(b);
            let a_triggers_on = labels_in_conditions(a);

            for label in &a_adds {
                if b_triggers_on.contains(label) {
                    for b_label in &b_adds {
                        if a_triggers_on.contains(b_label) {
                            warnings.push(ValidationWarning {
                                severity: Severity::Warning,
                                rule_id: a.id.clone(),
                                message: format!(
                                    "Potential loop: rule '{}' adds label '{label}' triggering rule '{}', \
                                     which adds label '{b_label}' triggering rule '{}' again",
                                    a.name, b.name, a.name
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn labels_added(rule: &Rule) -> Vec<String> {
    rule.actions
        .iter()
        .filter_map(|a| match a {
            RuleAction::AddLabel { label } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

fn labels_in_conditions(rule: &Rule) -> Vec<String> {
    let mut labels = Vec::new();
    for cond in &rule.conditions {
        collect_label_values(cond, &mut labels);
    }
    labels
}

fn collect_label_values(cond: &RuleCondition, out: &mut Vec<String>) {
    match cond {
        RuleCondition::FieldMatch {
            field: EmailField::Labels,
            value,
            ..
        } => {
            out.push(value.clone());
        }
        RuleCondition::And { conditions } | RuleCondition::Or { conditions } => {
            for child in conditions {
                collect_label_values(child, out);
            }
        }
        RuleCondition::Not { condition } => {
            collect_label_values(condition, out);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::*;
    use chrono::Utc;

    fn make_rule(
        id: &str,
        conditions: Vec<RuleCondition>,
        actions: Vec<RuleAction>,
    ) -> Rule {
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
    fn valid_rule_no_warnings() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "hello".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings.is_empty(), "Got warnings: {warnings:?}");
    }

    #[test]
    fn detect_archive_delete_contradiction() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "spam".to_string(),
            }],
            vec![RuleAction::Archive, RuleAction::Delete],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings.iter().any(|w| w.severity == Severity::Error
            && w.message.contains("Contradictory")));
    }

    #[test]
    fn detect_add_remove_same_label() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "test".to_string(),
            }],
            vec![
                RuleAction::AddLabel {
                    label: "work".to_string(),
                },
                RuleAction::RemoveLabel {
                    label: "work".to_string(),
                },
            ],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings
            .iter()
            .any(|w| w.severity == Severity::Error && w.message.contains("work")));
    }

    #[test]
    fn detect_empty_name() {
        let mut rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "x".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        rule.name = "  ".to_string();
        let warnings = validate_rule(&rule);
        assert!(warnings.iter().any(|w| w.message.contains("name")));
    }

    #[test]
    fn detect_empty_conditions() {
        let rule = make_rule("r1", vec![], vec![RuleAction::Archive]);
        let warnings = validate_rule(&rule);
        assert!(warnings.iter().any(|w| w.message.contains("condition")));
    }

    #[test]
    fn detect_empty_actions() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::From,
                operator: MatchOperator::Contains,
                value: "x".to_string(),
            }],
            vec![],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings.iter().any(|w| w.message.contains("action")));
    }

    #[test]
    fn detect_invalid_regex() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Regex,
                value: "[invalid(".to_string(),
            }],
            vec![RuleAction::MarkRead],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings
            .iter()
            .any(|w| w.severity == Severity::Error && w.message.contains("regex")));
    }

    #[test]
    fn detect_semantic_empty_query() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::Semantic {
                query: "  ".to_string(),
                threshold: 0.8,
            }],
            vec![RuleAction::MarkRead],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings
            .iter()
            .any(|w| w.severity == Severity::Error && w.message.contains("empty query")));
    }

    #[test]
    fn detect_semantic_bad_threshold() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::Semantic {
                query: "test".to_string(),
                threshold: 1.5,
            }],
            vec![RuleAction::MarkRead],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings
            .iter()
            .any(|w| w.severity == Severity::Error && w.message.contains("threshold")));
    }

    #[test]
    fn detect_duplicate_actions() {
        let rule = make_rule(
            "r1",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: "test".to_string(),
            }],
            vec![RuleAction::MarkRead, RuleAction::MarkRead],
        );
        let warnings = validate_rule(&rule);
        assert!(warnings
            .iter()
            .any(|w| w.severity == Severity::Warning && w.message.contains("Duplicate")));
    }

    #[test]
    fn detect_potential_loop() {
        let rule_a = make_rule(
            "a",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Contains,
                value: "processed".to_string(),
            }],
            vec![RuleAction::AddLabel {
                label: "reviewed".to_string(),
            }],
        );
        let rule_b = make_rule(
            "b",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Contains,
                value: "reviewed".to_string(),
            }],
            vec![RuleAction::AddLabel {
                label: "processed".to_string(),
            }],
        );
        let warnings = validate_rules(&[rule_a, rule_b]);
        assert!(warnings.iter().any(|w| w.message.contains("loop")));
    }

    #[test]
    fn no_loop_when_disabled() {
        let mut rule_a = make_rule(
            "a",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Contains,
                value: "processed".to_string(),
            }],
            vec![RuleAction::AddLabel {
                label: "reviewed".to_string(),
            }],
        );
        rule_a.enabled = false;

        let rule_b = make_rule(
            "b",
            vec![RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Contains,
                value: "reviewed".to_string(),
            }],
            vec![RuleAction::AddLabel {
                label: "processed".to_string(),
            }],
        );
        let warnings = validate_rules(&[rule_a, rule_b]);
        assert!(!warnings.iter().any(|w| w.message.contains("loop")));
    }

    #[test]
    fn has_errors_returns_correctly() {
        let warnings = vec![ValidationWarning {
            severity: Severity::Warning,
            rule_id: "r1".to_string(),
            message: "test".to_string(),
        }];
        assert!(!has_errors(&warnings));

        let with_error = vec![ValidationWarning {
            severity: Severity::Error,
            rule_id: "r1".to_string(),
            message: "test".to_string(),
        }];
        assert!(has_errors(&with_error));
    }
}
