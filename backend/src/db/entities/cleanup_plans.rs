//! `cleanup_plans` — the immutable plan envelope (migration 024, ADR-030).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cleanup_plans")]
pub struct Model {
    /// UUID v7, 16 raw bytes (BLOB/BYTEA).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Vec<u8>,
    pub user_id: Vec<u8>,
    /// Unix millis (INTEGER in SQLite, BIGINT in PostgreSQL).
    pub created_at: i64,
    /// Unix millis.
    pub valid_until: i64,
    /// 32 raw blake3 bytes.
    pub plan_hash: Vec<u8>,
    pub status: String,
    pub totals_json: String,
    pub risk_json: String,
    pub warnings_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
