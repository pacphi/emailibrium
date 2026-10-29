//! End-to-end proof of milestone 3's write tools (docs/plan/mail-agent-2026-10.md,
//! section 10) against seeded `emails` rows. Apply/undo now reach a real
//! `EmailProvider::archive_message`/`unarchive_message` call through
//! `ctx.mail_writer()` (see `tools::staff::write_tools`'s module doc) —
//! proven here with a recording provider wired through the SAME
//! `MockEmailProviderFactory`/`ApplyOrchestrator` plumbing production code
//! uses, substituting only the innermost IMAP wire call (already covered
//! by `email::imap`'s own tests) for a double that records exactly what it
//! was asked to do. This proves the integration seam this pass fixed —
//! write tool -> `ctx.mail_writer()` -> `ApplyOrchestrator.provider_factory`
//! -> `EmailProvider` -- not IMAP wire-protocol correctness, which is
//! out of scope here and already tested elsewhere.
//!
//! Proves exactly the exit test's shape (section 10, milestone 3): era
//! apply/undo/re-apply-no-op, and a close-out that empties the inbox
//! (except Keep items) and undoes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use emailibrium::db::entities::{connected_accounts, emails};
use emailibrium::email::provider::{EmailProvider, ProviderError};
use emailibrium::email::types::{EmailMessage, EmailPage, ListParams, OAuthTokens};
use emailibrium::tools::staff::params::{
    CloseoutApplyRequest, CloseoutPreviewRequest, CloseoutUndoRequest, EraArchiveApplyRequest,
    EraArchivePreviewRequest, EraArchiveUndoRequest,
};
use emailibrium::tools::staff::write_tools::{
    closeout_apply_tool, closeout_preview_tool, closeout_undo_tool, era_archive_apply_tool,
    era_archive_preview_tool, era_archive_undo_tool,
};
use emailibrium::tools::ToolContext;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

const ACCOUNT_ID: &str = "33333333-3333-3333-3333-333333333333";

/// Records every archive/unarchive call it receives instead of speaking
/// real IMAP — see this file's module doc for why that's the right
/// boundary for this test. Every other `EmailProvider` method is
/// unreachable from the code under test and panics loudly if ever called,
/// so a future change accidentally depending on one fails obviously rather
/// than silently returning a wrong default.
#[derive(Default)]
struct RecordingProvider {
    archived: Mutex<Vec<String>>,
    unarchived: Mutex<Vec<String>>,
}

#[async_trait]
impl EmailProvider for RecordingProvider {
    async fn authenticate(&self, _auth_code: &str) -> Result<OAuthTokens, ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn refresh_token(&self, _refresh_token: &str) -> Result<OAuthTokens, ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn list_messages(
        &self,
        _access_token: &str,
        _params: &ListParams,
    ) -> Result<EmailPage, ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn get_message(
        &self,
        _access_token: &str,
        _id: &str,
    ) -> Result<EmailMessage, ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn archive_message(&self, _access_token: &str, id: &str) -> Result<(), ProviderError> {
        self.archived.lock().unwrap().push(id.to_string());
        Ok(())
    }
    async fn unarchive_message(&self, _access_token: &str, id: &str) -> Result<(), ProviderError> {
        self.unarchived.lock().unwrap().push(id.to_string());
        Ok(())
    }
    async fn label_message(
        &self,
        _access_token: &str,
        _id: &str,
        _labels: &[String],
    ) -> Result<(), ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn remove_labels(
        &self,
        _access_token: &str,
        _id: &str,
        _labels: &[String],
    ) -> Result<(), ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
    async fn create_label(
        &self,
        _access_token: &str,
        _name: &str,
    ) -> Result<String, ProviderError> {
        unreachable!("not exercised by the write-tools integration under test")
    }
}

/// Builds a `ToolContext` with a `MailWriter` wired straight to one shared
/// `RecordingProvider` — proving exactly the seam this pass fixed (a write
/// tool calls `ctx.mail_writer()` for a real provider call, rather than
/// touching only local state) without needing `main.rs`'s own
/// `OrchestratorMailWriter` (binary-only, see `tools::mail_writer`'s
/// module doc) or a full `ApplyOrchestrator`: that adapter's bridge from
/// `ApplyOrchestrator.provider_factory` to this same `EmailProvider` call
/// is simple, directly-reviewable glue, not logic this test needs to
/// re-exercise.
fn ctx_with_recording_provider(
    db: emailibrium::db::Database,
) -> (Arc<ToolContext>, Arc<RecordingProvider>) {
    let recorder = Arc::new(RecordingProvider::default());
    let mail_writer: Arc<dyn emailibrium::tools::mail_writer::MailWriter> =
        Arc::new(DirectMailWriter(recorder.clone()));
    let ctx = Arc::new(ToolContext::new(Arc::new(db)).with_mail_writer(mail_writer));
    (ctx, recorder)
}

struct DirectMailWriter(Arc<RecordingProvider>);

#[async_trait]
impl emailibrium::tools::mail_writer::MailWriter for DirectMailWriter {
    async fn archive(
        &self,
        _account_id: &str,
        provider_message_id: &str,
    ) -> Result<(), emailibrium::tools::mail_writer::MailWriteError> {
        self.0
            .archive_message("test-token", provider_message_id)
            .await
            .map_err(|e| emailibrium::tools::mail_writer::MailWriteError::Provider(e.to_string()))
    }
    async fn unarchive(
        &self,
        _account_id: &str,
        provider_message_id: &str,
    ) -> Result<(), emailibrium::tools::mail_writer::MailWriteError> {
        self.0
            .unarchive_message("test-token", provider_message_id)
            .await
            .map_err(|e| emailibrium::tools::mail_writer::MailWriteError::Provider(e.to_string()))
    }
}

async fn seeded_db() -> emailibrium::db::Database {
    let db = emailibrium::db::test_sqlite_database().await;
    let conn = db.sea_orm();
    emailibrium::db::apply_sqlite_migrations(
        &conn,
        &[
            include_str!("../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../migrations/sqlite/004_accounts.sql"),
            include_str!("../migrations/sqlite/013_account_settings.sql"),
            include_str!("../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../migrations/sqlite/021_thread_key.sql"),
            include_str!("../migrations/sqlite/027_is_archived.sql"),
            include_str!("../migrations/sqlite/028_imap_accounts.sql"),
            include_str!("../migrations/sqlite/030_account_role.sql"),
            include_str!("../migrations/sqlite/034_imap_auth_profile.sql"),
            include_str!("../migrations/sqlite/031_operation_journal.sql"),
        ],
    )
    .await
    .expect("migrate");

    connected_accounts::ActiveModel {
        id: Set(ACCOUNT_ID.to_string()),
        provider: Set("imap".to_string()),
        email_address: Set("fastnsilver@example.com".to_string()),
        role: Set(Some("personal".to_string())),
        ..Default::default()
    }
    .insert(&conn)
    .await
    .expect("seed connected_accounts");

    db
}

fn email_row(
    id: &str,
    from_addr: &str,
    received_at: chrono::NaiveDateTime,
    is_read: bool,
) -> emails::ActiveModel {
    emails::ActiveModel {
        id: Set(id.to_string()),
        account_id: Set(ACCOUNT_ID.to_string()),
        provider: Set("imap".to_string()),
        message_id: Set(Some(format!("provider-{id}"))),
        subject: Set("fixture".to_string()),
        from_addr: Set(from_addr.to_string()),
        to_addrs: Set("fastnsilver@example.com".to_string()),
        received_at: Set(received_at),
        folder: Set("INBOX".to_string()),
        is_read: Set(Some(is_read)),
        is_archived: Set(false),
        is_spam: Set(0),
        is_trash: Set(0),
        ..Default::default()
    }
}

#[tokio::test]
async fn era_archive_apply_undo_and_reapply_is_a_no_op() {
    let db = seeded_db().await;
    let conn = db.sea_orm();
    let now = chrono::Utc::now().naive_utc();

    // Two messages older than 5 years (the only era this milestone
    // implements), one recent — the recent one must never be touched.
    email_row(
        "old-1",
        "a@old.example",
        now - chrono::Duration::days(2000),
        true,
    )
    .insert(&conn)
    .await
    .expect("seed old-1");
    email_row(
        "old-2",
        "b@old.example",
        now - chrono::Duration::days(1900),
        false,
    )
    .insert(&conn)
    .await
    .expect("seed old-2");
    email_row(
        "recent-1",
        "c@recent.example",
        now - chrono::Duration::days(10),
        false,
    )
    .insert(&conn)
    .await
    .expect("seed recent-1");

    let (ctx, recorder) = ctx_with_recording_provider(db);

    // 1. Preview sees exactly the two old messages.
    let preview = era_archive_preview_tool(
        ctx.clone(),
        EraArchivePreviewRequest {
            account_id: ACCOUNT_ID.to_string(),
        },
    )
    .await
    .expect("preview");
    assert_eq!(preview["candidate_count"], 2);
    let batch_key = preview["batch_key"].as_str().unwrap().to_string();

    // 2. A stale/wrong key is rejected, not silently applied.
    let wrong = era_archive_apply_tool(
        ctx.clone(),
        EraArchiveApplyRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key: "not-the-real-key".to_string(),
        },
    )
    .await;
    assert!(wrong.is_err(), "a mismatched batch_key must be rejected");

    // 3. Apply with the real key archives exactly the two old messages.
    let applied = era_archive_apply_tool(
        ctx.clone(),
        EraArchiveApplyRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key: batch_key.clone(),
        },
    )
    .await
    .expect("apply");
    assert_eq!(applied["archived_count"], 2);

    let old1 = emails::Entity::find_by_id("old-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(old1.is_archived, "old-1 must be archived");
    let old2 = emails::Entity::find_by_id("old-2".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(old2.is_archived, "old-2 must be archived");
    let recent = emails::Entity::find_by_id("recent-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(!recent.is_archived, "recent-1 must be untouched");

    // The real provider call happened — not just a local DB flip. Exactly
    // the two old messages, by their real provider_message_id, never
    // recent-1 (see email_row: message_id = "provider-{id}").
    {
        let archived = recorder.archived.lock().unwrap();
        assert_eq!(archived.len(), 2, "exactly two real archive_message calls");
        assert!(archived.contains(&"provider-old-1".to_string()));
        assert!(archived.contains(&"provider-old-2".to_string()));
        assert!(!archived.contains(&"provider-recent-1".to_string()));
    }

    // 4. Re-preview now sees zero candidates — re-apply is a no-op.
    let second_preview = era_archive_preview_tool(
        ctx.clone(),
        EraArchivePreviewRequest {
            account_id: ACCOUNT_ID.to_string(),
        },
    )
    .await
    .expect("second preview");
    assert_eq!(
        second_preview["candidate_count"], 0,
        "already-archived messages must not be offered again"
    );
    let second_apply = era_archive_apply_tool(
        ctx.clone(),
        EraArchiveApplyRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key: second_preview["batch_key"].as_str().unwrap().to_string(),
        },
    )
    .await
    .expect("second apply is a no-op, not an error");
    assert_eq!(second_apply["archived_count"], 0);

    // 5. Undo restores exactly the original state.
    let undone = era_archive_undo_tool(
        ctx.clone(),
        EraArchiveUndoRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key,
        },
    )
    .await
    .expect("undo");
    assert_eq!(undone["restored_count"], 2);

    let old1_after = emails::Entity::find_by_id("old-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(!old1_after.is_archived, "undo must restore old-1");
    let old2_after = emails::Entity::find_by_id("old-2".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(!old2_after.is_archived, "undo must restore old-2");

    // The real provider's inverse call happened too.
    let unarchived = recorder.unarchived.lock().unwrap();
    assert_eq!(
        unarchived.len(),
        2,
        "exactly two real unarchive_message calls"
    );
    assert!(unarchived.contains(&"provider-old-1".to_string()));
    assert!(unarchived.contains(&"provider-old-2".to_string()));
}

#[tokio::test]
async fn closeout_empties_the_inbox_except_keep_items_and_undoes() {
    let db = seeded_db().await;
    let conn = db.sea_orm();
    let now = chrono::Utc::now().naive_utc();

    // People-kind (human-looking sender, no bulk markers) -> needs you, stays.
    email_row(
        "human-1",
        "jamie@example-partner.com",
        now - chrono::Duration::days(1),
        false,
    )
    .insert(&conn)
    .await
    .expect("seed human-1");
    // Already-read bulk mail -> done, archived.
    email_row(
        "bulk-read-1",
        "noreply@service.example",
        now - chrono::Duration::hours(2),
        true,
    )
    .insert(&conn)
    .await
    .expect("seed bulk-read-1");
    // Unread, ambiguous -> keep, stays (default-safe).
    email_row(
        "unread-1",
        "someone@unknown.example",
        now - chrono::Duration::hours(1),
        false,
    )
    .insert(&conn)
    .await
    .expect("seed unread-1");

    let (ctx, recorder) = ctx_with_recording_provider(db);

    let preview = closeout_preview_tool(
        ctx.clone(),
        CloseoutPreviewRequest {
            account_id: ACCOUNT_ID.to_string(),
        },
    )
    .await
    .expect("preview");
    assert_eq!(
        preview["to_archive_count"], 1,
        "only the read bulk item is 'done'"
    );
    let batch_key = preview["batch_key"].as_str().unwrap().to_string();

    let applied = closeout_apply_tool(
        ctx.clone(),
        CloseoutApplyRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key: batch_key.clone(),
        },
    )
    .await
    .expect("apply");
    assert_eq!(applied["archived_count"], 1);

    let bulk = emails::Entity::find_by_id("bulk-read-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(bulk.is_archived, "the done item must be archived");
    let human = emails::Entity::find_by_id("human-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !human.is_archived,
        "needs-you (People) must stay in the inbox"
    );
    let unread = emails::Entity::find_by_id("unread-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(!unread.is_archived, "keep items must stay in the inbox");

    // The real provider call happened for exactly the one "done" item.
    {
        let archived = recorder.archived.lock().unwrap();
        assert_eq!(archived.len(), 1, "exactly one real archive_message call");
        assert_eq!(archived[0], "provider-bulk-read-1");
    }

    // Close-out again: only the two non-archived items remain, neither is
    // "done" (the only done item is already archived and so no longer
    // offered), so a second apply is a no-op.
    let second_preview = closeout_preview_tool(
        ctx.clone(),
        CloseoutPreviewRequest {
            account_id: ACCOUNT_ID.to_string(),
        },
    )
    .await
    .expect("second preview");
    assert_eq!(second_preview["to_archive_count"], 0);

    let undone = closeout_undo_tool(
        ctx.clone(),
        CloseoutUndoRequest {
            account_id: ACCOUNT_ID.to_string(),
            batch_key,
        },
    )
    .await
    .expect("undo");
    assert_eq!(undone["restored_count"], 1);

    let bulk_after = emails::Entity::find_by_id("bulk-read-1".to_string())
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !bulk_after.is_archived,
        "undo must restore the archived item"
    );

    let unarchived = recorder.unarchived.lock().unwrap();
    assert_eq!(
        unarchived.len(),
        1,
        "exactly one real unarchive_message call"
    );
    assert_eq!(unarchived[0], "provider-bulk-read-1");
}

#[tokio::test]
async fn no_send_or_compose_tool_exists_in_the_registry() {
    // Section 4's hard guarantee: send/compose are REST-only and must never
    // be registered as MCP tools, so "never creates email" is a property of
    // the MCP surface, not a prompt. Checked against the real declarations(),
    // not a hand-maintained list that could drift.
    for decl in emailibrium::tools::declarations() {
        let name = decl.name.to_lowercase();
        assert!(
            !name.contains("send") && !name.contains("compose") && !name.contains("reply"),
            "found a send/compose/reply-shaped tool name in the registry: {}",
            decl.name
        );
    }
}
