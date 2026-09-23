//! `ai_metadata` — AI/embedding bookkeeping key/value store (migration 003).
//!
//! `updated_at` is a real `TIMESTAMP` (no zone) in both dialects, so the entity
//! carries `NaiveDateTime` and writes bind `.naive_utc()` (ADR-036 shape rule).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ai_metadata")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    pub value: String,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
