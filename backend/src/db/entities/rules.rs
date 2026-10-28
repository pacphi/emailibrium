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
    /// 'suggest' | 'trusted' (migration 032). Level 0 Observe and level 3
    /// Act are not rule states; see `rules::authority`.
    pub authority_level: Option<String>,
    pub clean_streak: Option<i32>,
    /// TEXT, not a native timestamp type, matching `connected_accounts`'
    /// `role_declared_at` (migration 030, ADR-035 2.5).
    pub graduated_at: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
