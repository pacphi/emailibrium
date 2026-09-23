//! `ingestion_checkpoints` — resumable ingestion progress (migration 006).
//!
//! `created_at`/`updated_at` are TEXT by design (ADR-035 §2.5 — the migration
//! keeps them TEXT in both dialects because downstream code parses the exact
//! `YYYY-MM-DD HH:MM:SS` shape) — `String` here, format owned by the application.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ingestion_checkpoints")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub batch_id: String,
    pub account_id: String,
    pub stage: String,
    pub status: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub total: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub processed: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub failed: i32,
    pub last_processed_id: Option<String>,
    pub error_msg: Option<String>,
    pub metadata: Option<String>,
    /// TEXT timestamp (§2.5 class).
    pub created_at: String,
    /// TEXT timestamp (§2.5 class).
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
