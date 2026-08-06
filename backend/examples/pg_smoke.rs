//! One-off live-PostgreSQL smoke check for the phase-3 SeaORM repo-layer
//! ports (postgres-support DoD). Run against a disposable postgres:16-alpine:
//!
//!   PG_SMOKE_URL=postgres://postgres:smoke@127.0.0.1:59876/emailibrium \
//!     cargo run --example pg_smoke --features vectors
//!
//! Deliberately NOT a checked-in test: env-gated PG test infrastructure
//! (EMAILIBRIUM_TEST_PG_URL) is phase 4's deliverable.

use std::collections::HashMap;
use std::sync::Arc;

use emailibrium::db::{self, Database};
use emailibrium::vectors::store::{InMemoryVectorStore, VectorStoreBackend};

use sea_orm::ActiveValue::Set;
use sea_orm::EntityTrait;

fn check(name: &str, ok: bool) {
    if ok {
        println!("PASS  {name}");
    } else {
        println!("FAIL  {name}");
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    let url = std::env::var("PG_SMOKE_URL").expect("PG_SMOKE_URL");
    assert!(url.starts_with("postgres"), "must be a postgres url");

    // 1. Connect + migrations (proves the PG migration set applies).
    let database = Database::connect(&url).await.expect("connect");
    database.run_migrations().await.expect("migrations");
    check("connect + run_migrations (postgres)", true);

    let db = Arc::new(database);
    let conn = db.sea_orm();

    // 2. Seed: one account, four emails (3 from one sender for HAVING >= 3).
    use emailibrium::db::entities::{connected_accounts, emails};
    connected_accounts::Entity::insert(connected_accounts::ActiveModel {
        id: Set("acct-1".into()),
        provider: Set("gmail".into()),
        email_address: Set("user@example.com".into()),
        status: Set("connected".into()),
        archive_strategy: Set("archive".into()),
        label_prefix: Set("".into()),
        sync_depth: Set("30d".into()),
        sync_frequency: Set(120),
        ..Default::default()
    })
    .exec_without_returning(&conn)
    .await
    .expect("seed account");

    let base = chrono::NaiveDate::from_ymd_opt(2026, 7, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    for (i, sender) in [
        (0, "news@example.com"),
        (1, "news@example.com"),
        (2, "news@example.com"),
        (3, "alice@example.com"),
    ] {
        emails::Entity::insert(emails::ActiveModel {
            id: Set(format!("email-{i}")),
            account_id: Set("acct-1".into()),
            provider: Set("gmail".into()),
            subject: Set(format!("Subject {i}")),
            from_addr: Set(sender.into()),
            to_addrs: Set("me@example.com".into()),
            received_at: Set(base + chrono::Duration::days(i64::from(i) * 7)),
            body_text: Set(Some("please unsubscribe here".into())),
            embedding_status: Set(Some("embedded".into())),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".into()),
            is_archived: Set(false),
            ..Default::default()
        })
        .exec_without_returning(&conn)
        .await
        .expect("seed email");
    }
    check("emails entity inserts (naive TIMESTAMP binds)", true);

    // 3. update_email_state — boolean→i32 widening, TEXT deleted_at.
    let n = db::update_email_state(
        &conn,
        "email-3",
        true,
        false,
        "TRASH",
        Some("2026-07-30T00:00:00+00:00"),
    )
    .await
    .expect("update_email_state");
    check("update_email_state", n == 1);

    // 4. InsightEngine — GROUP BY + repeated COUNT(*) HAVING, MIN/MAX decode,
    //    read-rate CASE WHEN aggregate, derived-table alias.
    let store: Arc<dyn VectorStoreBackend> = Arc::new(InMemoryVectorStore::new());
    let insights = emailibrium::vectors::insights::InsightEngine::new(db.clone(), store.clone());
    let subs = insights
        .detect_subscriptions()
        .await
        .expect("subscriptions");
    check(
        "insights.detect_subscriptions (HAVING COUNT(*) >= 3)",
        subs.len() == 1 && subs[0].sender_address == "news@example.com" && subs[0].has_unsubscribe,
    );
    let recurring = insights
        .analyze_recurring_senders()
        .await
        .expect("recurring");
    check("insights.analyze_recurring_senders", recurring.len() == 1);
    let report = insights.generate_report().await.expect("report");
    check(
        "insights.generate_report (derived-table AS sub)",
        report.total_emails == 4 && report.subscription_count == 1,
    );

    // 5. ReindexOrchestrator — KV upsert + update_many.
    let reindex = emailibrium::vectors::reindex::ReindexOrchestrator::new(db.clone());
    let first = reindex
        .check_model_change("model-a", 384)
        .await
        .expect("first");
    let changed = reindex
        .check_model_change("model-b", 384)
        .await
        .expect("second");
    let stale = reindex.mark_all_stale().await.expect("stale");
    check(
        "reindex model-change upsert + mark_all_stale",
        !first && changed && stale == 4,
    );

    // 6. InteractionTracker — BOOLEAN binds, aggregates, grouped counts.
    let tracker = emailibrium::vectors::interactions::InteractionTracker::new(db.clone());
    let q1 = tracker
        .record_search("quarterly report")
        .await
        .expect("search");
    let _q2 = tracker.record_search("lunch").await.expect("search2");
    tracker
        .record_click(&q1, "email-0", 2)
        .await
        .expect("click");
    tracker
        .record_feedback(&q1, "email-0", "relevant")
        .await
        .expect("fb");
    let ctr = tracker.get_click_through_rate().await.expect("ctr");
    let dist = tracker.get_rank_distribution().await.expect("dist");
    let listed = tracker.get_interactions(10).await.expect("list");
    check(
        "interactions record/click/feedback/ctr/rank",
        (ctr - 0.5).abs() < 1e-6 && dist.get(&2) == Some(&1) && listed.len() == 2,
    );

    // 7. EvaluationEngine — per-backend DDL (IDENTITY arm), naive ts binds.
    let eval = emailibrium::vectors::evaluation::EvaluationEngine::new(db.clone());
    eval.ensure_tables().await.expect("eval tables");
    let vc = |name: &str| emailibrium::vectors::evaluation::VariantConfig {
        name: name.to_string(),
        params: HashMap::new(),
    };
    let test = eval
        .create_test("pg-smoke".into(), vc("a"), vc("b"), 0.5)
        .await
        .expect("create_test");
    for _ in 0..35 {
        eval.record_observation(&test.test_id, "a", 0.5, 0.6, 0.4, 0.5)
            .await
            .expect("obs a");
        eval.record_observation(&test.test_id, "b", 0.7, 0.8, 0.6, 0.7)
            .await
            .expect("obs b");
    }
    let summary = eval.conclude_test(&test.test_id).await.expect("conclude");
    check(
        "evaluation create/record/conclude",
        summary.recommendation == "b",
    );

    // 8. UserLearningStore — composite-PK upsert, DB reload.
    let learning = emailibrium::vectors::user_learning::UserLearningStore::new(
        db.clone(),
        emailibrium::vectors::learning::LearningConfig::default(),
    );
    learning.ensure_table().await.expect("learning table");
    learning
        .on_feedback(
            "user-1",
            emailibrium::vectors::types::EmailCategory::Work,
            &[0.5, 0.5, 0.0],
            &[1.0, 0.0, 0.0],
            &emailibrium::vectors::learning::FeedbackAction::Star,
        )
        .await
        .expect("feedback");
    let users = learning.list_users().await.expect("list_users");
    let fresh = emailibrium::vectors::user_learning::UserLearningStore::new(
        db.clone(),
        emailibrium::vectors::learning::LearningConfig::default(),
    );
    let reloaded = fresh.get_or_create("user-1").await;
    check(
        "user_learning upsert + reload",
        users == vec!["user-1".to_string()] && reloaded.total_feedback == 1,
    );

    // 9. VectorBackupService — BYTEA blobs, OR-REPLACE→DO-UPDATE upsert.
    let backup =
        emailibrium::vectors::backup::VectorBackupService::new(db.clone(), store.clone(), None);
    let doc = emailibrium::vectors::types::VectorDocument {
        id: emailibrium::vectors::types::VectorId::new(),
        email_id: "email-0".into(),
        vector: vec![1.0, 2.0, 3.0],
        metadata: HashMap::new(),
        collection: emailibrium::vectors::types::VectorCollection::EmailText,
        created_at: chrono::Utc::now(),
    };
    let vid = doc.id.to_string();
    backup.backup_vector(&doc).await.expect("backup");
    backup.backup_vector(&doc).await.expect("backup upsert");
    let restored = backup.restore_vector(&vid).await.expect("restore");
    backup.delete_backup(&vid).await.expect("delete backup");
    check(
        "vector backup upsert/restore/delete (BYTEA)",
        restored.map(|d| d.vector) == Some(vec![1.0, 2.0, 3.0]),
    );

    // 10. ClusterEngine — txn replace-set + information_schema existence check.
    let cluster = emailibrium::vectors::clustering::ClusterEngine::with_clusters(
        store.clone(),
        db.clone(),
        emailibrium::vectors::clustering::ClusterConfig::default(),
        vec![emailibrium::vectors::clustering::TopicCluster {
            id: "cluster-1".into(),
            name: "Reports".into(),
            description: "quarterly reports".into(),
            centroid: vec![0.1, 0.2],
            email_ids: vec!["email-0".into()],
            email_count: 1,
            top_terms: vec![],
            representative_email_ids: vec![],
            stability_score: 0.9,
            stability_runs: 1,
            is_pinned: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
    );
    cluster.persist_clusters().await.expect("persist clusters");
    let loaded = cluster
        .load_persisted_clusters()
        .await
        .expect("load clusters");
    check("clustering persist/load (txn + catalog check)", loaded == 1);

    // 11. RemoteWipeService — per-backend DDL + sea-query audit insert.
    let wipe = emailibrium::vectors::remote_wipe::RemoteWipeService::new(db.clone());
    wipe.ensure_table().await.expect("wipe table");
    let result = wipe.wipe_vectors_only().await.expect("wipe vectors only");
    check(
        "remote_wipe ensure_table + vectors-only wipe + audit log",
        result.backups_deleted == 0,
    );

    // 12. CheckpointService — TEXT timestamps, upsert, retention delete.
    use emailibrium::email::checkpoint::{
        CheckpointService, CheckpointState, ProcessingCheckpoint,
    };
    let checkpoints = CheckpointService::new((*db).clone());
    checkpoints
        .save_checkpoint(&ProcessingCheckpoint {
            job_id: "job-1".into(),
            provider: "gmail".into(),
            account_id: "acct-1".into(),
            last_processed_id: Some("email-1".into()),
            total_count: Some(4),
            processed_count: 2,
            state: CheckpointState::Running,
            error_message: None,
            updated_at: chrono::Utc::now(),
        })
        .await
        .expect("save checkpoint");
    let got = checkpoints
        .get_checkpoint("job-1")
        .await
        .expect("get checkpoint");
    check(
        "processing checkpoint upsert + read",
        got.map(|c| c.processed_count) == Some(2),
    );

    // 13. OfflineQueue — enqueue/dequeue over sync_queue.
    use emailibrium::email::offline_queue::{OfflineQueue, OperationType, QueuedOperation};
    let queue = OfflineQueue::new((*db).clone());
    let op = QueuedOperation::new(
        "acct-1".into(),
        OperationType::Archive,
        "email-0".into(),
        None,
    );
    queue.enqueue(&op).await.expect("enqueue");
    let batch = queue.dequeue_batch(10).await.expect("dequeue");
    check(
        "offline_queue enqueue/dequeue",
        batch.len() == 1 && batch[0].target_id == "email-0",
    );

    println!("ALL PASS — phase-3 repo-layer ports verified against live postgres:16-alpine");
}
