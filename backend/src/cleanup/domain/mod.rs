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

pub use builder::{BuildError, PlanBuilder};
pub use classifier::{AccountContext, RiskClassifier};
pub use operation::{
    AccountStateEtag, ArchiveStrategy, ClusterAction, EmailRef, ErrorCode, FolderOrLabel, JobState,
    MoveKind, OperationStatus, PlanAction, PlanSource, PlanStatus, PlanWarning, PlannedOperation,
    PlannedOperationPredicate, PlannedOperationRow, PredicateKind, PredicateStatus, Provider,
    ReverseOp, RiskLevel, RiskMax, SkipReason, UnsubscribeMethodKind,
};
pub use plan::{
    canonical_plan_hash, CleanupApplyJob, CleanupPlan, CleanupPlanSummary, JobCounts, JobId,
    PlanId, PlanTotals, RiskRollup, WizardSelections,
};
pub use ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RuleEvaluator, SubscriptionRecord,
    SubscriptionRepository,
};
