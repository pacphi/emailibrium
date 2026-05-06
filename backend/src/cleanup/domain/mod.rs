//! Cleanup Planning domain (Phase A).
//!
//! Pure types and pure services. No I/O. No provider calls. No SQL.
//!
//! - `operation` -- value types (PlanAction, PlanSource, RiskLevel, etc.)
//! - `plan` -- the CleanupPlan aggregate root + plan_hash
//! - `classifier` -- pure RiskClassifier
//! - `ports` -- repository ports (hexagonal). Adapters live in `repository::adapters`.
//! - `builder` -- PlanBuilder (composes the four sources into one plan)

pub mod builder;
pub mod classifier;
pub mod operation;
pub mod plan;
pub mod ports;
