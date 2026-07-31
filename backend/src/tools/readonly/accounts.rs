//! Account and sync-state tools.
//!
//! Both handlers project onto an explicit allow-list of fields. `ConnectedAccount`
//! carries no credentials (OAuth tokens live in a separate table), and the
//! `history_id` / `next_page_token` sync cursors are deliberately dropped: they
//! are opaque provider state with no value to a model.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::{GetSyncStatusRequest, ListAccountsRequest};
use super::validate_uuid;
use crate::email::AccountStatus;
use crate::tools::{ToolContext, ToolError};
use crate::vectors::ingestion::SyncPhase;

/// List connected email accounts with their sync counters and settings.
///
/// No status filter is applied — disconnected and errored accounts appear too,
/// so `status` and `is_active` are surfaced on every entry rather than left for
/// the caller to infer from presence in the list.
pub async fn list_accounts(
    ctx: Arc<ToolContext>,
    _req: ListAccountsRequest,
) -> Result<Value, ToolError> {
    let oauth = ctx.oauth()?;

    let accounts = oauth
        .list_accounts()
        .await
        .map_err(|e| super::db_error("Listing accounts", e))?;

    let mut items = Vec::with_capacity(accounts.len());
    for account in accounts {
        let is_active = account.status == AccountStatus::Connected;
        let sync = oauth.get_sync_state(&account.id).await.ok().flatten();

        items.push(json!({
            "id": account.id,
            "provider": account.provider.as_str(),
            "email_address": account.email_address,
            "status": account.status.as_str(),
            "is_active": is_active,
            "emails_synced": sync.as_ref().map(|s| s.emails_synced).unwrap_or(0),
            "last_sync_at": sync
                .as_ref()
                .and_then(|s| s.last_sync_at.map(|dt| dt.to_rfc3339())),
            "sync_failures": sync.as_ref().map(|s| s.sync_failures).unwrap_or(0),
            "archive_strategy": account.archive_strategy,
            "label_prefix": account.label_prefix,
            "sync_depth": account.sync_depth,
            "sync_frequency": account.sync_frequency,
            "connected_at": account.created_at.to_rfc3339(),
        }));
    }

    Ok(json!({ "count": items.len(), "accounts": items }))
}

/// Report ingestion pipeline progress, poll scheduler state, and per-account sync state.
///
/// Two independent things are reported and must not be conflated: `ingestion`
/// describes the local embedding/categorizing pipeline, while each entry in
/// `accounts` describes that account's provider-side sync. An account can be
/// mid-sync while the pipeline reports idle, because the pipeline only creates
/// a job once messages have been fetched.
pub async fn get_sync_status(
    ctx: Arc<ToolContext>,
    req: GetSyncStatusRequest,
) -> Result<Value, ToolError> {
    if let Some(ref account_id) = req.account_id {
        validate_uuid("account_id", account_id).map_err(ToolError::Invalid)?;
    }

    let ingestion = current_ingestion(&ctx).await?;
    let oauth = ctx.oauth()?;

    // Per-account sync state: one account when scoped, otherwise all of them.
    let account_ids = match &req.account_id {
        Some(id) => vec![id.clone()],
        None => oauth
            .list_accounts()
            .await
            .map_err(|e| super::db_error("Listing accounts", e))?
            .into_iter()
            .map(|a| a.id)
            .collect(),
    };

    let mut accounts = Vec::with_capacity(account_ids.len());
    for id in account_ids {
        match oauth.get_sync_state(&id).await {
            Ok(Some(sync)) => accounts.push(json!({
                "account_id": sync.account_id,
                "status": sync.status,
                "last_sync_at": sync.last_sync_at.map(|dt| dt.to_rfc3339()),
                "emails_synced": sync.emails_synced,
                "sync_failures": sync.sync_failures,
                "last_error": sync.last_error,
            })),
            // An account with no sync_state row has simply never synced.
            Ok(None) => accounts.push(json!({
                "account_id": id,
                "status": "never_synced",
                "last_sync_at": null,
                "emails_synced": 0,
                "sync_failures": 0,
                "last_error": null,
            })),
            Err(e) => return Err(super::db_error("Reading account sync state", e)),
        }
    }

    // An absent scheduler means periodic sync is disabled for this deployment,
    // which is different from "enabled but idle".
    let poll = match ctx.poll_scheduler.as_ref() {
        Some(handle) => {
            let status = handle.status().await;
            json!({
                "configured": true,
                "enabled": status.enabled,
                "total_polls": status.total_polls,
                "total_errors": status.total_errors,
                "accounts": status
                    .accounts
                    .iter()
                    .map(|a| json!({
                        "account_id": a.account_id,
                        "seconds_since_last_poll": a.seconds_since_last_poll,
                        "backoff_secs": a.backoff_secs,
                        "in_progress": a.in_progress,
                        "consecutive_failures": a.consecutive_failures,
                    }))
                    .collect::<Vec<_>>(),
            })
        }
        None => json!({ "configured": false }),
    };

    // The pipeline lock is per-account, so it is only meaningful when scoped.
    // `null` means "not asked", which is distinct from `{"held": false}`.
    let lock = match (&req.account_id, ctx.pipeline_locks.as_ref()) {
        (Some(id), Some(locks)) => match locks.get_activity(id).await {
            Some(activity) => json!({
                "held": true,
                "job_id": activity.job_id,
                "phase": activity.phase,
                "source": activity.source,
                "started_at": activity.started_at.to_rfc3339(),
            }),
            None => json!({ "held": false }),
        },
        _ => Value::Null,
    };

    Ok(json!({
        "ingestion": ingestion,
        "accounts": accounts,
        "poll": poll,
        "lock": lock,
    }))
}

/// Snapshot the current ingestion job.
///
/// Two sources, in order. The pipeline's own job state covers embedding,
/// categorizing, and clustering; `current_job` is never reset once set, so an
/// idle pipeline is distinguished by phase rather than by absence. The sync
/// broadcast then covers the *syncing* phase, which runs before the pipeline
/// creates a job at all — without it this reports "not active" throughout an
/// initial sync, which is a wrong answer rather than a missing one.
async fn current_ingestion(ctx: &ToolContext) -> Result<Value, ToolError> {
    let vectors = ctx.vectors()?;

    if let Some(p) = vectors.ingestion_pipeline.get_progress().await {
        if p.phase != "complete" {
            return Ok(json!({
                "active": true,
                "job_id": p.job_id,
                "phase": p.phase,
                "total": p.total,
                "processed": p.processed,
                "embedded": p.embedded,
                "categorized": p.categorized,
                "failed": p.failed,
                "eta_seconds": p.eta_seconds,
                "emails_per_second": p.emails_per_second,
            }));
        }
    }

    if let Some(broadcast) = ctx.sync_progress.as_ref() {
        if let Some(bp) = broadcast.last_progress().await {
            if bp.phase != SyncPhase::Complete {
                return Ok(json!({
                    "active": true,
                    "job_id": bp.job_id,
                    "phase": bp.phase.to_string(),
                    "total": bp.total,
                    "processed": bp.processed,
                    "embedded": bp.embedded,
                    "categorized": bp.categorized,
                    "failed": bp.failed,
                    "eta_seconds": bp.eta_seconds,
                    "emails_per_second": bp.emails_per_second,
                }));
            }
        }
    }

    Ok(json!({ "active": false, "phase": null }))
}
