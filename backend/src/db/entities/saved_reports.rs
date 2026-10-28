//! `saved_reports` — persisted copies of generated chief-of-staff reports
//! (migration 033).
//!
//! `generated_at` is TEXT, matching every other temporal column in this
//! schema (ADR-035 section 2.5): the application writes/parses the string
//! itself rather than relying on dialect-specific timestamp handling.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "saved_reports")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// 'brief' | 'review' | 'health'.
    pub kind: String,
    /// `None` when the report merges every connected account.
    pub account_id: Option<String>,
    pub generated_at: String,
    pub content_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
