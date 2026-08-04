//! `category_centroids` — per-category embedding centroids (migration 001).
//!
//! `last_updated` is plain `TIMESTAMP` (no zone) — `NaiveDateTime`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "category_centroids")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub category: String,
    pub vector_data: Vec<u8>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub dimensions: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub email_count: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub feedback_count: i32,
    pub last_updated: Option<chrono::NaiveDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
