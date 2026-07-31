//! Emailibrium -- vector-native email intelligence platform.
//!
//! Everything shared between the server binary and `backend/tests/` lives
//! here. The binary does **not** re-declare these modules with `mod`; it
//! re-exports them from this crate, so `emailibrium::db::Database` and the
//! binary's `crate::db::Database` are one type rather than two that refuse to
//! unify. Integration tests can therefore drive the same code that ships.
//!
//! What stays binary-side is what depends on `AppState`: the REST `api` layer,
//! and the halves of `cleanup` that reach into it (see [`cleanup`]).

pub mod cache;
pub mod config;
pub mod content;
pub mod db;
pub mod email;
pub mod events;
pub mod mcp;
pub mod middleware;
pub mod rules;
pub mod sync_lock;
pub mod tools;
pub mod vectors;

/// Cleanup planning — the portable half of the subdomain.
///
/// Only `domain` and `repository` live here. `cleanup::api` and
/// `cleanup::orchestrator` take `AppState` and stay binary-side, where
/// `src/cleanup/mod.rs` re-exports these two so `crate::cleanup::domain`
/// resolves to the same types from either crate.
///
/// The split is safe because it was checked in both directions: `domain` and
/// `repository` reach only into `crate::db`, `crate::email::types` and
/// `crate::rules`, and never back into `orchestrator` or `api`.
pub mod cleanup {
    pub mod domain;
    pub mod repository;
}
