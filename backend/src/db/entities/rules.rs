//! `rules` — user-defined email rules (migrations 012, 026).
//!
//! `created_at`/`updated_at`/`last_run_at` are `TIMESTAMPTZ` on PostgreSQL, so the
//! entity carries `DateTime<Utc>` — the same type the pre-port code already bound
//! and decoded on both backends. `enabled` is INTEGER (not BOOLEAN) in both
//! dialects; the i32 typing here is what collapses `save_rule`'s two arms (their
//! SQL text was identical — only this bind's type differed).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "rules")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub conditions_json: String,
    pub actions_json: String,
    pub priority: Option<i32>,
    /// INTEGER 0/1 flag in both dialects — not BOOLEAN.
    pub enabled: Option<i32>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// INTEGER — 4-byte on PostgreSQL (the old `i64` decode of COALESCE(match_count,0)
    /// was the ADR-035 width class).
    pub match_count: i32,
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
