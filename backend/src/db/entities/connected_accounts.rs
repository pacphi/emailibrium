//! `connected_accounts` — linked mailbox accounts (migrations 004, 013, 028).
//!
//! All temporal columns here are TEXT by design (ADR-035 §2.5): the DDL default is
//! `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')` and downstream code
//! parses the strings itself (RFC3339 first, `%Y-%m-%d %H:%M:%S` fallback). The
//! entity therefore carries `String` and the application owns the format.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "connected_accounts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub provider: String,
    pub email_address: String,
    pub encrypted_access_token: Option<Vec<u8>>,
    pub encrypted_refresh_token: Option<Vec<u8>>,
    /// TEXT — RFC3339 written/parsed by the application.
    pub token_expires_at: Option<String>,
    pub status: String,
    pub archive_strategy: String,
    pub label_prefix: String,
    /// TEXT timestamp (§2.5 class) — see the module doc.
    pub created_at: String,
    /// TEXT timestamp (§2.5 class) — see the module doc.
    pub updated_at: String,
    pub sync_depth: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub sync_frequency: i32,
    pub imap_host: Option<String>,
    pub imap_port: Option<i32>,
    pub imap_encryption: Option<String>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
