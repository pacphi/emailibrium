//! Cleanup Planning domain (Phase A).
//!
//! Pure types and pure services. No I/O. No provider calls. No SQL.
//!
//! - `operation` -- value types (PlanAction, PlanSource, RiskLevel, etc.)
//! - `plan` -- the CleanupPlan aggregate root + plan_hash
//! - `classifier` -- pure RiskClassifier
//! - `ports` -- repository ports (hexagonal). Adapters live in `repository::adapters`.
//! - `builder` -- PlanBuilder (composes the four sources into one plan)
//! - `kind` -- header-rule `Kind` classifier (milestone 1, ahead of the
//!   semantic/vector classifier for the Kind axis specifically)
//! - `sender_key` -- `local-part@registrable-domain` sender key (milestone 1)

pub mod builder;
pub mod classifier;
pub mod kind;
pub mod label_namespace;
pub mod operation;
pub mod plan;
pub mod ports;
pub mod sender_key;
