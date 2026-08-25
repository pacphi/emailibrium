//! `ai_audit_log` — cloud-AI request audit trail (migration 002).
//!
//! `timestamp` is plain `TIMESTAMP` (no zone) — `NaiveDateTime`, writes bind
//! `.naive_utc()` (closes pl-timestamp-write-tz for this table; see
//! `ai_consent.rs`).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ai_audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub timestamp: chrono::NaiveDateTime,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub input_token_count: Option<i32>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub output_token_count: Option<i32>,
    pub input_hash: Option<String>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub latency_ms: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
