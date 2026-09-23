//! `cloud_api_audit_log` — cloud-API usage audit (migration 008).
//!
//! `timestamp` is plain `TIMESTAMP` (no zone) — `NaiveDateTime`, writes bind
//! `.naive_utc()` (closes pl-timestamp-write-tz for this table; see
//! `ai_consent.rs`).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cloud_api_audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub timestamp: chrono::NaiveDateTime,
    pub provider: String,
    pub model: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub input_tokens: Option<i32>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub output_tokens: Option<i32>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub latency_ms: i32,
    pub user_id: Option<String>,
    pub request_type: String,
    pub status: String,
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
