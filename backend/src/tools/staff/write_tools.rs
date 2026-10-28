//! Milestone 3: the first write-capable chief-of-staff tools.
//!
//! Scope note #1 (read before extending): `cleanup::domain::operation`'s
//! `reverse_op` is computed at expansion time (`orchestrator::expander`)
//! but is never executed anywhere in the existing codebase -- there is no
//! working undo for the general rule/cluster `CleanupPlan` system today.
//! Rather than wrap that gap, era-archive and closeout here are
//! self-contained, with their own journal so they can be undone.
//!
//! Scope note #2 (resolved — follow-up pass): `cleanup::orchestrator`
//! itself cannot appear in this lib crate's types (`main.rs` compiles its
//! own, separate copy of `cleanup` via its own `mod cleanup;`, distinct
//! from this crate's `pub mod cleanup { domain; repository; }` — see
//! `crate::tools::mail_writer`'s module doc for the full reasoning). So
//! apply/undo reach the real mailbox through `ctx.mail_writer()`, a trait
//! the binary implements by wrapping its own `ApplyOrchestrator` and
//! resolving a live provider through its existing `provider_factory` — the
//! same hardened, already-wired connection path `AccountWorker` uses for
//! every other apply. No provider or connection logic is reimplemented
//! here. `emails.is_archived` is still updated locally, but only *after* a
//! successful real-provider call, mirroring
//! `account_worker::mark_archived_locally`'s own ordering.
//!
//! Partial-failure semantics: items within one apply/undo batch are
//! processed in order; if a real-provider call fails partway through, the
//! items processed before it have already been written (both provider and
//! local state) and are NOT rolled back — the error reports how far it got.
//! A caller can safely retry: the un-journaled failed batch simply has no
//! undo entry, and re-running preview/apply will naturally exclude whatever
//! already succeeded (since `scan_mail_with_ids`/`closeout_candidates`
//! filter on current state, e.g. `is_archived = false`).
//!
//! Authority level: only 0 (Observe, via the `*_preview` tools) and 1
//! (Suggest) are implemented. Every apply requires the exact batch key a
//! prior preview returned; a bare "do it" call is rejected. Levels 2/3
//! (trusted-rule auto-apply, send-on-behalf) are milestone 4+ and not
//! planned at all, respectively -- nothing here auto-applies anything.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Statement};
use serde::{Deserialize, Serialize};

use crate::cleanup::domain::kind::Kind;
use crate::db::entities::emails;
use crate::tools::staff::scan::{scan_mail_with_ids, Candidate};
use crate::tools::{ToolContext, ToolError};

/// A journal row's reversible item.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalItem {
    email_id: String,
    provider_message_id: String,
}

/// Set `emails.is_archived` for one row — the local-state half of a real
/// archive, mirroring `account_worker::mark_archived_locally`'s own
/// ordering (called only after the real provider call below succeeds).
async fn set_archived_locally(
    ctx: &Arc<ToolContext>,
    email_id: &str,
    archived: bool,
) -> Result<(), ToolError> {
    emails::Entity::update_many()
        .col_expr(
            emails::Column::IsArchived,
            sea_orm::sea_query::Expr::value(archived),
        )
        .filter(emails::Column::Id.eq(email_id))
        .exec(&ctx.conn())
        .await
        .map_err(|e| ToolError::Database(format!("updating is_archived for {email_id}: {e}")))?;
    Ok(())
}

/// Archive one message on the account's real mailbox, then mirror the
/// change locally. See this module's scope note #2: `ctx.mail_writer()` is
/// how a lib-crate tool reaches the binary's live provider connection.
async fn archive_for_real(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    candidate: &Candidate,
) -> Result<(), ToolError> {
    ctx.mail_writer()?
        .archive(account_id, &candidate.provider_message_id)
        .await
        .map_err(|e| {
            ToolError::Database(format!(
                "archiving {} on the real mailbox: {e}",
                candidate.provider_message_id
            ))
        })?;
    set_archived_locally(ctx, &candidate.email_id, true).await
}

/// Reverse of [`archive_for_real`]: unarchive on the real mailbox first,
/// then mirror locally.
async fn unarchive_for_real(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    item: &JournalItem,
) -> Result<(), ToolError> {
    ctx.mail_writer()?
        .unarchive(account_id, &item.provider_message_id)
        .await
        .map_err(|e| {
            ToolError::Database(format!(
                "unarchiving {} on the real mailbox: {e}",
                item.provider_message_id
            ))
        })?;
    set_archived_locally(ctx, &item.email_id, false).await
}

/// Hash a sorted candidate-id list into a confirmation key, the same role a
/// `plan_hash` plays for the main cleanup system: the caller must present
/// this exact value to apply, so an apply can never run against a
/// different (e.g. since-changed) candidate set than the one it saw.
fn batch_key(account_id: &str, email_ids: &[String]) -> String {
    let mut sorted = email_ids.to_vec();
    sorted.sort();
    let mut hasher = blake3::Hasher::new();
    hasher.update(account_id.as_bytes());
    for id in &sorted {
        hasher.update(id.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn require_batch_key(expected: &str, submitted: &str) -> Result<(), ToolError> {
    if expected != submitted {
        return Err(ToolError::Invalid(
            "batch key does not match the current preview; re-run the preview tool and confirm \
             with its exact key (never guess or reuse a stale one)"
                .to_string(),
        ));
    }
    Ok(())
}

async fn insert_journal(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    kind: &str,
    key: &str,
    items: &[JournalItem],
) -> Result<(), ToolError> {
    let conn = ctx.conn();
    let backend = conn.get_database_backend();
    let items_json = serde_json::to_string(items)
        .map_err(|e| ToolError::Database(format!("serializing journal items: {e}")))?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute_raw(Statement::from_sql_and_values(
        backend,
        "INSERT INTO operation_journal (id, account_id, kind, batch_key, items_json, applied_at, undone_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NULL)",
        [
            id.into(),
            account_id.into(),
            kind.into(),
            key.into(),
            items_json.into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| ToolError::Database(format!("writing operation_journal: {e}")))?;
    Ok(())
}

/// Load the still-undone journal entry for a batch key, erroring if it was
/// already undone or never applied -- undo is one-shot, not idempotent,
/// because re-running unarchive_message against a message the user has
/// since deliberately re-archived would be a real, surprising mutation.
async fn load_undoable_journal(
    ctx: &Arc<ToolContext>,
    key: &str,
) -> Result<(String, Vec<JournalItem>), ToolError> {
    let conn = ctx.conn();
    let backend = conn.get_database_backend();
    let row = conn
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            "SELECT id, items_json FROM operation_journal WHERE batch_key = $1 AND undone_at IS NULL",
            [key.into()],
        ))
        .await
        .map_err(|e| ToolError::Database(format!("reading operation_journal: {e}")))?
        .ok_or_else(|| {
            ToolError::Invalid(
                "no undoable entry for this batch key (never applied, or already undone)"
                    .to_string(),
            )
        })?;
    let journal_id: String = row
        .try_get("", "id")
        .map_err(|e| ToolError::Database(e.to_string()))?;
    let items_json: String = row
        .try_get("", "items_json")
        .map_err(|e| ToolError::Database(e.to_string()))?;
    let items: Vec<JournalItem> = serde_json::from_str(&items_json)
        .map_err(|e| ToolError::Database(format!("parsing journal items: {e}")))?;
    Ok((journal_id, items))
}

async fn mark_undone(ctx: &Arc<ToolContext>, journal_id: &str) -> Result<(), ToolError> {
    let conn = ctx.conn();
    let backend = conn.get_database_backend();
    conn.execute_raw(Statement::from_sql_and_values(
        backend,
        "UPDATE operation_journal SET undone_at = $1 WHERE id = $2",
        [Utc::now().to_rfc3339().into(), journal_id.into()],
    ))
    .await
    .map_err(|e| ToolError::Database(format!("updating operation_journal: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Era archive (docs/plan/mail-agent-2026-10.md section 8: ">5 years" era --
// Kind-only, archive, mark read; topic judgment and the 2-5y/last-2y/
// last-30d eras are NOT implemented here, see module doc).
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct EraArchivePreviewResult {
    pub account_id: String,
    pub candidate_count: usize,
    pub sample: Vec<Candidate>,
    pub batch_key: String,
}

const FIVE_YEAR_DAYS: i64 = 1825;

pub async fn era_archive_preview(
    ctx: &Arc<ToolContext>,
    account_id: &str,
) -> Result<EraArchivePreviewResult, ToolError> {
    let cutoff = Utc::now() - chrono::Duration::days(FIVE_YEAR_DAYS);
    let candidates = scan_mail_with_ids(ctx, account_id, cutoff).await?;
    let ids: Vec<String> = candidates.iter().map(|c| c.email_id.clone()).collect();
    let key = batch_key(account_id, &ids);
    Ok(EraArchivePreviewResult {
        account_id: account_id.to_string(),
        candidate_count: candidates.len(),
        sample: candidates.into_iter().take(3).collect(),
        batch_key: key,
    })
}

#[derive(Debug, Serialize)]
pub struct EraArchiveApplyResult {
    pub archived_count: usize,
    pub batch_key: String,
}

pub async fn era_archive_apply(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    submitted_batch_key: &str,
) -> Result<EraArchiveApplyResult, ToolError> {
    let cutoff = Utc::now() - chrono::Duration::days(FIVE_YEAR_DAYS);
    let candidates = scan_mail_with_ids(ctx, account_id, cutoff).await?;
    let ids: Vec<String> = candidates.iter().map(|c| c.email_id.clone()).collect();
    let expected = batch_key(account_id, &ids);
    require_batch_key(&expected, submitted_batch_key)?;

    if candidates.is_empty() {
        // Re-apply-is-a-no-op: nothing left to do, nothing journaled.
        return Ok(EraArchiveApplyResult {
            archived_count: 0,
            batch_key: expected,
        });
    }

    let mut items = Vec::with_capacity(candidates.len());
    for c in &candidates {
        archive_for_real(ctx, account_id, c).await?;
        items.push(JournalItem {
            email_id: c.email_id.clone(),
            provider_message_id: c.provider_message_id.clone(),
        });
    }

    insert_journal(ctx, account_id, "era_archive", &expected, &items).await?;

    Ok(EraArchiveApplyResult {
        archived_count: items.len(),
        batch_key: expected,
    })
}

#[derive(Debug, Serialize)]
pub struct EraArchiveUndoResult {
    pub restored_count: usize,
}

pub async fn era_archive_undo(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    batch_key: &str,
) -> Result<EraArchiveUndoResult, ToolError> {
    let (journal_id, items) = load_undoable_journal(ctx, batch_key).await?;

    for item in &items {
        unarchive_for_real(ctx, account_id, item).await?;
    }

    mark_undone(ctx, &journal_id).await?;
    Ok(EraArchiveUndoResult {
        restored_count: items.len(),
    })
}

// ---------------------------------------------------------------------------
// Close-out (docs/plan/mail-agent-2026-10.md section 8). Approximated to
// three dispositions for this milestone -- see module doc: the full six
// (Needs you / Done / File / New sender / Seen alert / Keep) need the
// sender ledger (first/last seen, protected list) that later milestones
// build. People -> "needs you" (no-op, stays). Already-read, non-People ->
// "done" (archived). Everything else -> "keep" (no-op, default-safe).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    NeedsYou,
    Done,
    Keep,
}

fn disposition_for(kind: Kind, is_read: bool) -> Disposition {
    match kind {
        Kind::People => Disposition::NeedsYou,
        _ if is_read => Disposition::Done,
        _ => Disposition::Keep,
    }
}

#[derive(Debug, Serialize)]
pub struct CloseoutPreviewItem {
    pub email_id: String,
    pub from_addr: String,
    pub disposition: Disposition,
}

#[derive(Debug, Serialize)]
pub struct CloseoutPreviewResult {
    pub account_id: String,
    pub items: Vec<CloseoutPreviewItem>,
    pub to_archive_count: usize,
    pub batch_key: String,
}

async fn closeout_candidates(
    ctx: &Arc<ToolContext>,
    account_id: &str,
) -> Result<Vec<(Candidate, Disposition)>, ToolError> {
    let inbox = crate::tools::staff::scan::scan_mail(ctx, Some(account_id), true).await?;
    let with_ids = scan_mail_with_ids(ctx, account_id, DateTime::<Utc>::MIN_UTC).await?;
    // scan_mail (milestone 2) doesn't carry email_id; scan_mail_with_ids
    // (this milestone) does but reuses the same classify() call rather
    // than milestone 2's ScannedMessage -- join on (from_addr, received_at)
    // since both queries read the same inbox-filtered or whole-mailbox
    // row set deterministically for a given account at a given instant.
    let mut out = Vec::new();
    for scanned in inbox {
        if let Some(c) = with_ids.iter().find(|c| {
            c.from_addr == scanned.from_addr && c.received_at.naive_utc() == scanned.received_at
        }) {
            let disposition = disposition_for(scanned.classification.kind, scanned.is_read);
            out.push((c.clone(), disposition));
        }
    }
    Ok(out)
}

pub async fn closeout_preview(
    ctx: &Arc<ToolContext>,
    account_id: &str,
) -> Result<CloseoutPreviewResult, ToolError> {
    let candidates = closeout_candidates(ctx, account_id).await?;
    let to_archive: Vec<String> = candidates
        .iter()
        .filter(|(_, d)| *d == Disposition::Done)
        .map(|(c, _)| c.email_id.clone())
        .collect();
    let key = batch_key(account_id, &to_archive);
    let to_archive_count = to_archive.len();
    let items = candidates
        .into_iter()
        .map(|(c, disposition)| CloseoutPreviewItem {
            email_id: c.email_id,
            from_addr: c.from_addr,
            disposition,
        })
        .collect();
    Ok(CloseoutPreviewResult {
        account_id: account_id.to_string(),
        items,
        to_archive_count,
        batch_key: key,
    })
}

#[derive(Debug, Serialize)]
pub struct CloseoutApplyResult {
    pub archived_count: usize,
    pub batch_key: String,
}

pub async fn closeout_apply(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    submitted_batch_key: &str,
) -> Result<CloseoutApplyResult, ToolError> {
    let candidates = closeout_candidates(ctx, account_id).await?;
    let to_archive: Vec<Candidate> = candidates
        .into_iter()
        .filter(|(_, d)| *d == Disposition::Done)
        .map(|(c, _)| c)
        .collect();
    let ids: Vec<String> = to_archive.iter().map(|c| c.email_id.clone()).collect();
    let expected = batch_key(account_id, &ids);
    require_batch_key(&expected, submitted_batch_key)?;

    if to_archive.is_empty() {
        return Ok(CloseoutApplyResult {
            archived_count: 0,
            batch_key: expected,
        });
    }

    let mut items = Vec::with_capacity(to_archive.len());
    for c in &to_archive {
        archive_for_real(ctx, account_id, c).await?;
        items.push(JournalItem {
            email_id: c.email_id.clone(),
            provider_message_id: c.provider_message_id.clone(),
        });
    }

    insert_journal(ctx, account_id, "closeout", &expected, &items).await?;

    Ok(CloseoutApplyResult {
        archived_count: items.len(),
        batch_key: expected,
    })
}

#[derive(Debug, Serialize)]
pub struct CloseoutUndoResult {
    pub restored_count: usize,
}

pub async fn closeout_undo(
    ctx: &Arc<ToolContext>,
    account_id: &str,
    batch_key: &str,
) -> Result<CloseoutUndoResult, ToolError> {
    let (journal_id, items) = load_undoable_journal(ctx, batch_key).await?;

    for item in &items {
        unarchive_for_real(ctx, account_id, item).await?;
    }

    mark_undone(ctx, &journal_id).await?;
    Ok(CloseoutUndoResult {
        restored_count: items.len(),
    })
}

// ---------------------------------------------------------------------------
// MCP-shaped wrappers (ctx, Request) -> Result<Value, ToolError>, matching
// every other staff tool's registration shape (see staff::doctor::doctor).
// ---------------------------------------------------------------------------

use crate::tools::readonly::validate_uuid;
use crate::tools::staff::params::{
    CloseoutApplyRequest, CloseoutPreviewRequest, CloseoutUndoRequest, EraArchiveApplyRequest,
    EraArchivePreviewRequest, EraArchiveUndoRequest,
};

pub async fn era_archive_preview_tool(
    ctx: Arc<ToolContext>,
    req: EraArchivePreviewRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = era_archive_preview(&ctx, &req.account_id).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

pub async fn era_archive_apply_tool(
    ctx: Arc<ToolContext>,
    req: EraArchiveApplyRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = era_archive_apply(&ctx, &req.account_id, &req.batch_key).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

pub async fn era_archive_undo_tool(
    ctx: Arc<ToolContext>,
    req: EraArchiveUndoRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = era_archive_undo(&ctx, &req.account_id, &req.batch_key).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

pub async fn closeout_preview_tool(
    ctx: Arc<ToolContext>,
    req: CloseoutPreviewRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = closeout_preview(&ctx, &req.account_id).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

pub async fn closeout_apply_tool(
    ctx: Arc<ToolContext>,
    req: CloseoutApplyRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = closeout_apply(&ctx, &req.account_id, &req.batch_key).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

pub async fn closeout_undo_tool(
    ctx: Arc<ToolContext>,
    req: CloseoutUndoRequest,
) -> Result<serde_json::Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;
    let result = closeout_undo(&ctx, &req.account_id, &req.batch_key).await?;
    serde_json::to_value(result).map_err(|e| ToolError::Database(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_key_is_order_independent_and_account_scoped() {
        let a = batch_key("acct1", &["b".into(), "a".into()]);
        let b = batch_key("acct1", &["a".into(), "b".into()]);
        assert_eq!(a, b, "sorting makes key order-independent");
        let c = batch_key("acct2", &["a".into(), "b".into()]);
        assert_ne!(a, c, "account id must be part of the key");
    }

    #[test]
    fn require_batch_key_rejects_a_mismatch() {
        assert!(require_batch_key("x", "y").is_err());
        assert!(require_batch_key("x", "x").is_ok());
    }

    #[test]
    fn disposition_people_is_needs_you_regardless_of_read_state() {
        assert_eq!(disposition_for(Kind::People, true), Disposition::NeedsYou);
        assert_eq!(disposition_for(Kind::People, false), Disposition::NeedsYou);
    }

    #[test]
    fn disposition_read_bulk_mail_is_done() {
        assert_eq!(
            disposition_for(Kind::Notifications, true),
            Disposition::Done
        );
    }

    #[test]
    fn disposition_unread_bulk_mail_defaults_to_keep() {
        assert_eq!(disposition_for(Kind::Other, false), Disposition::Keep);
    }
}
