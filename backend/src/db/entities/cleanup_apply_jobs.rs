//! `cleanup_apply_jobs` — apply-run lifecycle records (migration 024, ADR-030 §C).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cleanup_apply_jobs")]
pub struct Model {
    /// UUID v7, 16 raw bytes.
    #[sea_orm(primary_key, auto_increment = false)]
    pub job_id: Vec<u8>,
    pub plan_id: Vec<u8>,
    /// Unix millis.
    pub started_at: i64,
    /// Unix millis; `None` while the job is still running.
    pub finished_at: Option<i64>,
    /// `'queued' | 'running' | 'finished' | 'cancelled' | 'failed'`.
    pub state: String,
    /// `'low' | 'medium' | 'high'`.
    pub risk_max: String,
    pub counts_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
