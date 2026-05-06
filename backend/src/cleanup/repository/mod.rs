//! SQLite-backed persistence for cleanup plans (Phase A).

pub mod adapters;
pub mod job_repo;
pub mod plan_repo;

pub use adapters::{
    SqlxAccountStateProvider, SqlxClusterRepository, SqlxEmailRepository, SqlxRuleEvaluator,
    SqlxSubscriptionRepository,
};
pub use job_repo::{CleanupApplyJobRepository, SqliteCleanupApplyJobRepo};
pub use plan_repo::{CleanupPlanRepository, OpsFilter, Page, SqliteCleanupPlanRepo};
