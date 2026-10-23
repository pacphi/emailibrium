//! `user_learning_models` — per-user category learning offsets (migration 007).
//!
//! Composite `(user_id, category)` primary key. `created_at`/`updated_at` are plain
//! `TIMESTAMP` (no zone) — `NaiveDateTime`, writes bind `.naive_utc()`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_learning_models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub category: String,
    pub offset_json: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub feedback_count: i32,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
