//! `mcp_tool_audit` — best-effort MCP tool-call audit trail (migrations 022, 029).
//!
//! `timestamp` is TEXT (RFC3339 written by the application). `latency_ms` is
//! INTEGER — the old `as i64` bind was the ADR-035 width class and failed silently
//! on PostgreSQL because the insert is best-effort.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "mcp_tool_audit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// TEXT — RFC3339 written by the application.
    pub timestamp: String,
    pub tool_name: String,
    pub arguments_hash: String,
    pub result_status: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub latency_ms: i32,
    pub source: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
