//! Re-indexing orchestrator for model changes (ADR-013).
//!
//! When the active embedding model changes between runs, all existing
//! embeddings become stale and must be regenerated. This module detects
//! that situation via the `ai_metadata` table and orchestrates the
//! re-indexing process.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
pub struct ReindexOrchestrator {
    db: Arc<Database>,
    status: Arc<RwLock<ReindexStatus>>,
}

impl ReindexOrchestrator {
    /// Create a new orchestrator backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
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
        let stored: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM ai_metadata WHERE key = 'active_embedding_model'",
        )
        .fetch_optional(&self.db.pool)
        .await
        .map_err(VectorError::DatabaseError)?;

        match stored {
            Some((stored_model,)) if stored_model != current_model => {
                tracing::warn!(
                    "Embedding model changed from '{}' to '{}'. Re-indexing required.",
                    stored_model,
                    current_model
                );
                sqlx::query(
                    "INSERT OR REPLACE INTO ai_metadata (key, value, updated_at) \
                     VALUES ('active_embedding_model', ?, datetime('now'))",
                )
                .bind(current_model)
                .execute(&self.db.pool)
                .await
                .map_err(VectorError::DatabaseError)?;
                Ok(true)
            }
            None => {
                // First run -- store current model, no re-index needed.
                sqlx::query(
                    "INSERT INTO ai_metadata (key, value, updated_at) \
                     VALUES ('active_embedding_model', ?, datetime('now'))",
                )
                .bind(current_model)
                .execute(&self.db.pool)
                .await
                .map_err(VectorError::DatabaseError)?;
                Ok(false)
            }
            _ => Ok(false), // same model, no re-index
        }
    }

    /// Mark all previously-embedded emails as stale so the ingestion pipeline
    /// will re-embed them with the new model.
    pub async fn mark_all_stale(&self) -> Result<u64, VectorError> {
        let result = sqlx::query(
            "UPDATE emails SET embedding_status = 'stale' WHERE embedding_status = 'embedded'",
        )
        .execute(&self.db.pool)
        .await
        .map_err(VectorError::DatabaseError)?;

        let count = result.rows_affected();

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
    use super::*;

    async fn test_db() -> Database {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        // Run the initial schema migration.
        sqlx::query(include_str!("../../migrations/001_initial_schema.sql"))
            .execute(&db.pool)
            .await
            .unwrap();
        // Run the ai_metadata migration.
        sqlx::query(include_str!("../../migrations/003_ai_metadata.sql"))
            .execute(&db.pool)
            .await
            .unwrap();
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

        // Insert some emails with 'embedded' status.
        for i in 0..5 {
            let id = format!("email-{}", i);
            sqlx::query(
                "INSERT INTO emails (id, account_id, provider, embedding_status) \
                 VALUES (?, 'acct-1', 'test', 'embedded')",
            )
            .bind(&id)
            .execute(&db.pool)
            .await
            .unwrap();
        }
        // One pending email that should not be affected.
        sqlx::query(
            "INSERT INTO emails (id, account_id, provider, embedding_status) \
             VALUES ('email-pending', 'acct-1', 'test', 'pending')",
        )
        .execute(&db.pool)
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
        let stale: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM emails WHERE embedding_status = 'stale'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(stale.0, 5);

        // Pending email should be unchanged.
        let pending: (String,) = sqlx::query_as(
            "SELECT embedding_status FROM emails WHERE id = 'email-pending'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(pending.0, "pending");
    }
}
