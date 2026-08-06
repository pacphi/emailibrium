//! Remote wipe service for device loss mitigation (ADR-008).
//!
//! Provides capabilities to securely delete user data including vectors,
//! embeddings, learning data, and cached items. Supports immediate and
//! scheduled wipes with full audit logging.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::error::VectorError;
use crate::db::entities::{category_centroids, emails, search_interactions, vector_backups};
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
    conn: DatabaseConnection,
    scheduled_wipes: Arc<RwLock<Vec<ScheduledWipe>>>,
}

impl RemoteWipeService {
    /// Create a new remote wipe service.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            conn: db.sea_orm(),
            scheduled_wipes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Ensure the wipe audit log table exists.
    ///
    /// Raw-DDL escape hatch (ADR-036 §5): unlike `cloud_api_audit_log`,
    /// `wipe_audit_log` has no numbered migration — this method is the
    /// table's sole source of truth — and `id`'s auto-increment syntax
    /// genuinely differs per backend (ADR-035 §2.3), so the two DDL strings
    /// stay hand-written per backend.
    pub async fn ensure_table(&self) -> Result<(), VectorError> {
        let ddl = match self.conn.get_database_backend() {
            DatabaseBackend::Postgres => {
                "CREATE TABLE IF NOT EXISTS wipe_audit_log (
                    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    scope TEXT NOT NULL,
                    user_id TEXT,
                    vectors_deleted INTEGER NOT NULL DEFAULT 0,
                    backups_deleted INTEGER NOT NULL DEFAULT 0,
                    learning_deleted INTEGER NOT NULL DEFAULT 0,
                    interactions_deleted INTEGER NOT NULL DEFAULT 0,
                    initiated_by TEXT,
                    status TEXT NOT NULL DEFAULT 'completed'
                )"
            }
            _ => {
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
                )"
            }
        };
        self.conn
            .execute_unprepared(ddl)
            .await
            .map_err(VectorError::Db)?;

        for stmt in [
            "CREATE INDEX IF NOT EXISTS idx_wipe_audit_timestamp ON wipe_audit_log(timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_wipe_audit_user ON wipe_audit_log(user_id)",
        ] {
            self.conn
                .execute_unprepared(stmt)
                .await
                .map_err(VectorError::Db)?;
        }

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
        let backups = vector_backups::Entity::delete_many()
            .filter(
                vector_backups::Column::EmailId.in_subquery(
                    Query::select()
                        .column(emails::Column::Id)
                        .from(emails::Entity)
                        .and_where(emails::Column::AccountId.eq(user_id))
                        .to_owned(),
                ),
            )
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;

        // Delete search interactions. `user_id` exists in NO migration's
        // search_interactions table (001 has no such column), so on a real
        // migrated database this statement errors and the wipe fails here —
        // ported bug-for-bug; see pl-wipe-interactions-phantom-column.
        let interactions = search_interactions::Entity::delete_many()
            .filter(Expr::col(Alias::new("user_id")).eq(user_id))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;

        // `user_learning_profiles`/`user_feedback` exist in no migration; the
        // pre-port best-effort deletes always failed and contributed zero, so
        // the counter is the constant they always produced.
        let learning = 0u64;

        // Delete user's emails (cascades to related data via FK).
        let vectors = emails::Entity::delete_many()
            .filter(emails::Column::AccountId.eq(user_id))
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;

        let result = WipeResult {
            vectors_deleted: vectors,
            backups_deleted: backups,
            learning_records_deleted: learning,
            interactions_deleted: interactions,
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

        let backups = vector_backups::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;
        let interactions = search_interactions::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;
        // Phantom learning tables — see the note in `wipe_user_data`.
        let learning = 0u64;
        let vectors = emails::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;

        // Also clear category centroids.
        category_centroids::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?;

        let result = WipeResult {
            vectors_deleted: vectors,
            backups_deleted: backups,
            learning_records_deleted: learning,
            interactions_deleted: interactions,
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

        let backups = vector_backups::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;
        let centroids = category_centroids::Entity::delete_many()
            .exec(&self.conn)
            .await
            .map_err(VectorError::Db)?
            .rows_affected;

        let result = WipeResult {
            vectors_deleted: centroids,
            backups_deleted: backups,
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

    /// Log a wipe operation to the audit table.
    ///
    /// `wipe_audit_log` has no entity (no numbered migration — see
    /// `ensure_table`), so the insert is built directly with sea-query and
    /// rendered per backend. `timestamp` is a plain TIMESTAMP in both
    /// dialects, so the pre-port `DateTime<Utc>` bind becomes a naive-UTC
    /// bind (the write-side TZ fix class).
    async fn log_wipe(
        &self,
        result: &WipeResult,
        initiated_by: Option<&str>,
    ) -> Result<(), VectorError> {
        let stmt = Query::insert()
            .into_table(Alias::new("wipe_audit_log"))
            .columns([
                Alias::new("timestamp"),
                Alias::new("scope"),
                Alias::new("user_id"),
                Alias::new("vectors_deleted"),
                Alias::new("backups_deleted"),
                Alias::new("learning_deleted"),
                Alias::new("interactions_deleted"),
                Alias::new("initiated_by"),
                Alias::new("status"),
            ])
            .values_panic([
                Expr::value(result.completed_at.naive_utc()),
                Expr::value(result.scope.to_string()),
                Expr::value(result.user_id.clone()),
                Expr::value(result.vectors_deleted as i64),
                Expr::value(result.backups_deleted as i64),
                Expr::value(result.learning_records_deleted as i64),
                Expr::value(result.interactions_deleted as i64),
                Expr::value(initiated_by.map(str::to_owned)),
                Expr::value("completed"),
            ])
            .to_owned();

        self.conn.execute(&stmt).await.map_err(VectorError::Db)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use sea_orm::Statement;

    use super::*;

    async fn test_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        conn.execute_unprepared("PRAGMA foreign_keys = OFF")
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
            conn.execute_unprepared(ddl).await.unwrap();
        }
        db
    }

    async fn seed(db: &Database, uid: &str) {
        let conn = db.sea_orm();
        let eid = format!("e-{uid}");
        let vid = format!("v-{uid}");
        conn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO emails (id, account_id, subject) VALUES (?1, ?2, 'Test')",
            [eid.as_str().into(), uid.into()],
        ))
        .await
        .unwrap();
        conn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO vector_backups (vector_id, email_id, collection, dimensions, \
            vector_data, created_at, updated_at) VALUES (?1, ?2, 'email_text', 3, \
            X'000000000000803F0000003F', datetime('now'), datetime('now'))",
            [vid.as_str().into(), eid.as_str().into()],
        ))
        .await
        .unwrap();
        conn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO search_interactions (user_id, query, results_count, created_at) \
            VALUES (?1, 'q', 5, datetime('now'))",
            [uid.into()],
        ))
        .await
        .unwrap();
    }

    async fn count(db: &Database, sql: &str) -> i64 {
        db.sea_orm()
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                sql.to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
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
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM emails WHERE account_id='u1'").await,
            0
        );
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
        assert_eq!(count(&db, "SELECT COUNT(*) FROM emails").await, 0);
    }

    #[tokio::test]
    async fn test_wipe_vectors_only_preserves_emails() {
        let db = Arc::new(test_db().await);
        let svc = RemoteWipeService::new(db.clone());
        svc.ensure_table().await.unwrap();
        seed(&db, "u1").await;
        let r = svc.wipe_vectors_only().await.unwrap();
        assert_eq!(r.scope, WipeScope::VectorsOnly);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM vector_backups").await, 0);
        assert!(count(&db, "SELECT COUNT(*) FROM emails").await > 0);
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
        assert_eq!(count(&db, "SELECT COUNT(*) FROM wipe_audit_log").await, 1);
        let scope: String = db
            .sea_orm()
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT scope FROM wipe_audit_log LIMIT 1".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        assert_eq!(scope, "user");
    }
}
