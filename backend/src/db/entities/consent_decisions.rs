//! `consent_decisions` — GDPR consent ledger (migration 010).
//!
//! `granted_at`/`revoked_at`/`created_at` are `TIMESTAMPTZ` on PostgreSQL —
//! `DateTime<Utc>` here. `granted` is INTEGER (not BOOLEAN) in both dialects.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "consent_decisions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub consent_type: String,
    /// INTEGER 0/1 flag — not BOOLEAN.
    pub granted: i32,
    pub granted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
