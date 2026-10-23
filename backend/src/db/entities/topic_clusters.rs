//! `topic_clusters` — discovered email topic clusters (migration 018).
//!
//! `created_at`/`updated_at` are TEXT (NOT NULL, always written by the
//! application) — `String` here. `email_ids`/`top_terms`/
//! `representative_email_ids` are TEXT holding JSON arrays; parsing happens in
//! Rust (the SQLite `json_each` unpacking they used to feed is replaced by a
//! Rust-side parse per ADR-036 §2.4's read-modify-write precedent).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "topic_clusters")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub description: String,
    pub centroid: Vec<u8>,
    /// TEXT holding a JSON array of email ids.
    pub email_ids: String,
    /// INTEGER — 4-byte on PostgreSQL.
    pub email_count: i32,
    /// TEXT holding a JSON array of {word, score, count}.
    pub top_terms: String,
    /// TEXT holding a JSON array of email ids.
    pub representative_email_ids: String,
    /// REAL — 4-byte float on PostgreSQL.
    pub stability_score: f32,
    /// INTEGER — 4-byte on PostgreSQL.
    pub stability_runs: i32,
    /// INTEGER 0/1 flag — not BOOLEAN.
    pub is_pinned: i32,
    /// TEXT timestamp, application-written.
    pub created_at: String,
    /// TEXT timestamp, application-written.
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
