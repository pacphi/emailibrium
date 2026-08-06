//! Re-indexing orchestrator for model changes (ADR-013).
//!
//! When the active embedding model changes between runs, all existing
//! embeddings become stale and must be regenerated. This module detects
//! that situation via the `ai_metadata` table and orchestrates the
//! re-indexing process.

use std::sync::Arc;

use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::db::entities::{ai_metadata, emails};
use crate::db::Database;

use super::error::VectorError;

/// Snapshot of re-indexing progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexStatus {
    /// Whether a re-index is currently running.
    pub in_progress: bool,
    /// Total emails that need re-embedding.
    pub total_emails: u64,
    /// Emails marked stale awaiting new embeddings.
    pub stale_emails: u64,
    /// Emails successfully re-embedded so far.
    pub reindexed_emails: u64,
    /// Progress as a percentage (0.0 -- 100.0).
    pub progress_percent: f32,
    /// Estimated seconds remaining, if calculable.
    pub estimated_remaining_secs: Option<u64>,
    /// Human-readable reason for the re-index.
    pub reason: Option<String>,
}

impl Default for ReindexStatus {
    fn default() -> Self {
        Self {
            in_progress: false,
            total_emails: 0,
            stale_emails: 0,
            reindexed_emails: 0,
            progress_percent: 0.0,
            estimated_remaining_secs: None,
            reason: None,
        }
    }
}

/// Orchestrates re-indexing when the embedding model changes.
///
/// Persistence is single-code-path SeaORM (ADR-036).
pub struct ReindexOrchestrator {
    conn: DatabaseConnection,
    status: Arc<RwLock<ReindexStatus>>,
}

impl ReindexOrchestrator {
    /// Create a new orchestrator backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            conn: db.sea_orm(),
            status: Arc::new(RwLock::new(ReindexStatus::default())),
        }
    }

    /// Check whether the active model has changed since last startup.
    ///
    /// On first run the current model is stored and `Ok(false)` is returned.
    /// On subsequent runs, if the stored model differs from `current_model`,
    /// the stored value is updated and `Ok(true)` is returned to signal that
    /// a full re-index is required.
    pub async fn check_model_change(
        &self,
        current_model: &str,
        _current_dims: usize,
    ) -> Result<bool, VectorError> {
        let stored: Option<(String,)> = ai_metadata::Entity::find()
            .select_only()
            .column(ai_metadata::Column::Value)
            .filter(ai_metadata::Column::Key.eq("active_embedding_model"))
            .into_tuple()
            .one(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        // One upsert body serves both the first-run INSERT and the
        // model-change `INSERT OR REPLACE` (for this single-column-key KV row
        // the replace and DO-UPDATE forms are observably identical).
        // `updated_at` is a real TIMESTAMP in both dialects (migration 003),
        // so the old `datetime('now')` literal becomes a naive-UTC bind.
        let upsert = ai_metadata::Entity::insert(ai_metadata::ActiveModel {
            key: Set("active_embedding_model".to_owned()),
            value: Set(current_model.to_owned()),
            updated_at: Set(Some(chrono::Utc::now().naive_utc())),
        })
        .on_conflict(
            OnConflict::column(ai_metadata::Column::Key)
                .update_columns([ai_metadata::Column::Value, ai_metadata::Column::UpdatedAt])
                .to_owned(),
        );

        match stored {
            Some((stored_model,)) if stored_model != current_model => {
                tracing::warn!(
                    "Embedding model changed from '{}' to '{}'. Re-indexing required.",
                    stored_model,
                    current_model
                );
                upsert
                    .exec_without_returning(&self.conn)
                    .await
                    .map_err(VectorError::Db)?;
                Ok(true)
            }
            None => {
                // First run -- store current model, no re-index needed.
                upsert
                    .exec_without_returning(&self.conn)
                    .await
                    .map_err(VectorError::Db)?;
                Ok(false)
            }
            _ => Ok(false), // same model, no re-index
        }
    }

    /// Mark all previously-embedded emails as stale so the ingestion pipeline
    /// will re-embed them with the new model.
    pub async fn mark_all_stale(&self) -> Result<u64, VectorError> {
        let result = emails::Entity::update_many()
            .col_expr(emails::Column::EmbeddingStatus, Expr::value("stale"))
            .filter(emails::Column::EmbeddingStatus.eq("embedded"))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let count = result.rows_affected;

        let mut status = self.status.write().await;
        status.in_progress = true;
        status.stale_emails = count;
        status.total_emails = count;
        status.reason = Some("Model changed".to_string());

        Ok(count)
    }

    /// Return the current re-index status snapshot.
    pub async fn get_status(&self) -> ReindexStatus {
        self.status.read().await.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, PaginatorTrait};

    use super::*;

    async fn test_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/003_ai_metadata.sql"),
            include_str!("../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../migrations/sqlite/027_is_archived.sql"),
        ] {
            let cleaned: String = raw
                .lines()
                .map(|l| {
                    if let Some(idx) = l.find("--") {
                        &l[..idx]
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in cleaned.split(';') {
                let s = stmt.trim();
                if !s.is_empty() {
                    conn.execute_unprepared(s).await.unwrap();
                }
            }
        }
        db
    }

    #[tokio::test]
    async fn test_reindex_check_first_run() {
        let db = Arc::new(test_db().await);
        let orchestrator = ReindexOrchestrator::new(db);

        // First run: should store the model and return false.
        let needs = orchestrator
            .check_model_change("all-MiniLM-L6-v2", 384)
            .await
            .unwrap();
        assert!(!needs, "First run should not require re-index");
    }

    #[tokio::test]
    async fn test_reindex_check_same_model() {
        let db = Arc::new(test_db().await);
        let orchestrator = ReindexOrchestrator::new(db);

        // First run stores the model.
        orchestrator
            .check_model_change("all-MiniLM-L6-v2", 384)
            .await
            .unwrap();

        // Second run with same model: no re-index.
        let needs = orchestrator
            .check_model_change("all-MiniLM-L6-v2", 384)
            .await
            .unwrap();
        assert!(!needs, "Same model should not require re-index");
    }

    #[tokio::test]
    async fn test_reindex_check_model_changed() {
        let db = Arc::new(test_db().await);
        let orchestrator = ReindexOrchestrator::new(db);

        // First run: store model A.
        orchestrator
            .check_model_change("all-MiniLM-L6-v2", 384)
            .await
            .unwrap();

        // Second run: switch to model B.
        let needs = orchestrator
            .check_model_change("bge-small-en-v1.5", 384)
            .await
            .unwrap();
        assert!(needs, "Model change should require re-index");
    }

    #[tokio::test]
    async fn test_mark_all_stale() {
        let db = Arc::new(test_db().await);

        let conn = db.sea_orm();
        // Insert some emails with 'embedded' status.
        for i in 0..5 {
            conn.execute_unprepared(&format!(
                "INSERT INTO emails (id, account_id, provider, embedding_status) \
                 VALUES ('email-{i}', 'acct-1', 'test', 'embedded')"
            ))
            .await
            .unwrap();
        }
        // One pending email that should not be affected.
        conn.execute_unprepared(
            "INSERT INTO emails (id, account_id, provider, embedding_status) \
             VALUES ('email-pending', 'acct-1', 'test', 'pending')",
        )
        .await
        .unwrap();

        let orchestrator = ReindexOrchestrator::new(db.clone());
        let count = orchestrator.mark_all_stale().await.unwrap();
        assert_eq!(count, 5, "Should mark 5 embedded emails as stale");

        // Verify the status was updated.
        let status = orchestrator.get_status().await;
        assert!(status.in_progress);
        assert_eq!(status.stale_emails, 5);
        assert_eq!(status.total_emails, 5);

        // Verify the DB was updated.
        let stale = emails::Entity::find()
            .filter(emails::Column::EmbeddingStatus.eq("stale"))
            .count(&conn)
            .await
            .unwrap();
        assert_eq!(stale, 5);

        // Pending email should be unchanged.
        let pending: Option<(Option<String>,)> = emails::Entity::find()
            .select_only()
            .column(emails::Column::EmbeddingStatus)
            .filter(emails::Column::Id.eq("email-pending"))
            .into_tuple()
            .one(&conn)
            .await
            .unwrap();
        assert_eq!(pending.unwrap().0.as_deref(), Some("pending"));
    }
}
