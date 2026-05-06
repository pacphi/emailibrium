//! Cleanup Planning subdomain (DDD-008 addendum, ADR-030).
//!
//! The cleanup module owns the immutable, materialized **CleanupPlan** that
//! the Inbox Cleaner Wizard produces before any provider mutation. Phase A
//! ships the domain types, the SQLite repository, and the Plan API. Phase B
//! adds the frontend Review screen; Phase C adds Apply with SSE; Phase D
//! adds risk + telemetry; Phase E ships Undo via reverse_op.
//!
//! See:
//! - `docs/ADRs/ADR-030-cleanup-dry-run.md`
//! - `docs/DDDs/DDD-008-addendum-cleanup-planning.md`
//! - `docs/plan/cleanup-dry-run-implementation.md`

pub mod audit;
pub mod domain;
pub mod orchestrator;
pub mod repository;
pub mod telemetry;

pub mod api;

pub use api::routes;
