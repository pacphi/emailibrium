//! Shared execution context for tool handlers.
//!
//! Tool handlers run from three callers — the MCP server, the in-process chat
//! orchestrator, and integration tests. `AppState` cannot serve that role: it
//! lives in the binary crate, so anything depending on it is unreachable from
//! `backend/tests/`, which links the library crate.
//!
//! Every service except the database is optional, so a test can build a usable
//! context from a pool alone. A tool whose backing service is absent reports
//! `NotConfigured` rather than pretending to work.

use std::sync::Arc;

use crate::db::Database;
use crate::email::oauth::OAuthManager;
use crate::email::poll_scheduler::PollSchedulerHandle;
use crate::sync_lock::AccountLockMap;
use crate::vectors::ingestion::IngestionBroadcast;
use crate::vectors::VectorService;

use super::registry::ToolError;

/// Services available to every tool handler.
///
/// Extended by adding a field here plus the matching line in the binary's
/// conversion from `AppState` — the only two places that change when a tool
/// needs a new backing service.
#[derive(Clone)]
pub struct ToolContext {
    /// Always present: every tool reads the database.
    pub db: Arc<Database>,
    /// Search, store, clustering, ingestion and learning engines.
    pub vectors: Option<Arc<VectorService>>,
    /// Connected-account metadata and per-account sync state.
    pub oauth: Option<Arc<OAuthManager>>,
    /// Background poll scheduler, when periodic sync is enabled.
    pub poll_scheduler: Option<PollSchedulerHandle>,
    /// Sync-phase progress broadcast. Covers the window before the ingestion
    /// pipeline creates a job, which the pipeline's own state cannot report.
    /// Already `Clone`, so it needs no `Arc`.
    pub sync_progress: Option<IngestionBroadcast>,
    /// Per-account pipeline locks, for reporting which account is mid-run and
    /// what started it. Already `Clone` (it wraps an `Arc` internally).
    pub pipeline_locks: Option<AccountLockMap>,
}

impl ToolContext {
    /// The context the binary runs on, with every always-available service
    /// required rather than optional.
    ///
    /// The builders below are convenient and, for production wiring, dangerous:
    /// omitting one leaves its field `None`, which is not a compile error and
    /// not a test failure — the tool just reports "not configured" forever.
    /// That is exactly how `sync_progress` and `pipeline_locks` shipped unwired.
    /// Taking them as positional arguments makes the omission unrepresentable,
    /// and makes a newly added always-wired field break every caller until it is
    /// consciously handled.
    ///
    /// `poll_scheduler` stays optional because it is genuinely absent when
    /// periodic sync is disabled.
    pub fn wired(
        db: Arc<Database>,
        vectors: Arc<VectorService>,
        oauth: Arc<OAuthManager>,
        sync_progress: IngestionBroadcast,
        pipeline_locks: AccountLockMap,
        poll_scheduler: Option<PollSchedulerHandle>,
    ) -> Self {
        Self {
            db,
            vectors: Some(vectors),
            oauth: Some(oauth),
            poll_scheduler,
            sync_progress: Some(sync_progress),
            pipeline_locks: Some(pipeline_locks),
        }
    }

    /// Minimal context: database only. Tools needing other services report
    /// `NotConfigured`, keeping test setup cheap without letting a tool lie.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            vectors: None,
            oauth: None,
            poll_scheduler: None,
            sync_progress: None,
            pipeline_locks: None,
        }
    }

    pub fn with_vectors(mut self, vectors: Arc<VectorService>) -> Self {
        self.vectors = Some(vectors);
        self
    }

    pub fn with_oauth(mut self, oauth: Arc<OAuthManager>) -> Self {
        self.oauth = Some(oauth);
        self
    }

    pub fn with_poll_scheduler(mut self, handle: PollSchedulerHandle) -> Self {
        self.poll_scheduler = Some(handle);
        self
    }

    pub fn with_sync_progress(mut self, broadcast: IngestionBroadcast) -> Self {
        self.sync_progress = Some(broadcast);
        self
    }

    pub fn with_pipeline_locks(mut self, locks: AccountLockMap) -> Self {
        self.pipeline_locks = Some(locks);
        self
    }

    /// Most handlers only need the pool.
    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.db.pool()
    }

    /// Vector services, or `NotConfigured` naming what is missing.
    pub fn vectors(&self) -> Result<&Arc<VectorService>, ToolError> {
        self.vectors
            .as_ref()
            .ok_or_else(|| ToolError::NotConfigured("Vector services are not configured".into()))
    }

    /// Account manager, or `NotConfigured` naming what is missing.
    pub fn oauth(&self) -> Result<&Arc<OAuthManager>, ToolError> {
        self.oauth
            .as_ref()
            .ok_or_else(|| ToolError::NotConfigured("Account manager is not configured".into()))
    }
}
