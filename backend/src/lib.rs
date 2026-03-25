//! Emailibrium -- vector-native email intelligence platform.
//!
//! This library crate re-exports internal modules so they are accessible
//! from integration tests and benchmarks.

pub mod cache;
pub mod config;
pub mod content;
pub mod db;
pub mod email;
pub mod events;
pub mod middleware;
pub mod rules;
pub mod vectors;
