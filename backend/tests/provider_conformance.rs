//! Provider-adapter conformance suite (mail-agent plan, milestone 6 exit
//! test, fake-server half): every `ImapProvider` profile — `Generic` with and
//! without custom keywords, `ProtonBridge`, `OutlookImap` — applies the same
//! labels, and undoing them restores the mailbox to the same state it started
//! in (same messages per mailbox, same keywords). Archive → unarchive is
//! checked the same way for each move strategy (MOVE, UIDPLUS, neither).
//!
//! What "undo" means differs per provider, and that difference is the point:
//! - keywords (Generic): `remove_labels`;
//! - Proton `Labels/` copies: `remove_labels` deletes the copy;
//! - Kind/Topic as a folder (Outlook, Generic without keywords): move the
//!   message back out of that folder. `remove_labels` refuses this and
//!   `move_message` only moves out of the configured mailbox, so the suite
//!   builds a provider whose mailbox is the label folder. No production undo
//!   path does that yet.
//!
//! The live half of the exit test (real mailboxes) is separate and gated.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use std::collections::{BTreeMap, HashMap};

use emailibrium::email::imap::{
    ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ProviderProfile,
};
use emailibrium::email::provider::{EmailProvider, MoveKind};

use fake_imap::{FakeImapServer, FixtureMessage};

const UID: u32 = 7;
const KIND: &str = "Kind/Newsletters";
const STATE: &str = "State/ReadLater";

type Snapshot = BTreeMap<String, Vec<(String, Vec<String>)>>;

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

/// INBOX with the message under test and a bystander that must never move.
async fn spawn(capabilities: &[&str], extra_mailboxes: &[&str]) -> FakeImapServer {
    let mut seed = HashMap::new();
    seed.insert("INBOX".to_string(), vec![msg(UID), msg(UID + 1)]);
    seed.insert("Archive".to_string(), Vec::new());
    for name in extra_mailboxes {
        seed.insert(name.to_string(), Vec::new());
    }
    FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, capabilities).await
}

fn provider(port: u16, profile: ProviderProfile, mailbox: &str) -> ImapProvider {
    ImapProvider::new(ImapConfig {
        host: "127.0.0.1".to_string(),
        port,
        encryption: ImapEncryption::Plaintext,
        username: "chris@example.com".to_string(),
        auth: ImapAuth::Password("fixture-app-password".to_string()),
        profile,
        mailbox: mailbox.to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    })
}

fn labels() -> Vec<String> {
    vec![KIND.to_string(), STATE.to_string()]
}

fn only(server: &FakeImapServer, mailbox: &str) -> u32 {
    let uids = server.mailbox_uids(mailbox);
    assert_eq!(uids.len(), 1, "expected exactly one message in {mailbox}");
    uids[0]
}

/// Move the labelled message back out of its Kind folder into INBOX.
async fn undo_folder_label(server: &FakeImapServer, profile: ProviderProfile) {
    let uid = only(server, KIND);
    provider(server.addr.port(), profile, KIND)
        .move_message("unused", &uid.to_string(), "INBOX", MoveKind::Folder)
        .await
        .expect("move back to INBOX");
}

#[tokio::test]
async fn generic_with_keywords_round_trips() {
    let server = spawn(&["MOVE", "UIDPLUS"], &[]).await;
    server.set_permanent_flags("\\Seen \\Deleted \\*");
    let before: Snapshot = server.snapshot();
    let p = provider(server.addr.port(), ProviderProfile::Generic, "INBOX");

    p.label_message("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(
        server.keywords("INBOX", UID),
        labels(),
        "labels are keywords"
    );
    assert_eq!(server.mailbox_uids("INBOX").len(), 2, "nothing moved");

    p.remove_labels("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(server.snapshot(), before);
}

#[tokio::test]
async fn generic_without_keywords_round_trips_through_a_folder() {
    let server = spawn(&["MOVE"], &[]).await;
    server.set_permanent_flags("\\Seen \\Deleted");
    let before = server.snapshot();
    let p = provider(server.addr.port(), ProviderProfile::Generic, "INBOX");

    p.label_message("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(server.mailbox_uids("INBOX"), vec![UID + 1], "moved out");
    let moved = only(&server, KIND);
    assert!(server.keywords(KIND, moved).is_empty(), "State not written");

    // State was never written: removing it is a no-op.
    p.remove_labels("unused", &UID.to_string(), &[STATE.to_string()])
        .await
        .unwrap();
    undo_folder_label(&server, ProviderProfile::Generic).await;
    assert_eq!(server.snapshot(), before);
}

#[tokio::test]
async fn proton_bridge_round_trips_through_label_copies() {
    let server = spawn(&["MOVE", "UIDPLUS"], &[]).await;
    let before = server.snapshot();
    let p = provider(server.addr.port(), ProviderProfile::ProtonBridge, "INBOX");

    p.label_message("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(server.mailbox_uids("INBOX").len(), 2, "original stays put");
    assert_eq!(server.mailbox_uids(&format!("Labels/{KIND}")).len(), 1);
    assert_eq!(server.mailbox_uids(&format!("Labels/{STATE}")).len(), 1);

    p.remove_labels("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(server.snapshot(), before);
}

#[tokio::test]
async fn outlook_round_trips_through_a_folder() {
    let server = spawn(&["MOVE", "UIDPLUS"], &[]).await;
    let before = server.snapshot();
    let p = provider(server.addr.port(), ProviderProfile::OutlookImap, "INBOX");

    p.label_message("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    assert_eq!(server.mailbox_uids("INBOX"), vec![UID + 1], "moved out");
    assert_eq!(server.mailbox_uids(KIND).len(), 1);

    p.remove_labels("unused", &UID.to_string(), &[STATE.to_string()])
        .await
        .unwrap();
    undo_folder_label(&server, ProviderProfile::OutlookImap).await;
    assert_eq!(server.snapshot(), before);
}

#[tokio::test]
async fn archive_round_trips_for_every_profile_and_move_strategy() {
    let strategies: [&[&str]; 3] = [&["MOVE"], &["UIDPLUS"], &[]];
    let profiles = [
        ProviderProfile::Generic,
        ProviderProfile::ProtonBridge,
        ProviderProfile::OutlookImap,
    ];
    for caps in strategies {
        for profile in profiles {
            let server = spawn(caps, &[]).await;
            let before = server.snapshot();
            let p = provider(server.addr.port(), profile, "INBOX");

            p.archive_message("unused", &UID.to_string())
                .await
                .unwrap_or_else(|e| panic!("archive {profile:?} {caps:?}: {e}"));
            assert_eq!(server.mailbox_uids("INBOX"), vec![UID + 1]);
            let archived = only(&server, "Archive");

            p.unarchive_message("unused", &archived.to_string())
                .await
                .unwrap_or_else(|e| panic!("unarchive {profile:?} {caps:?}: {e}"));
            assert_eq!(server.snapshot(), before, "{profile:?} {caps:?}");
        }
    }
}

#[tokio::test]
async fn keyword_round_trip_preserves_keywords_already_on_the_message() {
    // A keyword the user (or another client) set must survive our undo.
    let server = spawn(&["MOVE"], &[]).await;
    server.set_permanent_flags("\\Seen \\Deleted \\*");
    let p = provider(server.addr.port(), ProviderProfile::Generic, "INBOX");
    p.label_message("unused", &UID.to_string(), &["$Important".to_string()])
        .await
        .unwrap();
    let before = server.snapshot();

    p.label_message("unused", &UID.to_string(), &labels())
        .await
        .unwrap();
    p.remove_labels("unused", &UID.to_string(), &labels())
        .await
        .unwrap();

    assert_eq!(server.snapshot(), before);
    assert_eq!(
        server.keywords("INBOX", UID),
        vec!["$Important".to_string()]
    );
}
