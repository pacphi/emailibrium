//! Rules Engine for Email Automation (R-03).
//!
//! Provides a hybrid rule engine combining structural field matching
//! with semantic vector-similarity conditions. Supports:
//!
//! - JSON-based condition parsing with natural language fallback
//! - Validation (contradictions, regex, nesting depth)
//! - Priority-ordered evaluation against emails
//! - SQLite-backed CRUD for rule persistence

pub mod json_parser;
pub mod rule_engine;
pub mod rule_processor;
pub mod rule_validator;
pub mod types;

pub use types::{
    EmailField, MatchOperator, Rule, RuleAction, RuleCondition,
};
