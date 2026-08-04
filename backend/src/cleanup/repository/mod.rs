//! Persistence for cleanup plans. The plan/job repositories are single-code-path
//! SeaORM (ADR-036) — the exemplar pattern later ports follow; the `adapters`
//! module still carries hand-rolled SQLx adapters until phase 3 ports them.

pub mod adapters;
pub mod job_repo;
pub mod plan_repo;

pub use adapters::{
    SqlxAccountStateProvider, SqlxClusterRepository, SqlxEmailRepository, SqlxRuleEvaluator,
    SqlxSubscriptionRepository,
};
pub use job_repo::{CleanupApplyJobRepository, SeaOrmCleanupApplyJobRepo};
pub use plan_repo::{CleanupPlanRepository, OpsFilter, SeaOrmCleanupPlanRepo};
