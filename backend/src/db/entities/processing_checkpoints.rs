//! `processing_checkpoints` — provider-sync resume points (migration 006).
//!
//! `updated_at` is TEXT by design (ADR-035 §2.5). The application writes RFC3339
//! into it while the DDL DEFAULT produces `YYYY-MM-DD HH:MM:SS` — a pre-existing
//! format mix preserved bug-for-bug (see `email/checkpoint.rs`'s `cleanup_old`
//! doc for why the lexicographic comparison quirk is load-bearing).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "processing_checkpoints")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub job_id: String,
    pub provider: String,
    pub account_id: String,
    pub last_processed_id: Option<String>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub total_count: Option<i32>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub processed_count: i32,
    pub state: String,
    pub error_message: Option<String>,
    /// TEXT timestamp (§2.5 class).
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
