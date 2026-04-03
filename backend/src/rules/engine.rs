// Rule evaluation engine for email automation

use super::schema::{
    Action, Condition, ConditionNode, Field, LogicalOperator, Operator, Rule, RuleSet, Value,
};
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;

/// Email context for rule evaluation
#[derive(Debug, Clone)]
pub struct EmailContext {
    pub subject: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub labels: Vec<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_archived: bool,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub message_id: String,
    pub custom_fields: HashMap<String, Value>,
}

/// Result of rule evaluation
#[derive(Debug, Clone)]
pub struct RuleMatch {
    pub rule_id: String,
    pub rule_name: String,
    pub matched: bool,
    pub actions: Vec<Action>,
}

/// Rule evaluation engine
pub struct RuleEngine {
    ruleset: RuleSet,
    regex_cache: HashMap<String, Regex>,
}

impl RuleEngine {
    /// Create a new rule engine with a ruleset
    pub fn new(ruleset: RuleSet) -> Result<Self> {
        // Validate ruleset before accepting it
        ruleset
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid ruleset: {}", e))?;

        Ok(Self {
            ruleset,
            regex_cache: HashMap::new(),
        })
    }

    /// Evaluate all enabled rules against an email
    pub fn evaluate(&mut self, email: &EmailContext) -> Result<Vec<RuleMatch>> {
        let mut matches = Vec::new();

        // Clone enabled rules to avoid borrow checker issues
        let enabled_rules: Vec<Rule> = self.ruleset.enabled_rules().into_iter().cloned().collect();

        for rule in &enabled_rules {
            let matched = self.evaluate_rule(rule, email)?;
            matches.push(RuleMatch {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                matched,
                actions: if matched {
                    rule.actions.clone()
                } else {
                    Vec::new()
                },
            });

            // Check for Stop action
            if matched && rule.actions.iter().any(|a| matches!(a, Action::Stop)) {
                break;
            }
        }

        Ok(matches)
    }

    /// Evaluate a single rule against an email
    fn evaluate_rule(&mut self, rule: &Rule, email: &EmailContext) -> Result<bool> {
        self.evaluate_condition_node(&rule.conditions, email)
    }

    /// Evaluate a condition node (single or composite)
    fn evaluate_condition_node(
        &mut self,
        node: &ConditionNode,
        email: &EmailContext,
    ) -> Result<bool> {
        match node {
            ConditionNode::Single(condition) => self.evaluate_condition(condition, email),
            ConditionNode::Composite {
                operator,
                conditions,
            } => self.evaluate_composite(operator, conditions, email),
        }
    }

    /// Evaluate a composite condition with logical operators
    fn evaluate_composite(
        &mut self,
        operator: &LogicalOperator,
        conditions: &[ConditionNode],
        email: &EmailContext,
    ) -> Result<bool> {
        if conditions.is_empty() {
            return Ok(false);
        }

        match operator {
            LogicalOperator::And => {
                for condition in conditions {
                    if !self.evaluate_condition_node(condition, email)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LogicalOperator::Or => {
                for condition in conditions {
                    if self.evaluate_condition_node(condition, email)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            LogicalOperator::Not => {
                // NOT should only have one condition
                if conditions.len() != 1 {
                    anyhow::bail!("NOT operator must have exactly one condition");
                }
                Ok(!self.evaluate_condition_node(&conditions[0], email)?)
            }
        }
    }

    /// Evaluate a single condition
    fn evaluate_condition(&mut self, condition: &Condition, email: &EmailContext) -> Result<bool> {
        let field_value = self.get_field_value(&condition.field, email)?;
        self.compare_values(
            &field_value,
            &condition.operator,
            &condition.value,
            condition.case_sensitive,
        )
    }

    /// Get field value from email context
    fn get_field_value(&self, field: &Field, email: &EmailContext) -> Result<Value> {
        match field {
            Field::Subject => Ok(Value::String(email.subject.clone())),
            Field::From => Ok(Value::String(email.from.clone())),
            Field::To => Ok(Value::Array(
                email.to.iter().map(|s| Value::String(s.clone())).collect(),
            )),
            Field::Cc => Ok(Value::Array(
                email.cc.iter().map(|s| Value::String(s.clone())).collect(),
            )),
            Field::Bcc => Ok(Value::Array(
                email.bcc.iter().map(|s| Value::String(s.clone())).collect(),
            )),
            Field::Body | Field::BodyText => {
                Ok(Value::String(email.body_text.clone().unwrap_or_default()))
            }
            Field::BodyHtml => Ok(Value::String(email.body_html.clone().unwrap_or_default())),
            Field::Label => Ok(Value::Array(
                email
                    .labels
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            )),
            Field::IsRead => Ok(Value::Boolean(email.is_read)),
            Field::IsStarred => Ok(Value::Boolean(email.is_starred)),
            Field::IsArchived => Ok(Value::Boolean(email.is_archived)),
            Field::ReceivedAt => Ok(Value::String(email.received_at.to_rfc3339())),
            Field::ReceivedDate => Ok(Value::String(
                email.received_at.format("%Y-%m-%d").to_string(),
            )),
            Field::ReceivedTime => Ok(Value::String(
                email.received_at.format("%H:%M:%S").to_string(),
            )),
            Field::MessageId => Ok(Value::String(email.message_id.clone())),
            Field::Custom(name) => email
                .custom_fields
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Custom field '{}' not found", name)),
        }
    }

    /// Compare two values using an operator
    fn compare_values(
        &mut self,
        field_value: &Value,
        operator: &Operator,
        compare_value: &Value,
        case_sensitive: bool,
    ) -> Result<bool> {
        match operator {
            Operator::Equals => Ok(self.compare_equal(field_value, compare_value, case_sensitive)),
            Operator::NotEquals => {
                Ok(!self.compare_equal(field_value, compare_value, case_sensitive))
            }
            Operator::Contains => {
                Ok(self.compare_contains(field_value, compare_value, case_sensitive))
            }
            Operator::NotContains => {
                Ok(!self.compare_contains(field_value, compare_value, case_sensitive))
            }
            Operator::StartsWith => {
                Ok(self.compare_starts_with(field_value, compare_value, case_sensitive))
            }
            Operator::EndsWith => {
                Ok(self.compare_ends_with(field_value, compare_value, case_sensitive))
            }
            Operator::Matches => self.compare_regex(field_value, compare_value),
            Operator::GreaterThan => self.compare_numeric(field_value, compare_value, |a, b| a > b),
            Operator::LessThan => self.compare_numeric(field_value, compare_value, |a, b| a < b),
            Operator::GreaterOrEqual => {
                self.compare_numeric(field_value, compare_value, |a, b| a >= b)
            }
            Operator::LessOrEqual => {
                self.compare_numeric(field_value, compare_value, |a, b| a <= b)
            }
            Operator::In => Ok(self.compare_in(field_value, compare_value, case_sensitive)),
            Operator::NotIn => Ok(!self.compare_in(field_value, compare_value, case_sensitive)),
        }
    }

    /// Compare for equality
    fn compare_equal(&self, a: &Value, b: &Value, case_sensitive: bool) -> bool {
        if case_sensitive {
            a == b
        } else {
            a.as_string().to_lowercase() == b.as_string().to_lowercase()
        }
    }

    /// Compare contains
    fn compare_contains(&self, haystack: &Value, needle: &Value, case_sensitive: bool) -> bool {
        let haystack_str = haystack.as_string();
        let needle_str = needle.as_string();

        if case_sensitive {
            haystack_str.contains(&needle_str)
        } else {
            haystack_str
                .to_lowercase()
                .contains(&needle_str.to_lowercase())
        }
    }

    /// Compare starts with
    fn compare_starts_with(&self, value: &Value, prefix: &Value, case_sensitive: bool) -> bool {
        let value_str = value.as_string();
        let prefix_str = prefix.as_string();

        if case_sensitive {
            value_str.starts_with(&prefix_str)
        } else {
            value_str
                .to_lowercase()
                .starts_with(&prefix_str.to_lowercase())
        }
    }

    /// Compare ends with
    fn compare_ends_with(&self, value: &Value, suffix: &Value, case_sensitive: bool) -> bool {
        let value_str = value.as_string();
        let suffix_str = suffix.as_string();

        if case_sensitive {
            value_str.ends_with(&suffix_str)
        } else {
            value_str
                .to_lowercase()
                .ends_with(&suffix_str.to_lowercase())
        }
    }

    /// Compare with regex
    fn compare_regex(&mut self, value: &Value, pattern: &Value) -> Result<bool> {
        let value_str = value.as_string();
        let pattern_str = pattern.as_string();

        // Cache compiled regex patterns for performance
        let regex = if let Some(cached) = self.regex_cache.get(&pattern_str) {
            cached
        } else {
            let compiled = Regex::new(&pattern_str)
                .context(format!("Invalid regex pattern: {}", pattern_str))?;
            self.regex_cache.insert(pattern_str.clone(), compiled);
            self.regex_cache.get(&pattern_str).unwrap()
        };

        Ok(regex.is_match(&value_str))
    }

    /// Compare numeric values
    fn compare_numeric<F>(&self, a: &Value, b: &Value, op: F) -> Result<bool>
    where
        F: Fn(f64, f64) -> bool,
    {
        let a_num = a
            .as_number()
            .ok_or_else(|| anyhow::anyhow!("Cannot convert {} to number", a.as_string()))?;
        let b_num = b
            .as_number()
            .ok_or_else(|| anyhow::anyhow!("Cannot convert {} to number", b.as_string()))?;

        Ok(op(a_num, b_num))
    }

    /// Compare if value is in array
    fn compare_in(&self, value: &Value, array: &Value, case_sensitive: bool) -> bool {
        if let Value::Array(items) = array {
            items
                .iter()
                .any(|item| self.compare_equal(value, item, case_sensitive))
        } else {
            false
        }
    }

    /// Get actions from matching rules
    pub fn get_actions(&self, matches: &[RuleMatch]) -> Vec<Action> {
        matches
            .iter()
            .filter(|m| m.matched)
            .flat_map(|m| m.actions.clone())
            .collect()
    }
}
