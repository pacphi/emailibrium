//! `emails` — the core email store (migrations 001, 016, 018, 021, 027).
//!
//! Temporal columns follow ADR-036's shape rule: `received_at`/`embedded_at` are
//! plain `TIMESTAMP` (no zone) in both dialects, so they are `NaiveDateTime` here —
//! writes bind `.naive_utc()` and reads on the SQLite path stay lenient (sqlx tries
//! RFC3339 first, so rows written by the pre-port `to_rfc3339()` String binds still
//! decode; see ADR-035 §2.6). `deleted_at` is TEXT by design (016) and stays a
//! String the application formats. `created_at`/`updated_at` are DB-default-managed
//! and no call site reads them, so this partial model omits them.
//!
//! NOTE (SQLite wire format): post-port writes store `received_at` in sqlx's naive
//! `%F %T%.f` shape rather than the RFC3339 shape the old String binds produced.
//! Both shapes order correctly against each other across different dates; same-day
//! mixed-shape rows can interleave in string ORDER BY. The queued
//! `db-schema-modernization` pipeline (TIMESTAMPTZ normalization) retires this
//! caveat for good.
//!
//! `is_spam`/`is_trash` are INTEGER (016) while `is_read`/`is_starred`/
//! `has_attachments`/`is_archived` are BOOLEAN (001/027) — the split typing is
//! load-bearing on PostgreSQL (`boolean = integer` is a type error there), so the
//! entity mirrors the DDL exactly rather than normalizing to `bool`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "emails")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub account_id: String,
    pub provider: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub subject: String,
    pub from_addr: String,
    pub from_name: Option<String>,
    pub to_addrs: String,
    pub cc_addrs: Option<String>,
    pub bcc_addrs: Option<String>,
    /// Plain `TIMESTAMP` (no zone) in both dialects — see the module doc.
    pub received_at: chrono::NaiveDateTime,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub labels: Option<String>,
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
    pub has_attachments: Option<bool>,
    pub embedding_status: Option<String>,
    pub embedded_at: Option<chrono::NaiveDateTime>,
    pub embedding_model: Option<String>,
    pub vector_id: Option<String>,
    pub category: Option<String>,
    /// REAL in both dialects — 4-byte float on PostgreSQL.
    pub category_confidence: Option<f32>,
    pub category_method: Option<String>,
    /// TEXT by design (016): RFC3339 written by the application, compared as text.
    pub deleted_at: Option<String>,
    /// INTEGER 0/1 flag (016) — not BOOLEAN; see the module doc.
    pub is_spam: i32,
    /// INTEGER 0/1 flag (016) — not BOOLEAN; see the module doc.
    pub is_trash: i32,
    pub folder: String,
    pub list_unsubscribe: Option<String>,
    pub list_unsubscribe_post: Option<String>,
    pub thread_key: Option<String>,
    pub is_archived: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
