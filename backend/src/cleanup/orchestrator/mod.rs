//! Cleanup Apply orchestration (Phase C, ADR-030 §C / DDD-008 addendum).
//!
//! - [`apply::ApplyOrchestrator`] is the entry point that the API layer talks to.
//! - [`account_worker::AccountWorker`] runs per-account in its own task.
//! - [`drift::DriftDetector`] enforces ADR-030 §8 account-scoped drift policy.
//! - [`expander::PredicateExpander`] materialises predicate rows lazily.
//! - [`sse::ApplyEvent`] is the wire schema mirrored on the frontend.

pub mod account_worker;
pub mod apply;
pub mod drift;
pub mod expander;
pub mod factory;
pub mod sse;

pub use apply::ApplyOrchestrator;
pub use drift::DriftDetector;
pub use expander::PredicateExpander;
pub use factory::{EmailProviderFactory, OAuthEmailProviderFactory};
