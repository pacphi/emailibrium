//! Regression test for the IMAP `\Seen` side-effect bug: `ImapProvider`'s
//! message fetch used to request `RFC822`, which per RFC 3501 §6.4.5
//! implicitly marks a message `\Seen` on the server as a side effect of
//! fetching it -- silently contradicting the mail-agent plan's onboarding
//! promise ("Scanning headers only -- nothing will be marked read.").
//!
//! The fix changes the fetch item to `BODY.PEEK[]`, which returns the
//! identical message bytes (both populate `async_imap`'s `Fetch::body()`
//! via the same `BodySection{section: None}` attribute) but never sets
//! `\Seen`. This proves it against the real `ImapProvider`/async-imap
//! client and a fake server that tracks real per-UID `\Seen` state across
//! connections -- not a reimplementation of the client, and not a
//! hardcoded server response.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use emailibrium::email::imap::{
    ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ProviderProfile,
};
use emailibrium::email::provider::EmailProvider;
use emailibrium::email::types::ListParams;

use fake_imap::{FakeImapServer, FixtureMessage};

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
        auth: ImapAuth::Password("fixture-app-password".to_string()),
        profile: ProviderProfile::Generic,
        mailbox: "INBOX".to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    }
}

/// The regression itself: list_messages (full body, used by ingestion)
/// must not mark any fetched message `\Seen` on the server.
#[tokio::test]
async fn list_messages_does_not_mark_fetched_messages_seen() {
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

    assert_eq!(page.messages.len(), 3, "fixture should round-trip in full");

    for uid in [101u32, 102, 103] {
        assert!(
            !server.is_seen(uid),
            "list_messages (BODY.PEEK[]) must not mark uid {uid} \\Seen -- \
             the plan promises 'nothing will be marked read' during a scan"
        );
    }
}

/// Same promise for get_message (used by incremental sync and reply/forward
/// quoting).
#[tokio::test]
async fn get_message_does_not_mark_the_fetched_message_seen() {
    let server = FakeImapServer::spawn(business_account_fixture()).await;
    let config = fixture_config(server.addr.port());
    let provider = ImapProvider::new(config);

    let msg = provider
        .get_message("unused-token", "102")
        .await
        .expect("get_message against the fake IMAP server should succeed");

    // The fetch must still return the real, full message content --
    // this is not a header-only fetch, just a non-\Seen-setting one.
    assert_eq!(msg.subject, "[pacphi/emailibrium] CI run failed on develop");
    assert_eq!(
        msg.body.as_deref(),
        Some("The build failed. See the run for details.\r\n"),
        "BODY.PEEK[] must return the same full body RFC822 did"
    );

    assert!(
        !server.is_seen(102),
        "get_message (BODY.PEEK[]) must not mark uid 102 \\Seen"
    );
}

/// Sanity check on the fake server itself: a non-PEEK fetch item (what the
/// code used to send) really does set \Seen, so the two tests above are
/// actually exercising the fix and not a server that never sets the flag
/// at all.
#[tokio::test]
async fn fake_server_sanity_a_non_peek_fetch_does_set_seen() {
    use async_imap::Client;
    use futures::TryStreamExt;
    use tokio::net::TcpStream;

    let server = FakeImapServer::spawn(business_account_fixture()).await;
    let stream = TcpStream::connect(server.addr)
        .await
        .expect("connect to fake IMAP server");
    let mut client = Client::new(stream);
    client
        .read_response()
        .await
        .expect("greeting")
        .expect("greeting present");

    let mut session = client
        .login("chris@cpventuresgroup.com", "fixture-app-password")
        .await
        .expect("login");
    session.select("INBOX").await.expect("select");

    let fetches = session
        .uid_fetch("101", "(FLAGS RFC822)")
        .await
        .expect("fetch");
    let _raw: Vec<_> = fetches.try_collect().await.expect("fetch stream");

    assert!(
        server.is_seen(101),
        "a plain RFC822 fetch (the pre-fix behavior) must set \\Seen -- \
         otherwise this test suite can't tell a real fix from a server \
         that never flips the flag"
    );
}
