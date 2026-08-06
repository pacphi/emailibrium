//! `privacy_audit_log` — GDPR data-access/erasure audit (migration 010).
//!
//! `created_at` is `TIMESTAMPTZ` on PostgreSQL — `DateTime<Utc>` here.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "privacy_audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub event_type: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub actor: Option<String>,
    /// TEXT holding JSON serialized by the application.
    pub details: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
