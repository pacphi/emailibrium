//! Integration test (mail-agent plan, Milestone 0): proves emailibrium's
//! real IMAP client code — `ImapProvider`, built on `async-imap` — can log
//! in, select a mailbox, and fetch headers from a real (if minimal)
//! wire-protocol IMAP server, not a reimplementation of the client.
//!
//! This exercises `ImapProvider::list_messages` directly rather than
//! replicating its SQL/HTTP surface, per this session's standing finding
//! that replica-style tests (see `tests/api_integration.rs`) can miss real
//! bugs in the path they claim to cover.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use emailibrium::email::imap::{ImapConfig, ImapEncryption, ImapProvider};
use emailibrium::email::provider::EmailProvider;
use emailibrium::email::types::ListParams;

use fake_imap::{FakeImapServer, FixtureMessage};

/// A small mailbox shaped like the plan's "business" account profile
/// (§2 of `docs/plan/mail-agent-2026-10.md`): a human correspondent, a
/// GitHub-style notification, and a receipt — tens of messages' worth of
/// shape, not the real account's volume. This is a protocol-conformance
/// fixture, not a load test.
fn business_account_fixture() -> Vec<FixtureMessage> {
    vec![
        FixtureMessage::new(
            101,
            "jamie@example-partner.com",
            "chris@cpventuresgroup.com",
            "Re: Q4 contract review",
            "Mon, 26 Oct 2026 09:12:00 -0400",
            "Looks good, one small change on page 3.",
        ),
        FixtureMessage::new(
            102,
            "notifications@github.com",
            "chris@cpventuresgroup.com",
            "[pacphi/emailibrium] CI run failed on develop",
            "Mon, 26 Oct 2026 10:03:00 -0400",
            "The build failed. See the run for details.",
        ),
        FixtureMessage::new(
            103,
            "receipt@circle.so",
            "chris@cpventuresgroup.com",
            "Your October invoice",
            "Tue, 27 Oct 2026 08:00:00 -0400",
            "Your subscription renewed. Amount charged: $29.00.",
        ),
    ]
}

fn fixture_config(port: u16) -> ImapConfig {
    ImapConfig {
        host: "127.0.0.1".to_string(),
        port,
        encryption: ImapEncryption::Plaintext,
        username: "chris@cpventuresgroup.com".to_string(),
        password: "fixture-app-password".to_string(),
        mailbox: "INBOX".to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    }
}

#[tokio::test]
async fn list_messages_round_trips_against_fake_imap_server() {
    let server = FakeImapServer::spawn(business_account_fixture()).await;
    let config = fixture_config(server.addr.port());
    let provider = ImapProvider::new(config);

    let params = ListParams {
        max_results: 10,
        page_token: None,
        label: None,
        query: None,
    };

    let page = provider
        .list_messages("unused-token", &params)
        .await
        .expect("list_messages against the fake IMAP server should succeed");

    assert_eq!(
        page.messages.len(),
        3,
        "expected all three fixture messages to round-trip"
    );

    let subjects: Vec<&str> = page.messages.iter().map(|m| m.subject.as_str()).collect();
    assert!(subjects.contains(&"Re: Q4 contract review"));
    assert!(subjects.contains(&"[pacphi/emailibrium] CI run failed on develop"));
    assert!(subjects.contains(&"Your October invoice"));

    // UID SEARCH + UID FETCH preserved the server-assigned UIDs, which is
    // what later milestones' sender-key and era logic will key off of.
    let ids: Vec<&str> = page.messages.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"101"));
    assert!(ids.contains(&"102"));
    assert!(ids.contains(&"103"));
}

#[tokio::test]
async fn get_message_round_trips_a_single_uid() {
    let server = FakeImapServer::spawn(business_account_fixture()).await;
    let config = fixture_config(server.addr.port());
    let provider = ImapProvider::new(config);

    let msg = provider
        .get_message("unused-token", "102")
        .await
        .expect("get_message against the fake IMAP server should succeed");

    assert_eq!(msg.subject, "[pacphi/emailibrium] CI run failed on develop");
    assert_eq!(msg.from, "notifications@github.com");
}

/// Milestone 1 (header-rule Kind classifier) calls `list_message_headers`
/// instead of `list_messages`. This proves, against the real `ImapProvider`
/// client, that it (a) returns header fields with no body/snippet at all,
/// and (b) never marks a message `\Seen` as a side effect — the plan's
/// "headers only, never bodies" and "scanning marks nothing read" promises,
/// checked against the real fetch path rather than asserted by naming.
#[tokio::test]
async fn list_message_headers_returns_headers_without_body_and_does_not_mark_seen() {
    let server = FakeImapServer::spawn(business_account_fixture()).await;
    let config = fixture_config(server.addr.port());
    let provider = ImapProvider::new(config);

    let params = ListParams {
        max_results: 10,
        page_token: None,
        label: None,
        query: None,
    };

    let page = provider
        .list_message_headers(&params)
        .await
        .expect("list_message_headers against the fake IMAP server should succeed");

    assert_eq!(
        page.messages.len(),
        3,
        "expected all three fixture messages' headers to round-trip"
    );

    let ci_notification = page
        .messages
        .iter()
        .find(|m| m.id == "102")
        .expect("CI notification header should be present");
    assert_eq!(
        ci_notification.subject,
        "[pacphi/emailibrium] CI run failed on develop"
    );
    assert_eq!(ci_notification.from, "notifications@github.com");
    assert_eq!(
        ci_notification.body, None,
        "header-only fetch must never carry a body"
    );
    assert_eq!(
        ci_notification.body_html, None,
        "header-only fetch must never carry an HTML body"
    );
    assert_eq!(
        ci_notification.snippet, "",
        "header-only fetch must never carry a body-derived snippet"
    );

    for uid in [101u32, 102, 103] {
        assert!(
            !server.is_seen(uid),
            "BODY.PEEK[HEADER] must never mark a message \\Seen (uid {uid})"
        );
    }
}
