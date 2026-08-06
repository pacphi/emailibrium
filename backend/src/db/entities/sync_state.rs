//! `sync_state` — per-account sync progress (migration 004).
//!
//! `last_sync_at`/`updated_at` are TEXT by design (ADR-035 §2.5) — `String` here,
//! format owned by the application.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sync_state")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: String,
    /// TEXT timestamp (§2.5 class).
    pub last_sync_at: Option<String>,
    pub history_id: Option<String>,
    pub next_page_token: Option<String>,
    /// INTEGER — 4-byte on PostgreSQL.
    pub emails_synced: i32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub sync_failures: i32,
    pub last_error: Option<String>,
    pub status: String,
    /// TEXT timestamp (§2.5 class).
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
