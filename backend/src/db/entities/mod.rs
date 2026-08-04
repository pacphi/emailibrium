//! SeaORM entities — the single source of truth for Rust-side column types (ADR-036).
//!
//! Each entity is a *partial model* over a table the migrations already create: it declares
//! the columns the application actually reads or writes, typed to match the migration DDL
//! exactly (`Vec<u8>` for BLOB/BYTEA ids and hashes, `i64` for BIGINT unix-millis timestamps,
//! `i32` for INTEGER — a real 4-byte int on PostgreSQL). SeaORM's value layer owns per-backend
//! encode/decode, which makes ADR-035's decode-width bug class unrepresentable: the "check the
//! migration DDL before choosing a decode type" rule is enforced by the entity definition
//! instead of by convention at every call site.
//!
//! Conventions (established by the ADR-036 spike, followed by every later port):
//! - one module per table, named after the table;
//! - `Relation` enums stay empty until a join is actually needed;
//! - columns are added as call sites port over — never speculatively.

pub mod cleanup_apply_jobs;
pub mod cleanup_plan_account_etags;
pub mod cleanup_plan_operations;
pub mod cleanup_plans;
