//! Golden-mailbox fixtures (mail-agent plan, Milestone 1): proves the
//! header-rule Kind classifier, run against the real `ImapProvider` client
//! and the fake IMAP server (not a reimplementation of either), produces a
//! Kind distribution consistent with the three accounts measured in
//! `docs/plan/mail-agent-2026-10.md` §2 (26 Oct 2026 profiles) — at the
//! classifier level, since the full `get_snapshot` MCP tool that would
//! compute an actual "archetype" doesn't exist until milestone 2. Approximate
//! percentages, not exact message counts; this is a shape fixture, not a
//! volume test (tens of messages, not the real accounts' tens of thousands).

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use emailibrium::cleanup::domain::kind::{classify, ClassificationContext, Kind};
use emailibrium::email::imap::{
    ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ProviderProfile,
};
use emailibrium::email::types::ListParams;

use fake_imap::{FakeImapServer, FixtureMessage};

fn fixture_config(port: u16, username: &str) -> ImapConfig {
    ImapConfig {
        host: "127.0.0.1".to_string(),
        port,
        encryption: ImapEncryption::Plaintext,
        username: username.to_string(),
        auth: ImapAuth::Password("fixture-app-password".to_string()),
        profile: ProviderProfile::Generic,
        mailbox: "INBOX".to_string(),
        archive_folder: "Archive".to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    }
}

/// Fetch every fixture message's headers through the real client and
/// classify each with the real header-rule classifier — no shortcuts.
async fn classify_fixture(messages: Vec<FixtureMessage>, username: &str) -> Vec<Kind> {
    let server = FakeImapServer::spawn(messages).await;
    let config = fixture_config(server.addr.port(), username);
    let provider = ImapProvider::new(config);

    let page = provider
        .list_message_headers(&ListParams {
            max_results: 100,
            page_token: None,
            label: None,
            query: None,
        })
        .await
        .expect("list_message_headers against the fake IMAP server should succeed");

    let ctx = ClassificationContext::default();
    page.messages
        .iter()
        .map(|m| classify(m, &ctx).kind)
        .collect()
}

fn share(kinds: &[Kind], target: Kind) -> f64 {
    let matches = kinds.iter().filter(|k| **k == target).count();
    matches as f64 / kinds.len() as f64
}

/// §2's business profile: "Circle notifications 22%, marketing drips";
/// "Human share ~2% (~10 correspondents)". Dominant inflow is machine mail.
fn business_account_fixture() -> Vec<FixtureMessage> {
    let mut msgs = vec![FixtureMessage::new(
        1,
        "jamie@example-partner.com",
        "chris@cpventuresgroup.com",
        "Re: Q4 contract review",
        "Mon, 26 Oct 2026 09:12:00 -0400",
        "Looks good, one small change on page 3.",
    )];
    // Circle community-platform notifications (circle.so is a known
    // platform domain, matching §5's "Circle" pattern).
    for i in 0..4u32 {
        msgs.push(FixtureMessage::new(
            10 + i,
            "noreply@circle.so",
            "chris@cpventuresgroup.com",
            "New reply in your community",
            "Mon, 26 Oct 2026 10:00:00 -0400",
            "Someone replied to your post.",
        ));
    }
    // Marketing drips (List-Unsubscribe present -> Subscriptions).
    for i in 0..6u32 {
        msgs.push(
            FixtureMessage::new(
                20 + i,
                "newsletter@vendor-marketing.example",
                "chris@cpventuresgroup.com",
                "This week's picks",
                "Mon, 26 Oct 2026 11:00:00 -0400",
                "Check out our latest offers.",
            )
            .with_header(
                "List-Unsubscribe",
                "<mailto:unsub@vendor-marketing.example>",
            ),
        );
    }
    // Receipt-signature local-parts.
    msgs.push(FixtureMessage::new(
        30,
        "invoice@billing-provider.example",
        "chris@cpventuresgroup.com",
        "Your October invoice",
        "Tue, 27 Oct 2026 08:00:00 -0400",
        "Amount charged: $29.00.",
    ));
    msgs.push(FixtureMessage::new(
        31,
        "receipt@saas-tool.example",
        "chris@cpventuresgroup.com",
        "Payment received",
        "Tue, 27 Oct 2026 08:05:00 -0400",
        "Thanks for your payment.",
    ));
    msgs
}

/// §2's professional profile: "GitHub 62% (423 CI/push noise of 867),
/// LinkedIn 11%"; "moderate: recruiters + household" human share.
fn professional_account_fixture() -> Vec<FixtureMessage> {
    let mut msgs = Vec::new();
    // GitHub notifications, reason-routed by cc per §7.
    let github_reasons = [
        ("ci_activity", 5),
        ("push", 3),
        ("mention", 2),
        ("review_requested", 2),
        ("comment", 2),
    ];
    let mut uid = 100u32;
    for (reason, count) in github_reasons {
        for _ in 0..count {
            msgs.push(
                FixtureMessage::new(
                    uid,
                    "notifications@github.com",
                    "christopher.a.phillipson@gmail.com",
                    "[pacphi/emailibrium] activity",
                    "Mon, 26 Oct 2026 10:00:00 -0400",
                    "Activity on a repository you're watching.",
                )
                .with_header("Cc", &format!("{reason}@notifications.github.com")),
            );
            uid += 1;
        }
    }
    // LinkedIn job alerts (noreply marker -> Notifications).
    for _ in 0..2 {
        msgs.push(FixtureMessage::new(
            uid,
            "jobs-noreply@linkedin.com",
            "christopher.a.phillipson@gmail.com",
            "3 new jobs match your profile",
            "Mon, 26 Oct 2026 12:00:00 -0400",
            "New jobs available.",
        ));
        uid += 1;
    }
    // Recruiter + household humans.
    for (from, subject) in [
        ("recruiter@talent-search.example", "Exciting opportunity"),
        ("mom@family-mail.example", "Dinner Sunday?"),
        ("coach@kids-soccer.example", "Practice moved to 5pm"),
    ] {
        msgs.push(FixtureMessage::new(
            uid,
            from,
            "christopher.a.phillipson@gmail.com",
            subject,
            "Mon, 26 Oct 2026 13:00:00 -0400",
            "See subject.",
        ));
        uid += 1;
    }
    msgs
}

/// §2's personal profile: "Promotions 31% of all mail; Amazon
/// returns/orders in bursts"; "largest: family, household, community,
/// travel" human share.
fn personal_account_fixture() -> Vec<FixtureMessage> {
    let mut msgs = Vec::new();
    let mut uid = 200u32;
    // Promotional mail with List-Unsubscribe.
    for _ in 0..6 {
        msgs.push(
            FixtureMessage::new(
                uid,
                "deals@retailer-promo.example",
                "fastnsilver@gmail.com",
                "Flash sale ends tonight",
                "Mon, 26 Oct 2026 09:00:00 -0400",
                "Don't miss out.",
            )
            .with_header("List-Unsubscribe", "<mailto:unsub@retailer-promo.example>"),
        );
        uid += 1;
    }
    // Amazon event-address receipts (§7's named local-part signatures).
    for local in [
        "order-update",
        "shipment-tracking",
        "return",
        "auto-confirm",
        "payments-messages",
    ] {
        msgs.push(FixtureMessage::new(
            uid,
            &format!("{local}@amazon.example"),
            "fastnsilver@gmail.com",
            "Your order update",
            "Mon, 26 Oct 2026 10:00:00 -0400",
            "Order status changed.",
        ));
        uid += 1;
    }
    // Family, household, community humans — the largest share.
    for (from, subject) in [
        ("sister@family-mail.example", "Thanksgiving plans"),
        ("dad@family-mail.example", "Call me when you can"),
        ("neighbor@community-mail.example", "HOA meeting Thursday"),
        ("friend.from.college@example.com", "Long time!"),
        ("travel-buddy@example.com", "Flights booked"),
        ("book-club@community-mail.example", "This month's pick"),
        ("cousin@family-mail.example", "Happy birthday!"),
    ] {
        msgs.push(FixtureMessage::new(
            uid,
            from,
            "fastnsilver@gmail.com",
            subject,
            "Mon, 26 Oct 2026 11:00:00 -0400",
            "See subject.",
        ));
        uid += 1;
    }
    msgs
}

#[tokio::test]
async fn business_profile_is_machine_dominated_with_low_human_share() {
    let kinds = classify_fixture(business_account_fixture(), "chris@cpventuresgroup.com").await;

    let people_share = share(&kinds, Kind::People);
    let notifications_share = share(&kinds, Kind::Notifications);
    let subscriptions_share = share(&kinds, Kind::Subscriptions);

    // §2: human share ~2%, machine mail dominant. Tolerant threshold, not
    // an exact match — this is a shape fixture, not the real 4,300-message
    // account.
    assert!(
        people_share < 0.15,
        "business account should have a small human share, got {people_share}"
    );
    assert!(
        notifications_share + subscriptions_share > 0.6,
        "business account should be machine-mail-dominated, got notifications={notifications_share} subscriptions={subscriptions_share}"
    );
}

#[tokio::test]
async fn professional_profile_has_github_majority_and_a_moderate_human_signal() {
    let kinds = classify_fixture(
        professional_account_fixture(),
        "christopher.a.phillipson@gmail.com",
    )
    .await;

    let notifications_share = share(&kinds, Kind::Notifications);
    let people_share = share(&kinds, Kind::People);

    // §2: "GitHub 62%... LinkedIn 11%" ~= 73% combined notifications share.
    assert!(
        notifications_share > 0.55,
        "professional account should be notifications-dominated (GitHub+LinkedIn), got {notifications_share}"
    );
    // §2: "moderate: recruiters + household" — present, not negligible,
    // and higher than the business account's ~2% (checked by the
    // dedicated comparison test below, not duplicated here).
    assert!(
        people_share > 0.10,
        "professional account should have a moderate human share, got {people_share}"
    );
}

#[tokio::test]
async fn personal_profile_has_the_largest_human_share_of_the_three() {
    let kinds = classify_fixture(personal_account_fixture(), "fastnsilver@gmail.com").await;

    let people_share = share(&kinds, Kind::People);
    let receipts_share = share(&kinds, Kind::Receipts);

    // §2: "largest: family, household, community, travel" — the plan's
    // own ranking (business < professional < personal) is the thing
    // worth proving, checked directly below.
    assert!(
        people_share > 0.25,
        "personal account should have the largest human share, got {people_share}"
    );
    assert!(
        receipts_share > 0.15,
        "personal account should show meaningful Amazon-pattern receipts, got {receipts_share}"
    );
}

/// The ranking §2 actually asserts ("Human share ... business ~2% <
/// professional moderate < personal largest") is the signal a later
/// milestone's archetype detector would use to flag the professional
/// account "Mixed" — this proves the ranking holds at the classifier
/// level, which is as far as milestone 1 goes.
#[tokio::test]
async fn human_share_ranks_business_lowest_then_professional_then_personal() {
    let business = share(
        &classify_fixture(business_account_fixture(), "chris@cpventuresgroup.com").await,
        Kind::People,
    );
    let professional = share(
        &classify_fixture(
            professional_account_fixture(),
            "christopher.a.phillipson@gmail.com",
        )
        .await,
        Kind::People,
    );
    let personal = share(
        &classify_fixture(personal_account_fixture(), "fastnsilver@gmail.com").await,
        Kind::People,
    );

    assert!(
        business < professional,
        "business ({business}) should have a lower human share than professional ({professional})"
    );
    assert!(
        professional < personal,
        "professional ({professional}) should have a lower human share than personal ({personal})"
    );
}

/// §7's reason-routing-by-cc rule, proven against the real classifier and
/// the GitHub-heavy professional fixture, not a synthetic single message.
#[tokio::test]
async fn professional_profile_github_reasons_route_as_the_plan_describes() {
    let server = FakeImapServer::spawn(professional_account_fixture()).await;
    let config = fixture_config(server.addr.port(), "christopher.a.phillipson@gmail.com");
    let provider = ImapProvider::new(config);

    let page = provider
        .list_message_headers(&ListParams {
            max_results: 100,
            page_token: None,
            label: None,
            query: None,
        })
        .await
        .expect("list_message_headers should succeed");

    let ctx = ClassificationContext::default();
    let mut needs_you = 0;
    let mut filed = 0;
    let mut source_settings_candidate = 0;
    for m in &page.messages {
        if let Some(reason) = classify(m, &ctx).github_reason {
            use emailibrium::cleanup::domain::kind::GithubReason;
            match reason {
                GithubReason::NeedsYou => needs_you += 1,
                GithubReason::Filed => filed += 1,
                GithubReason::SourceSettingsCandidate => source_settings_candidate += 1,
            }
        }
    }

    // Fixture seeded 4 needs-you (mention x2, review_requested x2), 2 filed
    // (comment x2), 8 source-settings-candidate (ci_activity x5, push x3).
    assert_eq!(needs_you, 4, "mention/review_requested should need you");
    assert_eq!(filed, 2, "comment should be filed");
    assert_eq!(
        source_settings_candidate, 8,
        "ci_activity/push should be source-settings candidates"
    );
}
