//! Integration test: moving one message over IMAP must never expunge any
//! other message. A plain `EXPUNGE` erases every message in the mailbox
//! marked `\Deleted` — including ones another mail client flagged and the
//! user never meant emailibrium to touch. `ImapProvider` must use `UID MOVE`
//! or `UID EXPUNGE` (UIDPLUS), and refuse a server that offers neither
//! before copying anything.
//!
//! Drives the real `ImapProvider` against the in-process fake IMAP server.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use std::collections::HashMap;

use emailibrium::email::imap::{ImapConfig, ImapEncryption, ImapProvider};
use emailibrium::email::provider::{EmailProvider, MoveKind};

use fake_imap::{FakeImapServer, FixtureMessage};

const TO_MOVE: u32 = 1;
const ALREADY_DELETED: u32 = 2;

fn msg(uid: u32) -> FixtureMessage {
    FixtureMessage::new(
        uid,
        "sender@example.com",
        "chris@example.com",
        &format!("Message {uid}"),
        "Mon, 26 Oct 2026 09:00:00 -0400",
        "Body.",
    )
}

/// INBOX holds the message to move plus one another client already marked
/// `\Deleted` but never expunged; `Archive` exists and is empty.
async fn spawn(capabilities: &[&str]) -> FakeImapServer {
    let mut seed = HashMap::new();
    seed.insert(
        "INBOX".to_string(),
        vec![msg(TO_MOVE), msg(ALREADY_DELETED).marked_deleted()],
    );
    seed.insert("Archive".to_string(), Vec::new());
    FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, capabilities).await
}

fn provider(port: u16) -> ImapProvider {
    ImapProvider::new(ImapConfig {
        host: "127.0.0.1".to_string(),
        port,
        encryption: ImapEncryption::Plaintext,
        username: "chris@example.com".to_string(),
        password: "fixture-app-password".to_string(),
        mailbox: "INBOX".to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    })
}

async fn assert_archive_moves_only_target(capabilities: &[&str]) {
    let server = spawn(capabilities).await;

    provider(server.addr.port())
        .archive_message("unused", &TO_MOVE.to_string())
        .await
        .expect("archive should succeed");

    assert_eq!(
        server.mailbox_uids("INBOX"),
        vec![ALREADY_DELETED],
        "the other \\Deleted message must survive the move"
    );
    assert_eq!(server.mailbox_uids("Archive").len(), 1);
}

#[tokio::test]
async fn archive_with_move_leaves_other_deleted_messages() {
    assert_archive_moves_only_target(&["MOVE"]).await;
}

#[tokio::test]
async fn archive_with_uidplus_leaves_other_deleted_messages() {
    assert_archive_moves_only_target(&["UIDPLUS"]).await;
}

#[tokio::test]
async fn move_message_folder_leaves_other_deleted_messages() {
    let server = spawn(&["UIDPLUS"]).await;

    provider(server.addr.port())
        .move_message("unused", &TO_MOVE.to_string(), "Archive", MoveKind::Folder)
        .await
        .expect("move should succeed");

    assert_eq!(server.mailbox_uids("INBOX"), vec![ALREADY_DELETED]);
    assert_eq!(server.mailbox_uids("Archive").len(), 1);
}

#[tokio::test]
async fn unarchive_leaves_other_deleted_messages_in_archive() {
    let mut seed = HashMap::new();
    seed.insert("INBOX".to_string(), Vec::new());
    seed.insert(
        "Archive".to_string(),
        vec![msg(TO_MOVE), msg(ALREADY_DELETED).marked_deleted()],
    );
    let server = FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, &["MOVE"]).await;

    provider(server.addr.port())
        .unarchive_message("unused", &TO_MOVE.to_string())
        .await
        .expect("unarchive should succeed");

    assert_eq!(server.mailbox_uids("Archive"), vec![ALREADY_DELETED]);
    assert_eq!(server.mailbox_uids("INBOX").len(), 1);
}

#[tokio::test]
async fn server_without_move_or_uidplus_is_refused_before_copying() {
    let server = spawn(&[]).await;

    let err = provider(server.addr.port())
        .archive_message("unused", &TO_MOVE.to_string())
        .await
        .expect_err("a server with neither MOVE nor UIDPLUS must be refused");
    assert!(
        err.to_string().contains("neither MOVE nor UIDPLUS"),
        "{err}"
    );

    assert_eq!(
        server.mailbox_uids("INBOX"),
        vec![TO_MOVE, ALREADY_DELETED],
        "nothing removed"
    );
    assert!(
        server.mailbox_uids("Archive").is_empty(),
        "nothing copied before refusing"
    );
}

/// iCloud-shaped server (neither MOVE nor UIDPLUS) with no other message
/// marked `\Deleted`: the guarded plain-EXPUNGE fallback moves the message
/// and leaves every other message in place.
#[tokio::test]
async fn server_without_move_or_uidplus_moves_when_nothing_else_is_deleted() {
    const BYSTANDER: u32 = 3;
    let mut seed = HashMap::new();
    seed.insert("INBOX".to_string(), vec![msg(TO_MOVE), msg(BYSTANDER)]);
    seed.insert("Archive".to_string(), Vec::new());
    let server = FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, &[]).await;

    provider(server.addr.port())
        .archive_message("unused", &TO_MOVE.to_string())
        .await
        .expect("guarded fallback should archive when nothing else is \\Deleted");

    assert_eq!(server.mailbox_uids("INBOX"), vec![BYSTANDER]);
    assert_eq!(server.mailbox_uids("Archive").len(), 1);
}
