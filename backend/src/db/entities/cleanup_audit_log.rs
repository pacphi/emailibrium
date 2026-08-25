//! `cleanup_audit_log` — per-operation apply outcomes (migration 025, ADR-030).
//!
//! `timestamp` is BIGINT unix millis (`i64`) — immune to every dialect timestamp
//! issue. The four id columns are BLOB/BYTEA: `plan_id`/`job_id` hold raw UUID
//! bytes; `user_id`/`account_id` hold UTF-8 strings stored as bytes. The entity
//! keeps all four as `Vec<u8>` — converting them to native uuid/text columns would
//! be a schema change, not a port.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cleanup_audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Unix millis (BIGINT).
    pub timestamp: i64,
    pub plan_id: Vec<u8>,
    pub job_id: Vec<u8>,
    pub user_id: Vec<u8>,
    pub account_id: Vec<u8>,
    /// INTEGER — 4-byte on PostgreSQL; widened to/from `u64` at the trait boundary.
    pub seq: i32,
    pub op_kind: String,
    pub action_type: String,
    pub source_type: String,
    pub outcome: String,
    pub skip_reason: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
