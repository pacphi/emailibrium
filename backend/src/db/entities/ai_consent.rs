//! `ai_consent` — per-provider cloud-AI consent (migration 002).
//!
//! `consented_at`/`revoked_at` are plain `TIMESTAMP` (no zone) in both dialects —
//! `NaiveDateTime` here, with writes binding `.naive_utc()`. This closes the
//! write-side half of the pl-timestamp-write-tz gap: a `DateTime<Utc>` bind on
//! PostgreSQL assignment-casts through the session TimeZone GUC, silently shifting
//! stored values on any non-UTC session; a naive bind cannot shift.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ai_consent")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub provider: String,
    pub consented_at: chrono::NaiveDateTime,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub acknowledgment: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
