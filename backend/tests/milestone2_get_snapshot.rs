//! End-to-end proof of `get_snapshot` (mail-agent plan, milestone 2) against
//! a business-account-shaped fixture, completing the "snapshot reproduces
//! the Oct 26 profiles" exit test milestone 1's golden-mailbox tests left
//! for this milestone (see `milestone1_golden_mailboxes.rs`'s own doc
//! comment: "the full `get_snapshot` MCP tool ... doesn't exist until
//! milestone 2").
//!
//! **Not the fake-IMAP-server path.** `get_snapshot` reads already-ingested
//! mail from the `emails` table (via `staff::scan::scan_mail`), not from a
//! live IMAP connection — milestone 1's fixtures prove the classifier
//! against the real client; this proves the snapshot tool against the real
//! DB-query path, seeding `emails` rows directly (the same level of
//! integration `milestone2_staff_tools`'s own body-free test uses). Both
//! are "real", at the layer each tool actually operates on.
//!
//! The fixture is shaped like §2's business profile (Circle notifications,
//! marketing drips with `List-Unsubscribe`, receipt local-parts, one human
//! correspondent) but scaled up from milestone 1's dozen-message shape
//! fixture to actually cross `detect_archetype`'s absolute thresholds
//! (`inflow_per_day > 10.0` needs >300 messages inside a 30-day window) —
//! a volume test for this one threshold, not a shape test like milestone
//! 1's.

use std::sync::Arc;

use emailibrium::db::entities::{connected_accounts, emails};
use emailibrium::tools::staff::params::GetSnapshotRequest;
use emailibrium::tools::staff::snapshot::get_snapshot;
use emailibrium::tools::ToolContext;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

const ACCOUNT_ID: &str = "22222222-2222-2222-2222-222222222222";

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
        ],
    )
    .await
    .expect("migrate");

    // Declared "business", matching §2's chris@cpventuresgroup.com example —
    // the archetype this fixture is shaped to measure as, so the snapshot's
    // role-mismatch flag should stay absent (a correctly declared role).
    connected_accounts::ActiveModel {
        id: Set(ACCOUNT_ID.to_string()),
        provider: Set("imap".to_string()),
        email_address: Set("chris@cpventuresgroup.com".to_string()),
        role: Set(Some("business".to_string())),
        ..Default::default()
    }
    .insert(&conn)
    .await
    .expect("seed connected_accounts");

    let now = chrono::Utc::now().naive_utc();
    let mut rows = Vec::new();

    // One human correspondent — §2: "Human share ~2% (~10 correspondents)".
    rows.push(email_row(
        "human-1",
        "jamie@example-partner.com",
        now - chrono::Duration::days(1),
    ));

    // 320 Circle community-platform notifications (circle.so is §7's named
    // platform domain), spread across the last 29 days — enough to push
    // inflow_per_day past the 10/day threshold on its own, matching §2's
    // "Circle notifications 22%" as the dominant inflow source.
    for i in 0..320u32 {
        let offset_days = (i % 29) as i64;
        rows.push(email_row(
            &format!("circle-{i}"),
            "noreply@circle.so",
            now - chrono::Duration::days(offset_days),
        ));
    }

    // A handful of marketing drips (List-Unsubscribe -> Subscriptions, not
    // Notifications) and receipt-signature local-parts (-> Receipts) — present
    // so the snapshot's other shares are non-zero, not load-bearing for the
    // archetype threshold itself.
    for i in 0..6u32 {
        let mut row = email_row(
            &format!("marketing-{i}"),
            "newsletter@vendor-marketing.example",
            now - chrono::Duration::days(2),
        );
        row.list_unsubscribe = Set(Some("<mailto:unsub@vendor-marketing.example>".to_string()));
        rows.push(row);
    }
    rows.push(email_row(
        "invoice-1",
        "invoice@billing-provider.example",
        now - chrono::Duration::days(3),
    ));

    emails::Entity::insert_many(rows)
        .exec(&conn)
        .await
        .expect("seed emails");

    db
}

fn email_row(id: &str, from_addr: &str, received_at: chrono::NaiveDateTime) -> emails::ActiveModel {
    emails::ActiveModel {
        id: Set(id.to_string()),
        account_id: Set(ACCOUNT_ID.to_string()),
        provider: Set("imap".to_string()),
        subject: Set("fixture".to_string()),
        from_addr: Set(from_addr.to_string()),
        to_addrs: Set("chris@cpventuresgroup.com".to_string()),
        received_at: Set(received_at),
        folder: Set("INBOX".to_string()),
        is_read: Set(Some(false)),
        is_archived: Set(false),
        is_spam: Set(0),
        is_trash: Set(0),
        ..Default::default()
    }
}

#[tokio::test]
async fn business_shaped_fixture_snapshots_as_machine_dominated_with_no_role_mismatch() {
    let db = seeded_db().await;
    let ctx = Arc::new(ToolContext::new(Arc::new(db)));

    let snapshot = get_snapshot(
        ctx,
        GetSnapshotRequest {
            account_id: ACCOUNT_ID.to_string(),
        },
    )
    .await
    .expect("get_snapshot");

    assert_eq!(snapshot["archetype"], "Machine-dominated identity");
    assert_eq!(snapshot["declared_role"], "business");
    assert!(
        snapshot["role_mismatch"].is_null(),
        "declared 'business' matches the measured archetype's expectation, \
         so no mismatch should be flagged: {snapshot}"
    );

    let human_share = snapshot["human_share"].as_f64().expect("human_share");
    assert!(human_share < 0.05, "human_share was {human_share}");

    let inflow = snapshot["inflow_per_day_last_30d"]
        .as_f64()
        .expect("inflow_per_day_last_30d");
    assert!(inflow > 10.0, "inflow_per_day_last_30d was {inflow}");

    assert_eq!(snapshot["total_messages_scanned"], 328);
}
