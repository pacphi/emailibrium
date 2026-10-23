//! `attachments` — per-email attachment metadata (migration 014).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "attachments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub email_id: String,
    pub account_id: String,
    pub filename: String,
    pub content_type: String,
    /// INTEGER — a real 4-byte int on PostgreSQL (the ADR-035 width class the old
    /// `i64` decode tripped over).
    pub size_bytes: i32,
    pub is_inline: bool,
    pub content_id: Option<String>,
    pub storage_path: Option<String>,
    pub provider_attachment_id: Option<String>,
    pub fetch_status: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
