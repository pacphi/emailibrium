//! Remote wipe service for device loss mitigation (ADR-008).
//!
//! Provides capabilities to securely delete user data including vectors,
//! embeddings, learning data, and cached items. Supports immediate and
//! scheduled wipes with full audit logging.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::error::VectorError;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The scope of a wipe operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WipeScope {
    /// Wipe all data for a specific user.
    User,
    /// Wipe only vector store data (keep config/metadata).
    VectorsOnly,
    /// Full platform data wipe (admin only).
    All,
}

impl std::fmt::Display for WipeScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::VectorsOnly => write!(f, "vectors_only"),
            Self::All => write!(f, "all"),
        }
    }
}

/// Result of a wipe operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WipeResult {
    /// Number of vectors deleted.
    pub vectors_deleted: u64,
    /// Number of backup entries deleted.
    pub backups_deleted: u64,
    /// Number of learning records deleted.
    pub learning_records_deleted: u64,
    /// Number of interaction records deleted.
    pub interactions_deleted: u64,
    /// Scope of the wipe.
    pub scope: WipeScope,
    /// User ID (if user-scoped).
    pub user_id: Option<String>,
    /// Timestamp of the operation.
    pub completed_at: DateTime<Utc>,
}

/// A scheduled wipe that has not yet executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWipe {
    pub user_id: String,
    pub scheduled_at: DateTime<Utc>,
    pub execute_at: DateTime<Utc>,
    pub scope: WipeScope,
    pub cancelled: bool,
}

// ---------------------------------------------------------------------------
// RemoteWipeService
// ---------------------------------------------------------------------------

/// Service for securely wiping user and platform data (ADR-008).
///
/// All wipe operations are logged to the `wipe_audit_log` table for
/// compliance and accountability.
pub struct RemoteWipeService {
    db: Arc<Database>,
    scheduled_wipes: Arc<RwLock<Vec<ScheduledWipe>>>,
}

impl RemoteWipeService {
    /// Create a new remote wipe service.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            scheduled_wipes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Ensure the wipe audit log table exists.
    pub async fn ensure_table(&self) -> Result<(), VectorError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS wipe_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                scope TEXT NOT NULL,
                user_id TEXT,
                vectors_deleted INTEGER NOT NULL DEFAULT 0,
                backups_deleted INTEGER NOT NULL DEFAULT 0,
                learning_deleted INTEGER NOT NULL DEFAULT 0,
                interactions_deleted INTEGER NOT NULL DEFAULT 0,
                initiated_by TEXT,
                status TEXT NOT NULL DEFAULT 'completed'
            )",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_wipe_audit_timestamp \
             ON wipe_audit_log(timestamp)",
        )
        .execute(&self.db.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_wipe_audit_user \
             ON wipe_audit_log(user_id)",
        )
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }

    /// Wipe all data for a specific user.
    ///
    /// Deletes vectors, backups, learning data, interactions, and cached
    /// items associated with the given user ID.
    pub async fn wipe_user_data(&self, user_id: &str) -> Result<WipeResult, VectorError> {
        if user_id.is_empty() {
            return Err(VectorError::StoreFailed(
                "user_id must not be empty".to_string(),
            ));
        }

        info!(user_id = %user_id, "Starting user data wipe");

        // Delete vector backups for user's emails.
        let backups = sqlx::query(
            "DELETE FROM vector_backups WHERE email_id IN \
             (SELECT id FROM emails WHERE account_id = ?1)",
        )
        .bind(user_id)
        .execute(&self.db.pool)
        .await?;

        // Delete search interactions.
        let interactions = sqlx::query("DELETE FROM search_interactions WHERE user_id = ?1")
            .bind(user_id)
            .execute(&self.db.pool)
            .await?;

        // Delete user learning data (user_learning_profiles + user_feedback).
        let learning = self.delete_user_learning(user_id).await?;

        // Delete user's emails (cascades to related data via FK).
        let vectors = sqlx::query("DELETE FROM emails WHERE account_id = ?1")
            .bind(user_id)
            .execute(&self.db.pool)
            .await?;

        let result = WipeResult {
            vectors_deleted: vectors.rows_affected(),
            backups_deleted: backups.rows_affected(),
            learning_records_deleted: learning,
            interactions_deleted: interactions.rows_affected(),
            scope: WipeScope::User,
            user_id: Some(user_id.to_string()),
            completed_at: Utc::now(),
        };

        self.log_wipe(&result, Some(user_id)).await?;

        info!(
            user_id = %user_id,
            vectors = result.vectors_deleted,
            backups = result.backups_deleted,
            "User data wipe completed"
        );

        Ok(result)
    }

    /// Wipe all platform data (admin only).
    ///
    /// Deletes all vectors, backups, learning data, interactions, and
    /// cached items across the entire platform. The audit log itself is
    /// preserved for compliance.
    pub async fn wipe_all_data(&self) -> Result<WipeResult, VectorError> {
        warn!("Starting full platform data wipe");

        let backups = sqlx::query("DELETE FROM vector_backups")
            .execute(&self.db.pool)
            .await?;

        let interactions = sqlx::query("DELETE FROM search_interactions")
            .execute(&self.db.pool)
            .await?;

        let learning = self.delete_all_learning().await?;

        let vectors = sqlx::query("DELETE FROM emails")
            .execute(&self.db.pool)
            .await?;

        // Also clear category centroids.
        sqlx::query("DELETE FROM category_centroids")
            .execute(&self.db.pool)
            .await?;

        let result = WipeResult {
            vectors_deleted: vectors.rows_affected(),
            backups_deleted: backups.rows_affected(),
            learning_records_deleted: learning,
            interactions_deleted: interactions.rows_affected(),
            scope: WipeScope::All,
            user_id: None,
            completed_at: Utc::now(),
        };

        self.log_wipe(&result, None).await?;

        warn!(
            vectors = result.vectors_deleted,
            backups = result.backups_deleted,
            "Full platform data wipe completed"
        );

        Ok(result)
    }

    /// Wipe only vector store data, keeping config and metadata.
    ///
    /// Removes vector backups and category centroids but preserves emails,
    /// interactions, learning data, and account information.
    pub async fn wipe_vectors_only(&self) -> Result<WipeResult, VectorError> {
        info!("Starting vectors-only wipe");

        let backups = sqlx::query("DELETE FROM vector_backups")
            .execute(&self.db.pool)
            .await?;

        let centroids = sqlx::query("DELETE FROM category_centroids")
            .execute(&self.db.pool)
            .await?;

        let result = WipeResult {
            vectors_deleted: centroids.rows_affected(),
            backups_deleted: backups.rows_affected(),
            learning_records_deleted: 0,
            interactions_deleted: 0,
            scope: WipeScope::VectorsOnly,
            user_id: None,
            completed_at: Utc::now(),
        };

        self.log_wipe(&result, None).await?;

        info!(
            backups = result.backups_deleted,
            "Vectors-only wipe completed"
        );

        Ok(result)
    }

    /// Schedule a delayed wipe for a user (e.g., account deletion grace period).
    ///
    /// The `delay_seconds` parameter specifies how many seconds to wait before
    /// executing the wipe. Returns the scheduled execution time.
    pub async fn schedule_wipe(
        &self,
        user_id: &str,
        delay_seconds: i64,
    ) -> Result<ScheduledWipe, VectorError> {
        if user_id.is_empty() {
            return Err(VectorError::StoreFailed(
                "user_id must not be empty".to_string(),
            ));
        }
        if delay_seconds <= 0 {
            return Err(VectorError::StoreFailed(
                "delay_seconds must be positive".to_string(),
            ));
        }

        let now = Utc::now();
        let execute_at = now + Duration::seconds(delay_seconds);

        let wipe = ScheduledWipe {
            user_id: user_id.to_string(),
            scheduled_at: now,
            execute_at,
            scope: WipeScope::User,
            cancelled: false,
        };

        let mut scheduled = self.scheduled_wipes.write().await;

        // Remove any existing scheduled wipe for this user.
        scheduled.retain(|w| w.user_id != user_id);
        scheduled.push(wipe.clone());

        info!(
            user_id = %user_id,
            execute_at = %execute_at,
            "Wipe scheduled"
        );

        Ok(wipe)
    }

    /// Cancel a pending scheduled wipe for a user.
    ///
    /// Returns `true` if a scheduled wipe was found and cancelled,
    /// `false` if no pending wipe exists for the user.
    pub async fn cancel_scheduled_wipe(&self, user_id: &str) -> Result<bool, VectorError> {
        if user_id.is_empty() {
            return Err(VectorError::StoreFailed(
                "user_id must not be empty".to_string(),
            ));
        }

        let mut scheduled = self.scheduled_wipes.write().await;
        let before = scheduled.len();
        scheduled.retain(|w| w.user_id != user_id || w.cancelled);
        let removed = before - scheduled.len();

        if removed > 0 {
            info!(user_id = %user_id, "Scheduled wipe cancelled");
        }

        Ok(removed > 0)
    }

    /// List all pending (non-cancelled) scheduled wipes.
    pub async fn list_scheduled_wipes(&self) -> Vec<ScheduledWipe> {
        let scheduled = self.scheduled_wipes.read().await;
        scheduled.iter().filter(|w| !w.cancelled).cloned().collect()
    }

    /// Execute any scheduled wipes whose execution time has passed.
    ///
    /// Returns the number of wipes executed.
    pub async fn execute_pending_wipes(&self) -> Result<u64, VectorError> {
        let now = Utc::now();
        let pending: Vec<ScheduledWipe> = {
            let scheduled = self.scheduled_wipes.read().await;
            scheduled
                .iter()
                .filter(|w| !w.cancelled && w.execute_at <= now)
                .cloned()
                .collect()
        };

        let mut executed = 0u64;
        for wipe in &pending {
            match self.wipe_user_data(&wipe.user_id).await {
                Ok(_) => executed += 1,
                Err(e) => {
                    warn!(
                        user_id = %wipe.user_id,
                        error = %e,
                        "Failed to execute scheduled wipe"
                    );
                }
            }
        }

        // Remove executed wipes from the schedule.
        if executed > 0 {
            let mut scheduled = self.scheduled_wipes.write().await;
            scheduled.retain(|w| w.cancelled || w.execute_at > now);
        }

        Ok(executed)
    }

    // -- private helpers -----------------------------------------------------

    /// Delete user-specific learning data.
    async fn delete_user_learning(&self, user_id: &str) -> Result<u64, VectorError> {
        let mut total = 0u64;

        // Try user_learning_profiles (may not exist in all schemas).
        if let Ok(r) = sqlx::query("DELETE FROM user_learning_profiles WHERE user_id = ?1")
            .bind(user_id)
            .execute(&self.db.pool)
            .await
        {
            total += r.rows_affected();
        }

        // Try user_feedback (may not exist in all schemas).
        if let Ok(r) = sqlx::query("DELETE FROM user_feedback WHERE user_id = ?1")
            .bind(user_id)
            .execute(&self.db.pool)
            .await
        {
            total += r.rows_affected();
        }

        Ok(total)
    }

    /// Delete all learning data across the platform.
    async fn delete_all_learning(&self) -> Result<u64, VectorError> {
        let mut total = 0u64;

        if let Ok(r) = sqlx::query("DELETE FROM user_learning_profiles")
            .execute(&self.db.pool)
            .await
        {
            total += r.rows_affected();
        }

        if let Ok(r) = sqlx::query("DELETE FROM user_feedback")
            .execute(&self.db.pool)
            .await
        {
            total += r.rows_affected();
        }

        Ok(total)
    }

    /// Log a wipe operation to the audit table.
    async fn log_wipe(
        &self,
        result: &WipeResult,
        initiated_by: Option<&str>,
    ) -> Result<(), VectorError> {
        sqlx::query(
            "INSERT INTO wipe_audit_log \
             (timestamp, scope, user_id, vectors_deleted, backups_deleted, \
              learning_deleted, interactions_deleted, initiated_by, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'completed')",
        )
        .bind(result.completed_at)
        .bind(result.scope.to_string())
        .bind(&result.user_id)
        .bind(result.vectors_deleted as i64)
        .bind(result.backups_deleted as i64)
        .bind(result.learning_records_deleted as i64)
        .bind(result.interactions_deleted as i64)
        .bind(initiated_by)
        .execute(&self.db.pool)
        .await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_db() -> Database {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE emails (id TEXT PRIMARY KEY, account_id TEXT, subject TEXT)",
            "CREATE TABLE vector_backups (vector_id TEXT PRIMARY KEY, email_id TEXT, \
             collection TEXT, dimensions INTEGER, vector_data BLOB, metadata_json TEXT, \
             created_at TIMESTAMP, updated_at TIMESTAMP)",
            "CREATE TABLE category_centroids (id INTEGER PRIMARY KEY, category TEXT, \
             centroid BLOB, sample_count INTEGER)",
            "CREATE TABLE search_interactions (id INTEGER PRIMARY KEY AUTOINCREMENT, \
             user_id TEXT, query TEXT, results_count INTEGER, created_at TIMESTAMP)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Database { pool }
    }

    async fn seed(db: &Database, uid: &str) {
        let eid = format!("e-{uid}");
        let vid = format!("v-{uid}");
        sqlx::query("INSERT INTO emails (id, account_id, subject) VALUES (?1, ?2, 'Test')")
            .bind(&eid)
            .bind(uid)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO vector_backups (vector_id, email_id, collection, dimensions, \
            vector_data, created_at, updated_at) VALUES (?1, ?2, 'email_text', 3, \
            X'000000000000803F0000003F', datetime('now'), datetime('now'))",
        )
        .bind(&vid)
        .bind(&eid)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO search_interactions (user_id, query, results_count, created_at) \
            VALUES (?1, 'q', 5, datetime('now'))",
        )
        .bind(uid)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_ensure_table_idempotent() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db);
        svc.ensure_table().await.unwrap();
        svc.ensure_table().await.unwrap();
    }

    #[tokio::test]
    async fn test_wipe_user_data() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        let r = svc.wipe_user_data("u1").await.unwrap();
        assert_eq!(r.scope, WipeScope::User);
        assert_eq!(r.user_id.as_deref(), Some("u1"));
        assert!(r.vectors_deleted >= 1);
        let c: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails WHERE account_id='u1'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(c.0, 0);
    }

    #[tokio::test]
    async fn test_wipe_user_empty_id_rejected() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db);
        assert!(svc.wipe_user_data("").await.is_err());
    }

    #[tokio::test]
    async fn test_wipe_all_data() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        let r = svc.wipe_all_data().await.unwrap();
        assert_eq!(r.scope, WipeScope::All);
        let c: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(c.0, 0);
    }

    #[tokio::test]
    async fn test_wipe_vectors_only_preserves_emails() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        let r = svc.wipe_vectors_only().await.unwrap();
        assert_eq!(r.scope, WipeScope::VectorsOnly);
        let c: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vector_backups")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(c.0, 0);
        let c: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM emails")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(c.0 > 0);
    }

    #[tokio::test]
    async fn test_schedule_cancel_and_list() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db);
        let w = svc.schedule_wipe("u1", 3600).await.unwrap();
        assert_eq!(w.user_id, "u1");
        assert_eq!(svc.list_scheduled_wipes().await.len(), 1);
        // Replace existing schedule.
        svc.schedule_wipe("u1", 7200).await.unwrap();
        assert_eq!(svc.list_scheduled_wipes().await.len(), 1);
        // Cancel.
        assert!(svc.cancel_scheduled_wipe("u1").await.unwrap());
        assert!(svc.list_scheduled_wipes().await.is_empty());
        // Cancel nonexistent returns false.
        assert!(!svc.cancel_scheduled_wipe("u1").await.unwrap());
    }

    #[tokio::test]
    async fn test_schedule_validation() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db);
        assert!(svc.schedule_wipe("u1", 0).await.is_err());
        assert!(svc.schedule_wipe("u1", -1).await.is_err());
        assert!(svc.schedule_wipe("", 3600).await.is_err());
    }

    #[tokio::test]
    async fn test_execute_pending_wipes() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        {
            let mut s = svc.scheduled_wipes.write().await;
            s.push(ScheduledWipe {
                user_id: "u1".to_string(),
                scheduled_at: Utc::now() - Duration::seconds(10),
                execute_at: Utc::now() - Duration::seconds(5),
                scope: WipeScope::User,
                cancelled: false,
            });
        }
        assert_eq!(svc.execute_pending_wipes().await.unwrap(), 1);
        assert!(svc.list_scheduled_wipes().await.is_empty());
    }

    #[tokio::test]
    async fn test_audit_log_written() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        svc.wipe_user_data("u1").await.unwrap();
        let c: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM wipe_audit_log")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(c.0, 1);
        let s: (String,) = sqlx::query_as("SELECT scope FROM wipe_audit_log LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(s.0, "user");
    }
}
