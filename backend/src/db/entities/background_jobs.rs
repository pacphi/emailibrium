//! `background_jobs` — content-extraction/embedding job queue (migration 006).
//!
//! All four temporal columns are TEXT by design (ADR-035 §2.5) and the application
//! reads them as plain `String`s with no parsing — the entity keeps them `String`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "background_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub job_type: String,
    pub payload: String,
    pub status: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub priority: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub attempts: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub max_retries: i32,
    pub error_msg: Option<String>,
    /// TEXT timestamp (§2.5 class).
    pub created_at: String,
    /// TEXT timestamp (§2.5 class).
    pub updated_at: String,
    /// TEXT timestamp (§2.5 class).
    pub scheduled_at: Option<String>,
    /// TEXT timestamp (§2.5 class).
    pub completed_at: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
