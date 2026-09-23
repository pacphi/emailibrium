//! `search_interactions` — search click/feedback log (migration 001).
//!
//! `created_at` is plain `TIMESTAMP` (no zone) — `NaiveDateTime`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "search_interactions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub query_text: String,
    pub query_vector_id: Option<String>,
    pub result_email_id: Option<String>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub result_rank: Option<i32>,
    pub clicked: Option<bool>,
    pub feedback: Option<String>,
    pub created_at: Option<chrono::NaiveDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
