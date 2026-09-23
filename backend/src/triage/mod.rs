//! Deterministic admission and planning for approved mail triage.
//!
//! This layer never calls a model or provider. Its inputs must be grounded in
//! fresh provider state, approved mappings and the persistent obligation ledger.
pub mod policy;
