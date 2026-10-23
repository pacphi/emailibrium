//! `cleanup_plan_operations` — per-row plan operations (migration 024, ADR-030).
//!
//! Partial model: the table carries further columns no repository reads or
//! writes as columns today — predicate-only ones (`predicate_kind`,
//! `predicate_id`, `projected_count`) and row-level ones (`target_*`,
//! `source_id`, `reverse_op_json`, `skip_reason`, `error`, `partial_applied`);
//! all travel inside `payload_json`. Add them here when a call site actually
//! needs them (see `entities/mod.rs`).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cleanup_plan_operations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub plan_id: Vec<u8>,
    /// INTEGER — a real 4-byte int on PostgreSQL, so `i32` (ADR-035 width class).
    #[sea_orm(primary_key, auto_increment = false)]
    pub seq: i32,
    /// `'materialized' | 'predicate'`.
    pub op_kind: String,
    pub account_id: Vec<u8>,
    /// Materialized rows only.
    pub email_id: Option<Vec<u8>>,
    /// JSON-serialized `PlanAction`.
    pub action: String,
    /// `'subscription' | 'cluster' | 'rule' | 'strategy' | 'manual'`.
    pub source_kind: String,
    /// `'low' | 'medium' | 'high'`.
    pub risk: String,
    pub status: String,
    /// Unix millis; set when a row is applied.
    pub applied_at: Option<i64>,
    /// Predicate rows only; JSON array of email ids.
    pub sample_ids_json: Option<String>,
    /// Full serialized `PlannedOperation` for round-trip.
    pub payload_json: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
