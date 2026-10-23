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

pub mod ab_test_results;
pub mod ab_tests;
pub mod ai_audit_log;
pub mod ai_consent;
pub mod ai_metadata;
pub mod app_settings;
pub mod attachments;
pub mod background_jobs;
pub mod category_centroids;
pub mod cleanup_apply_jobs;
pub mod cleanup_audit_log;
pub mod cleanup_plan_account_etags;
pub mod cleanup_plan_operations;
pub mod cleanup_plans;
pub mod cloud_api_audit_log;
pub mod connected_accounts;
pub mod consent_decisions;
pub mod emails;
pub mod ingestion_checkpoints;
pub mod mcp_tool_audit;
pub mod privacy_audit_log;
pub mod processing_checkpoints;
pub mod rules;
pub mod search_interactions;
pub mod sync_conflicts;
pub mod sync_queue;
pub mod sync_state;
pub mod topic_clusters;
pub mod user_learning_models;
pub mod vector_backups;
