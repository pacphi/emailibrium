//! `sync_queue` — offline provider-operation queue (migration 011).
//!
//! `created_at`/`processed_at` are `TIMESTAMPTZ` on PostgreSQL (`DATETIME` on
//! SQLite), so the entity carries `DateTime<Utc>` — the type the pre-port Postgres
//! arm already used; on SQLite the encode is RFC3339, matching the old
//! `.to_rfc3339()` String binds byte-for-byte for UTC values. This is what
//! collapses the pre-port `QueueRowSqlite`/`QueueRowPostgres` split into one row
//! type.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sync_queue")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub account_id: String,
    pub operation_type: String,
    pub target_id: String,
    pub payload: Option<String>,
    pub status: Option<String>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub retry_count: Option<i32>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub max_retries: Option<i32>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
