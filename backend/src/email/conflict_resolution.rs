//! Conflict resolution for offline-first sync (R-02).
//!
//! When an offline operation is replayed against the remote provider and
//! the remote state has diverged, a conflict is detected. This module
//! implements configurable resolution strategies and persistent conflict
//! logging for manual review.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::offline_queue::QueuedOperation;
use crate::db::entities::sync_conflicts as conflicts;
use crate::db::Database;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Strategy for resolving sync conflicts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    /// Use the most recent timestamp (default).
    #[default]
    LastWriterWins,
    /// Always prefer local (offline) changes.
    LocalWins,
    /// Always prefer remote (server) state.
    RemoteWins,
    /// Queue for manual user resolution.
    Manual,
}

/// How a conflict was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    LocalWins,
    RemoteWins,
    Merged,
    ManualPending,
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalWins => "local_wins",
            Self::RemoteWins => "remote_wins",
            Self::Merged => "merged",
            Self::ManualPending => "manual_pending",
        }
    }
}

impl std::str::FromStr for Resolution {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local_wins" => Ok(Self::LocalWins),
            "remote_wins" => Ok(Self::RemoteWins),
            "merged" => Ok(Self::Merged),
            "manual_pending" => Ok(Self::ManualPending),
            other => Err(format!("Unknown resolution: {other}")),
        }
    }
}

/// A detected sync conflict between local and remote state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub id: String,
    pub queue_entry_id: String,
    pub local_state: serde_json::Value,
    pub remote_state: serde_json::Value,
    pub resolution: Option<Resolution>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Resolves and logs conflicts between offline operations and remote state.
pub struct ConflictResolver {
    conn: DatabaseConnection,
    strategy: ConflictStrategy,
}

impl ConflictResolver {
    /// Create a new conflict resolver with the given strategy.
    pub fn new(db: Database, strategy: ConflictStrategy) -> Self {
        Self {
            conn: db.sea_orm(),
            strategy,
        }
    }

    /// Create a resolver with the default strategy (LastWriterWins).
    pub fn with_defaults(db: Database) -> Self {
        Self::new(db, ConflictStrategy::default())
    }

    /// Get the configured strategy.
    pub fn strategy(&self) -> ConflictStrategy {
        self.strategy
    }

    /// Detect if an operation conflicts with remote state.
    ///
    /// Compares the operation's intent against the remote state JSON.
    /// Returns `Some(SyncConflict)` if a conflict is detected, `None` if
    /// the operation can proceed safely.
    pub fn detect_conflict(
        &self,
        operation: &QueuedOperation,
        remote_state: &serde_json::Value,
    ) -> Option<SyncConflict> {
        // A conflict exists when the remote state indicates the target
        // has been modified or deleted since the operation was queued.
        let remote_deleted = remote_state
            .get("deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let remote_modified = remote_state
            .get("modified_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|remote_ts| remote_ts > operation.created_at)
            .unwrap_or(false);

        if remote_deleted || remote_modified {
            let local_state = serde_json::json!({
                "operation": operation.operation_type.as_str(),
                "target_id": operation.target_id,
                "payload": operation.payload,
                "queued_at": operation.created_at.to_rfc3339(),
            });

            Some(SyncConflict {
                id: Uuid::new_v4().to_string(),
                queue_entry_id: operation.id.clone(),
                local_state,
                remote_state: remote_state.clone(),
                resolution: None,
                resolved_at: None,
                created_at: Utc::now(),
            })
        } else {
            None
        }
    }

    /// Resolve a conflict using the configured strategy.
    pub async fn resolve(&self, conflict: &SyncConflict) -> Result<Resolution, sea_orm::DbErr> {
        let resolution = match self.strategy {
            ConflictStrategy::LastWriterWins => {
                // Compare timestamps: local queued_at vs remote modified_at.
                let remote_ts = conflict
                    .remote_state
                    .get("modified_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

                let local_ts = conflict
                    .local_state
                    .get("queued_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

                match (local_ts, remote_ts) {
                    (Some(l), Some(r)) if l > r => Resolution::LocalWins,
                    _ => Resolution::RemoteWins,
                }
            }
            ConflictStrategy::LocalWins => Resolution::LocalWins,
            ConflictStrategy::RemoteWins => Resolution::RemoteWins,
            ConflictStrategy::Manual => Resolution::ManualPending,
        };

        // Persist the resolution.
        let resolution_str = if resolution == Resolution::ManualPending {
            None
        } else {
            Some(resolution.as_str().to_string())
        };

        let resolved_at = if resolution == Resolution::ManualPending {
            None
        } else {
            Some(Utc::now())
        };

        // update_many (not ActiveModel::update) so an unknown conflict id stays
        // a silent zero-rows no-op, exactly as the old UPDATE behaved.
        conflicts::Entity::update_many()
            .col_expr(conflicts::Column::Resolution, Expr::value(resolution_str))
            .col_expr(conflicts::Column::ResolvedAt, Expr::value(resolved_at))
            .filter(conflicts::Column::Id.eq(conflict.id.as_str()))
            .exec(&self.conn)
            .await?;

        Ok(resolution)
    }

    /// Log a conflict for review or later resolution.
    pub async fn log_conflict(&self, conflict: &SyncConflict) -> Result<(), sea_orm::DbErr> {
        let local_json = serde_json::to_string(&conflict.local_state).unwrap_or_default();
        let remote_json = serde_json::to_string(&conflict.remote_state).unwrap_or_default();
        let resolution_str = conflict.resolution.as_ref().map(|r| r.as_str().to_string());

        conflicts::Entity::insert(conflicts::ActiveModel {
            id: Set(conflict.id.clone()),
            queue_entry_id: Set(conflict.queue_entry_id.clone()),
            local_state: Set(local_json),
            remote_state: Set(remote_json),
            resolution: Set(resolution_str),
            resolved_at: Set(conflict.resolved_at),
            created_at: Set(Some(conflict.created_at)),
        })
        .exec_without_returning(&self.conn)
        .await?;

        Ok(())
    }

    /// Get unresolved conflicts for user review.
    pub async fn unresolved(&self) -> Result<Vec<SyncConflict>, sea_orm::DbErr> {
        let rows = conflicts::Entity::find()
            .filter(conflicts::Column::Resolution.is_null())
            .order_by_desc(conflicts::Column::CreatedAt)
            .all(&self.conn)
            .await?;

        Ok(rows.into_iter().map(map_conflict_model).collect())
    }

    /// Get all conflicts for a specific queue entry.
    pub async fn conflicts_for_entry(
        &self,
        queue_entry_id: &str,
    ) -> Result<Vec<SyncConflict>, sea_orm::DbErr> {
        let rows = conflicts::Entity::find()
            .filter(conflicts::Column::QueueEntryId.eq(queue_entry_id))
            .order_by_desc(conflicts::Column::CreatedAt)
            .all(&self.conn)
            .await?;

        Ok(rows.into_iter().map(map_conflict_model).collect())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a `sync_conflicts` entity row into a [`SyncConflict`].
///
/// The entity's declared types (not a backend-concrete `Row`) are what collapse
/// the two former per-backend row types into one path — see ADR-036.
/// `local_state`/`remote_state` stay TEXT holding application-serialized JSON,
/// so an unparseable value still degrades to `Value::Null` rather than failing
/// the whole query.
fn map_conflict_model(row: conflicts::Model) -> SyncConflict {
    let local_state = serde_json::from_str(&row.local_state).unwrap_or(serde_json::Value::Null);
    let remote_state = serde_json::from_str(&row.remote_state).unwrap_or(serde_json::Value::Null);
    let resolution = row
        .resolution
        .as_deref()
        .and_then(|s| s.parse::<Resolution>().ok());

    SyncConflict {
        id: row.id,
        queue_entry_id: row.queue_entry_id,
        local_state,
        remote_state,
        resolution,
        resolved_at: row.resolved_at,
        // `created_at` is nullable and the pre-port SQLite decoder substituted
        // "now" for a value it could not parse; keep that fallback.
        created_at: row.created_at.unwrap_or_else(Utc::now),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::offline_queue::OperationType;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"CREATE TABLE sync_queue (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                operation_type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                payload TEXT,
                status TEXT DEFAULT 'pending',
                retry_count INTEGER DEFAULT 0,
                max_retries INTEGER DEFAULT 3,
                created_at DATETIME DEFAULT (datetime('now')),
                processed_at DATETIME,
                error TEXT
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE sync_conflicts (
                id TEXT PRIMARY KEY,
                queue_entry_id TEXT NOT NULL,
                local_state TEXT NOT NULL,
                remote_state TEXT NOT NULL,
                resolution TEXT,
                resolved_at DATETIME,
                created_at DATETIME DEFAULT (datetime('now'))
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn make_operation() -> QueuedOperation {
        QueuedOperation::new(
            "acct-1".to_string(),
            OperationType::Archive,
            "msg-1".to_string(),
            None,
        )
    }

    #[tokio::test]
    async fn test_detect_conflict_with_remote_deleted() {
        let pool = test_pool().await;
        let resolver =
            ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LastWriterWins);
        let op = make_operation();

        let remote = serde_json::json!({ "deleted": true });
        let conflict = resolver.detect_conflict(&op, &remote);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().queue_entry_id, op.id);
    }

    #[tokio::test]
    async fn test_detect_conflict_with_remote_modified() {
        let pool = test_pool().await;
        let resolver =
            ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LastWriterWins);
        let op = make_operation();

        // Remote was modified after the operation was queued.
        let future_ts = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let remote = serde_json::json!({ "modified_at": future_ts });
        let conflict = resolver.detect_conflict(&op, &remote);
        assert!(conflict.is_some());
    }

    #[tokio::test]
    async fn test_no_conflict_when_unchanged() {
        let pool = test_pool().await;
        let resolver =
            ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LastWriterWins);
        let op = make_operation();

        let remote = serde_json::json!({ "status": "active" });
        let conflict = resolver.detect_conflict(&op, &remote);
        assert!(conflict.is_none());
    }

    #[tokio::test]
    async fn test_log_and_retrieve_conflict() {
        let pool = test_pool().await;
        let resolver = ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::Manual);
        let op = make_operation();

        let remote = serde_json::json!({ "deleted": true });
        let conflict = resolver.detect_conflict(&op, &remote).unwrap();

        resolver.log_conflict(&conflict).await.unwrap();

        let unresolved = resolver.unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].id, conflict.id);
        assert!(unresolved[0].resolution.is_none());
    }

    #[tokio::test]
    async fn test_conflicts_scope_to_queue_entry() {
        let pool = test_pool().await;
        let resolver = ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LocalWins);

        let op_a = QueuedOperation::new(
            "acct-a".to_string(),
            OperationType::Archive,
            "msg-a".to_string(),
            None,
        );
        let op_b = QueuedOperation::new(
            "acct-b".to_string(),
            OperationType::Archive,
            "msg-b".to_string(),
            None,
        );

        let remote = serde_json::json!({ "deleted": true });
        let conflict_a = resolver.detect_conflict(&op_a, &remote).unwrap();
        let conflict_b = resolver.detect_conflict(&op_b, &remote).unwrap();
        resolver.log_conflict(&conflict_a).await.unwrap();
        resolver.log_conflict(&conflict_b).await.unwrap();

        // A read returns only the requested entry's conflicts.
        let for_a = resolver.conflicts_for_entry(&op_a.id).await.unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].id, conflict_a.id);
        assert_eq!(for_a[0].queue_entry_id, op_a.id);

        // ...and resolving one account's conflict leaves the other's untouched.
        resolver.resolve(&conflict_a).await.unwrap();
        let unresolved = resolver.unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].id, conflict_b.id);
    }

    #[tokio::test]
    async fn test_resolve_local_wins_strategy() {
        let pool = test_pool().await;
        let resolver = ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LocalWins);
        let op = make_operation();

        let remote = serde_json::json!({ "deleted": true });
        let conflict = resolver.detect_conflict(&op, &remote).unwrap();
        resolver.log_conflict(&conflict).await.unwrap();

        let resolution = resolver.resolve(&conflict).await.unwrap();
        assert_eq!(resolution, Resolution::LocalWins);

        // Should no longer appear as unresolved.
        let unresolved = resolver.unresolved().await.unwrap();
        assert!(unresolved.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_remote_wins_strategy() {
        let pool = test_pool().await;
        let resolver = ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::RemoteWins);
        let op = make_operation();

        let remote = serde_json::json!({ "deleted": true });
        let conflict = resolver.detect_conflict(&op, &remote).unwrap();
        resolver.log_conflict(&conflict).await.unwrap();

        let resolution = resolver.resolve(&conflict).await.unwrap();
        assert_eq!(resolution, Resolution::RemoteWins);
    }

    #[tokio::test]
    async fn test_resolve_manual_stays_unresolved() {
        let pool = test_pool().await;
        let resolver = ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::Manual);
        let op = make_operation();

        let remote = serde_json::json!({ "deleted": true });
        let conflict = resolver.detect_conflict(&op, &remote).unwrap();
        resolver.log_conflict(&conflict).await.unwrap();

        let resolution = resolver.resolve(&conflict).await.unwrap();
        assert_eq!(resolution, Resolution::ManualPending);

        // Should still be unresolved (resolution column is NULL for manual).
        let unresolved = resolver.unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 1);
    }

    #[tokio::test]
    async fn test_last_writer_wins_remote_newer() {
        let pool = test_pool().await;
        let resolver =
            ConflictResolver::new(Database::Sqlite(pool), ConflictStrategy::LastWriterWins);
        let op = make_operation();

        let future_ts = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let remote = serde_json::json!({ "modified_at": future_ts });
        let conflict = resolver.detect_conflict(&op, &remote).unwrap();
        resolver.log_conflict(&conflict).await.unwrap();

        let resolution = resolver.resolve(&conflict).await.unwrap();
        assert_eq!(resolution, Resolution::RemoteWins);
    }

    #[test]
    fn test_conflict_strategy_default() {
        assert_eq!(
            ConflictStrategy::default(),
            ConflictStrategy::LastWriterWins
        );
    }

    #[test]
    fn test_resolution_roundtrip() {
        assert_eq!(
            "local_wins".parse::<Resolution>().unwrap(),
            Resolution::LocalWins
        );
        assert_eq!(Resolution::Merged.as_str(), "merged");
        assert!("bogus".parse::<Resolution>().is_err());
    }

    #[test]
    fn test_sync_conflict_serialization() {
        let conflict = SyncConflict {
            id: "c-1".to_string(),
            queue_entry_id: "q-1".to_string(),
            local_state: serde_json::json!({"op": "archive"}),
            remote_state: serde_json::json!({"deleted": true}),
            resolution: Some(Resolution::LocalWins),
            resolved_at: Some(Utc::now()),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&conflict).unwrap();
        let deserialized: SyncConflict = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "c-1");
        assert_eq!(deserialized.resolution, Some(Resolution::LocalWins));
    }
}
