//! `sync_conflicts` — queue-operation conflicts awaiting resolution (migration 011).
//!
//! Same temporal shape as `sync_queue`: `TIMESTAMPTZ` on PostgreSQL → `DateTime<Utc>`
//! here, collapsing the pre-port per-backend row types. `local_state`/`remote_state`
//! are TEXT holding JSON serialized by the application — deliberately NOT a JSON
//! column type, which would change the PostgreSQL storage shape.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sync_conflicts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub queue_entry_id: String,
    pub local_state: String,
    pub remote_state: String,
    pub resolution: Option<String>,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
