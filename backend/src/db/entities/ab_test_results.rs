//! `ab_test_results` — per-variant search-quality samples (migration 009).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "ab_test_results")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub test_id: String,
    pub variant: String,
    /// Plain `TIMESTAMP` (no zone) — `NaiveDateTime`.
    pub timestamp: chrono::NaiveDateTime,
    /// REAL — 4-byte float on PostgreSQL.
    pub mrr: Option<f32>,
    pub precision_at_k: Option<f32>,
    pub recall_at_k: Option<f32>,
    pub ndcg: Option<f32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
