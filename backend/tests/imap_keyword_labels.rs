//! Integration test: `Generic`-profile labels (Gmail/Workspace/iCloud/
//! Fastmail/Yahoo app-password) are written as IMAP keywords only when the
//! mailbox keeps them (RFC 3501 §7.1 `PERMANENTFLAGS`), fall back to the
//! folder strategy otherwise, and are validated as RFC 3501 atoms before any
//! `STORE` so label text can never split or inject into the command.
//!
//! Drives the real `ImapProvider` against the in-process fake IMAP server.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use std::collections::HashMap;

use emailibrium::email::imap::{
    ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ProviderProfile,
};
use emailibrium::email::provider::{EmailProvider, MoveKind};

use fake_imap::{FakeImapServer, FixtureMessage};

const UID: u32 = 1;
const KIND: &str = "Kind/Newsletters";
const STATE: &str = "State/New";

fn msg(uid: u32) -> FixtureMessage {
    FixtureMessage::new(
        uid,
        "news@example.com",
        "chris@example.com",
        &format!("Issue {uid}"),
        "Mon, 26 Oct 2026 09:00:00 -0400",
        "Body.",
    )
}

async fn spawn() -> FakeImapServer {
    let mut seed = HashMap::new();
    seed.insert("INBOX".to_string(), vec![msg(UID)]);
    FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, &["MOVE"]).await
}

fn provider(port: u16) -> ImapProvider {
    ImapProvider::new(ImapConfig {
        host: "127.0.0.1".to_string(),
        port,
        encryption: ImapEncryption::Plaintext,
        username: "chris@example.com".to_string(),
        auth: ImapAuth::Password("fixture-app-password".to_string()),
        profile: ProviderProfile::Generic,
        mailbox: "INBOX".to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    })
}

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn keyword_server_stores_and_removes_keywords() {
    let server = spawn().await;
    server.set_permanent_flags("\\Seen \\Deleted \\*");
    let p = provider(server.addr.port());

    p.label_message("unused", &UID.to_string(), &labels(&[KIND, STATE]))
        .await
        .expect("label");
    assert_eq!(server.keywords("INBOX", UID), labels(&[KIND, STATE]));

    p.remove_labels("unused", &UID.to_string(), &labels(&[STATE]))
        .await
        .expect("unlabel");
    assert_eq!(server.keywords("INBOX", UID), labels(&[KIND]));
    assert_eq!(server.mailbox_uids("INBOX"), vec![UID], "message not moved");
}

#[tokio::test]
async fn server_without_permanentflags_is_treated_as_accepting_keywords() {
    let server = spawn().await; // no PERMANENTFLAGS sent: RFC 3501 §7.1 default

    provider(server.addr.port())
        .label_message("unused", &UID.to_string(), &labels(&[KIND]))
        .await
        .expect("label");

    assert_eq!(server.keywords("INBOX", UID), labels(&[KIND]));
}

#[tokio::test]
async fn server_refusing_keywords_falls_back_to_folders() {
    let server = spawn().await;
    server.set_permanent_flags("\\Seen \\Deleted");
    let p = provider(server.addr.port());

    p.label_message("unused", &UID.to_string(), &labels(&[KIND, STATE]))
        .await
        .expect("label via folders");

    assert!(
        server.mailbox_uids("INBOX").is_empty(),
        "moved out of INBOX"
    );
    assert_eq!(
        server.mailbox_uids(KIND).len(),
        1,
        "moved into the Kind folder"
    );
    let moved = server.mailbox_uids(KIND)[0];
    assert!(
        server.keywords(KIND, moved).is_empty(),
        "no keywords written when the server refuses them"
    );

    // State was never written, so removing it is a no-op; Kind undo goes
    // through move_message, so removing it is refused.
    p.remove_labels("unused", &UID.to_string(), &labels(&[STATE]))
        .await
        .expect("State removal is a no-op");
    assert!(p
        .remove_labels("unused", &UID.to_string(), &labels(&[KIND]))
        .await
        .is_err());
}

#[tokio::test]
async fn invalid_label_is_rejected_before_any_keyword_is_stored() {
    let server = spawn().await;
    server.set_permanent_flags("\\Seen \\Deleted \\*");
    let p = provider(server.addr.port());

    for bad in ["Topic/Read Later", "Bad)Label", "x\r\nA1 DELETE INBOX"] {
        let err = p
            .label_message("unused", &UID.to_string(), &labels(&[KIND, bad]))
            .await
            .expect_err("invalid keyword must be rejected");
        assert!(
            err.to_string().contains("not a valid IMAP keyword"),
            "{err}"
        );
    }

    assert!(
        server.keywords("INBOX", UID).is_empty(),
        "the valid label was not half-applied"
    );
    assert_eq!(server.mailbox_uids("INBOX"), vec![UID]);
}

#[tokio::test]
async fn move_message_label_rejects_invalid_keyword() {
    let server = spawn().await;

    let err = provider(server.addr.port())
        .move_message("unused", &UID.to_string(), "Bad)Label", MoveKind::Label)
        .await
        .expect_err("invalid keyword must be rejected");
    assert!(
        err.to_string().contains("not a valid IMAP keyword"),
        "{err}"
    );
    assert!(server.keywords("INBOX", UID).is_empty());
}
