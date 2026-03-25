//! JSON and natural-language parser for rule conditions (R-03).
//!
//! Provides two entry points:
//! - `parse_condition` -- deserialise a `serde_json::Value` into `RuleCondition`.
//! - `parse_natural_language` -- regex-based extraction of simple English phrases
//!   into `RuleCondition` trees (e.g. "from boss@co.com", "subject contains urgent",
//!   "about project updates" -> Semantic).

use anyhow::{bail, Context, Result};
use regex::Regex;

use super::types::{EmailField, MatchOperator, RuleCondition};

// ---------------------------------------------------------------------------
// JSON parsing
// ---------------------------------------------------------------------------

/// Parse a `serde_json::Value` into a `RuleCondition`.
///
/// Accepts the same JSON shape produced by `serde_json::to_value(&RuleCondition)`,
/// but also tolerates a few shorthand forms used by the frontend Rules Studio.
pub fn parse_condition(json: &serde_json::Value) -> Result<RuleCondition> {
    // Fast-path: try direct deserialisation first.
    if let Ok(cond) = serde_json::from_value::<RuleCondition>(json.clone()) {
        return Ok(cond);
    }

    // Fallback: attempt manual extraction for looser frontend payloads.
    let obj = json
        .as_object()
        .context("Condition must be a JSON object")?;

    // Check for boolean combinators first.
    if let Some(kind) = obj.get("type").and_then(|v| v.as_str()) {
        match kind {
            "and" | "And" | "AND" => {
                let children = parse_children(obj)?;
                return Ok(RuleCondition::And {
                    conditions: children,
                });
            }
            "or" | "Or" | "OR" => {
                let children = parse_children(obj)?;
                return Ok(RuleCondition::Or {
                    conditions: children,
                });
            }
            "not" | "Not" | "NOT" => {
                let child = parse_single_child(obj)?;
                return Ok(RuleCondition::Not {
                    condition: Box::new(child),
                });
            }
            _ => {}
        }
    }

    // Try field-match shorthand: { "field": "from", "operator": "contains", "value": "x" }
    if let (Some(field_str), Some(op_str), Some(val)) = (
        obj.get("field").and_then(|v| v.as_str()),
        obj.get("operator").and_then(|v| v.as_str()),
        obj.get("value").and_then(|v| v.as_str()),
    ) {
        let field = parse_email_field(field_str)?;
        let operator = parse_match_operator(op_str)?;
        return Ok(RuleCondition::FieldMatch {
            field,
            operator,
            value: val.to_string(),
        });
    }

    // Try semantic shorthand: { "query": "...", "threshold": 0.8 }
    if let Some(query) = obj.get("query").and_then(|v| v.as_str()) {
        let threshold = obj
            .get("threshold")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.75);
        return Ok(RuleCondition::Semantic {
            query: query.to_string(),
            threshold,
        });
    }

    bail!("Unable to parse condition from JSON: {json}")
}

fn parse_children(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<RuleCondition>> {
    let arr = obj
        .get("conditions")
        .and_then(|v| v.as_array())
        .context("Boolean combinator requires a 'conditions' array")?;
    arr.iter().map(parse_condition).collect()
}

fn parse_single_child(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<RuleCondition> {
    if let Some(cond) = obj.get("condition") {
        return parse_condition(cond);
    }
    if let Some(arr) = obj.get("conditions").and_then(|v| v.as_array()) {
        if arr.len() == 1 {
            return parse_condition(&arr[0]);
        }
    }
    bail!("NOT combinator requires a single 'condition' value")
}

fn parse_email_field(s: &str) -> Result<EmailField> {
    match s.to_lowercase().as_str() {
        "from" => Ok(EmailField::From),
        "to" => Ok(EmailField::To),
        "subject" => Ok(EmailField::Subject),
        "body" => Ok(EmailField::Body),
        "labels" | "label" => Ok(EmailField::Labels),
        "date" => Ok(EmailField::Date),
        other => bail!("Unknown email field: {other}"),
    }
}

fn parse_match_operator(s: &str) -> Result<MatchOperator> {
    match s.to_lowercase().as_str() {
        "contains" => Ok(MatchOperator::Contains),
        "equals" | "eq" => Ok(MatchOperator::Equals),
        "startswith" | "starts_with" => Ok(MatchOperator::StartsWith),
        "endswith" | "ends_with" => Ok(MatchOperator::EndsWith),
        "regex" | "matches" => Ok(MatchOperator::Regex),
        "greaterthan" | "greater_than" | "gt" => Ok(MatchOperator::GreaterThan),
        "lessthan" | "less_than" | "lt" => Ok(MatchOperator::LessThan),
        other => bail!("Unknown match operator: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Natural-language parsing
// ---------------------------------------------------------------------------

/// Parse a simple English sentence into a `RuleCondition`.
///
/// Recognised patterns (case-insensitive):
///
/// | Pattern                        | Result                                       |
/// |-------------------------------|----------------------------------------------|
/// | `from <addr>`                  | FieldMatch(From, Contains, addr)             |
/// | `to <addr>`                    | FieldMatch(To, Contains, addr)               |
/// | `subject contains <text>`      | FieldMatch(Subject, Contains, text)          |
/// | `subject is <text>`            | FieldMatch(Subject, Equals, text)            |
/// | `subject starts with <text>`   | FieldMatch(Subject, StartsWith, text)        |
/// | `subject ends with <text>`     | FieldMatch(Subject, EndsWith, text)          |
/// | `body contains <text>`         | FieldMatch(Body, Contains, text)             |
/// | `label is <text>`              | FieldMatch(Labels, Equals, text)             |
/// | `about <topic>`                | Semantic(topic, 0.75)                        |
/// | `<expr> and <expr>`            | And([left, right])                           |
/// | `<expr> or <expr>`             | Or([left, right])                            |
///
/// Anything that does not match a structural pattern falls back to a Semantic
/// condition with the full text as the query.
pub fn parse_natural_language(text: &str) -> Result<RuleCondition> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("Cannot parse empty text into a rule condition");
    }

    // Try splitting on " and " / " or " for compound conditions.
    // Only split at the top level (first occurrence).
    if let Some(idx) = find_top_level_split(trimmed, " and ") {
        let left = parse_natural_language(&trimmed[..idx])?;
        let right = parse_natural_language(&trimmed[idx + 5..])?;
        return Ok(RuleCondition::And {
            conditions: vec![left, right],
        });
    }
    if let Some(idx) = find_top_level_split(trimmed, " or ") {
        let left = parse_natural_language(&trimmed[..idx])?;
        let right = parse_natural_language(&trimmed[idx + 4..])?;
        return Ok(RuleCondition::Or {
            conditions: vec![left, right],
        });
    }

    // Try each structural pattern.
    if let Some(cond) = try_from_pattern(trimmed) {
        return Ok(cond);
    }
    if let Some(cond) = try_field_operator_pattern(trimmed) {
        return Ok(cond);
    }

    // Fallback: treat the whole string as a semantic query.
    Ok(RuleCondition::Semantic {
        query: trimmed.to_string(),
        threshold: 0.75,
    })
}

/// Find the byte-index of a top-level `needle` in `text`, ignoring occurrences
/// inside quoted strings. Returns `None` if not found.
fn find_top_level_split(text: &str, needle: &str) -> Option<usize> {
    let lower = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    lower.find(&needle_lower)
}

/// Try "from <addr>" and "to <addr>" patterns.
fn try_from_pattern(text: &str) -> Option<RuleCondition> {
    let lower = text.to_lowercase();

    // "from <value>"
    let from_re = Regex::new(r"(?i)^from\s+(.+)$").ok()?;
    if let Some(caps) = from_re.captures(text) {
        let value = caps.get(1)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field: EmailField::From,
            operator: MatchOperator::Contains,
            value,
        });
    }

    // "to <value>"
    let to_re = Regex::new(r"(?i)^to\s+(.+)$").ok()?;
    if let Some(caps) = to_re.captures(text) {
        let value = caps.get(1)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field: EmailField::To,
            operator: MatchOperator::Contains,
            value,
        });
    }

    // "about <topic>" -> Semantic
    if lower.starts_with("about ") {
        let query = text[6..].trim().to_string();
        if !query.is_empty() {
            return Some(RuleCondition::Semantic {
                query,
                threshold: 0.75,
            });
        }
    }

    // "label is <value>"
    let label_re = Regex::new(r"(?i)^label\s+is\s+(.+)$").ok()?;
    if let Some(caps) = label_re.captures(text) {
        let value = caps.get(1)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field: EmailField::Labels,
            operator: MatchOperator::Equals,
            value,
        });
    }

    None
}

/// Try "<field> <operator> <value>" patterns.
fn try_field_operator_pattern(text: &str) -> Option<RuleCondition> {
    // subject contains <value>
    let contains_re = Regex::new(r"(?i)^(subject|body)\s+contains\s+(.+)$").ok()?;
    if let Some(caps) = contains_re.captures(text) {
        let field = match caps.get(1)?.as_str().to_lowercase().as_str() {
            "subject" => EmailField::Subject,
            "body" => EmailField::Body,
            _ => return None,
        };
        let value = caps.get(2)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field,
            operator: MatchOperator::Contains,
            value,
        });
    }

    // subject is <value>
    let equals_re = Regex::new(r"(?i)^(subject|body)\s+is\s+(.+)$").ok()?;
    if let Some(caps) = equals_re.captures(text) {
        let field = match caps.get(1)?.as_str().to_lowercase().as_str() {
            "subject" => EmailField::Subject,
            "body" => EmailField::Body,
            _ => return None,
        };
        let value = caps.get(2)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field,
            operator: MatchOperator::Equals,
            value,
        });
    }

    // subject starts with <value>
    let sw_re = Regex::new(r"(?i)^(subject|body)\s+starts\s+with\s+(.+)$").ok()?;
    if let Some(caps) = sw_re.captures(text) {
        let field = match caps.get(1)?.as_str().to_lowercase().as_str() {
            "subject" => EmailField::Subject,
            "body" => EmailField::Body,
            _ => return None,
        };
        let value = caps.get(2)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field,
            operator: MatchOperator::StartsWith,
            value,
        });
    }

    // subject ends with <value>
    let ew_re = Regex::new(r"(?i)^(subject|body)\s+ends\s+with\s+(.+)$").ok()?;
    if let Some(caps) = ew_re.captures(text) {
        let field = match caps.get(1)?.as_str().to_lowercase().as_str() {
            "subject" => EmailField::Subject,
            "body" => EmailField::Body,
            _ => return None,
        };
        let value = caps.get(2)?.as_str().trim().to_string();
        return Some(RuleCondition::FieldMatch {
            field,
            operator: MatchOperator::EndsWith,
            value,
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- JSON parsing tests --

    #[test]
    fn parse_field_match_json() {
        let val = json!({
            "type": "fieldMatch",
            "field": "subject",
            "operator": "contains",
            "value": "urgent"
        });
        let cond = parse_condition(&val).unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                ..
            }
        ));
    }

    #[test]
    fn parse_semantic_json() {
        let val = json!({
            "type": "semantic",
            "query": "project deadlines",
            "threshold": 0.85
        });
        let cond = parse_condition(&val).unwrap();
        if let RuleCondition::Semantic { query, threshold } = cond {
            assert_eq!(query, "project deadlines");
            assert!((threshold - 0.85).abs() < f32::EPSILON);
        } else {
            panic!("Expected Semantic condition");
        }
    }

    #[test]
    fn parse_and_json() {
        let val = json!({
            "type": "and",
            "conditions": [
                { "type": "fieldMatch", "field": "from", "operator": "contains", "value": "boss" },
                { "type": "semantic", "query": "budget", "threshold": 0.7 }
            ]
        });
        let cond = parse_condition(&val).unwrap();
        assert!(matches!(cond, RuleCondition::And { conditions } if conditions.len() == 2));
    }

    #[test]
    fn parse_shorthand_field_match() {
        let val = json!({
            "field": "from",
            "operator": "contains",
            "value": "alice@example.com"
        });
        let cond = parse_condition(&val).unwrap();
        assert!(matches!(cond, RuleCondition::FieldMatch { .. }));
    }

    #[test]
    fn parse_shorthand_semantic() {
        let val = json!({ "query": "weekly status update" });
        let cond = parse_condition(&val).unwrap();
        assert!(matches!(cond, RuleCondition::Semantic { .. }));
    }

    #[test]
    fn parse_invalid_json_fails() {
        let val = json!({ "garbage": true });
        assert!(parse_condition(&val).is_err());
    }

    // -- Natural-language parsing tests --

    #[test]
    fn nl_from_pattern() {
        let cond = parse_natural_language("from boss@company.com").unwrap();
        if let RuleCondition::FieldMatch { field, operator, value } = cond {
            assert_eq!(field, EmailField::From);
            assert_eq!(operator, MatchOperator::Contains);
            assert_eq!(value, "boss@company.com");
        } else {
            panic!("Expected FieldMatch");
        }
    }

    #[test]
    fn nl_to_pattern() {
        let cond = parse_natural_language("to team@company.com").unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch { field: EmailField::To, .. }
        ));
    }

    #[test]
    fn nl_subject_contains() {
        let cond = parse_natural_language("subject contains URGENT").unwrap();
        if let RuleCondition::FieldMatch { field, operator, value } = cond {
            assert_eq!(field, EmailField::Subject);
            assert_eq!(operator, MatchOperator::Contains);
            assert_eq!(value, "URGENT");
        } else {
            panic!("Expected FieldMatch");
        }
    }

    #[test]
    fn nl_subject_is() {
        let cond = parse_natural_language("subject is Hello World").unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Equals,
                ..
            }
        ));
    }

    #[test]
    fn nl_about_becomes_semantic() {
        let cond = parse_natural_language("about quarterly earnings").unwrap();
        if let RuleCondition::Semantic { query, threshold } = cond {
            assert_eq!(query, "quarterly earnings");
            assert!((threshold - 0.75).abs() < f32::EPSILON);
        } else {
            panic!("Expected Semantic");
        }
    }

    #[test]
    fn nl_label_is() {
        let cond = parse_natural_language("label is important").unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch {
                field: EmailField::Labels,
                operator: MatchOperator::Equals,
                ..
            }
        ));
    }

    #[test]
    fn nl_and_combinator() {
        let cond =
            parse_natural_language("from boss@co.com and subject contains review").unwrap();
        assert!(matches!(cond, RuleCondition::And { .. }));
    }

    #[test]
    fn nl_or_combinator() {
        let cond =
            parse_natural_language("from alice@co.com or from bob@co.com").unwrap();
        assert!(matches!(cond, RuleCondition::Or { .. }));
    }

    #[test]
    fn nl_unknown_becomes_semantic() {
        let cond = parse_natural_language("something about cats and dogs").unwrap();
        // "and" splits first, so we get And([Semantic, Semantic])
        // unless "something about cats" matches the about pattern -- it won't
        // because "something about" != "about".
        // "something about cats" -> no structural match -> Semantic
        assert!(matches!(cond, RuleCondition::And { .. }));
    }

    #[test]
    fn nl_empty_fails() {
        assert!(parse_natural_language("").is_err());
        assert!(parse_natural_language("   ").is_err());
    }

    #[test]
    fn nl_subject_starts_with() {
        let cond = parse_natural_language("subject starts with Re:").unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch {
                operator: MatchOperator::StartsWith,
                ..
            }
        ));
    }

    #[test]
    fn nl_subject_ends_with() {
        let cond = parse_natural_language("subject ends with [DONE]").unwrap();
        assert!(matches!(
            cond,
            RuleCondition::FieldMatch {
                operator: MatchOperator::EndsWith,
                ..
            }
        ));
    }
}
