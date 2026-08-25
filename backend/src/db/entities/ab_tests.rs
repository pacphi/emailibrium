//! `ab_tests` — search-quality A/B test envelopes (migration 009).
//!
//! `created_at`/`concluded_at` are plain `TIMESTAMP` (no zone) — `NaiveDateTime`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "ab_tests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub test_id: String,
    pub name: String,
    pub variant_a_config: String,
    pub variant_b_config: String,
    /// REAL — 4-byte float on PostgreSQL.
    pub traffic_split: f32,
    pub status: String,
    pub created_at: chrono::NaiveDateTime,
    pub concluded_at: Option<chrono::NaiveDateTime>,
    pub metrics_a: String,
    pub metrics_b: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
