//! `cleanup_plan_account_etags` — per-account drift-detection snapshots (migration 024).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cleanup_plan_account_etags")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub plan_id: Vec<u8>,
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: Vec<u8>,
    /// `'gmail_history' | 'outlook_delta' | 'imap_uvms' | 'pop3_sentinel' | 'none'`.
    pub etag_kind: String,
    /// JSON-serialized `AccountStateEtag` payload.
    pub etag_value: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
