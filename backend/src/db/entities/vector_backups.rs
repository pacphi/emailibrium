//! `vector_backups` — durable vector copies for reindex/restore (migration 001).
//!
//! `created_at`/`updated_at` are plain `TIMESTAMP` (no zone) — `NaiveDateTime`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "vector_backups")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub vector_id: String,
    pub email_id: String,
    pub collection: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub dimensions: i32,
    pub vector_data: Vec<u8>,
    pub metadata_json: Option<String>,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
