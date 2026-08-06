//! Rule execution — applies matched rule actions to emails via direct DB writes.
//!
//! Shared by both `POST /rules/:id/run` (manual) and the inbound sync pipeline
//! so that manual and automatic rule application are always identical.
//!
//! Single-code-path SeaORM (ADR-036): the `emails` entity owns per-backend
//! encode/decode, so every action below is one statement that runs unchanged
//! against SQLite and PostgreSQL. The eight backend match-arm pairs this file
//! used to carry are gone, along with the bind-type asymmetry that motivated
//! them — `is_read`/`is_starred` are BOOLEAN while `is_trash` is INTEGER, a
//! split the entity now mirrors instead of each call site (ADR-035).

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect};

use crate::db::entities::emails;
use crate::db::Database;
use crate::email::types::EmailMessage;
use crate::rules::rule_processor::evaluate_rule;
use crate::rules::types::{Rule, RuleAction};

/// Apply a single `RuleAction` to an email row in the local DB.
///
/// All writes are idempotent: re-applying the same action to an email that
/// was already affected leaves the DB in the same state.
pub async fn apply_rule_action(
    db: &Database,
    email_id: &str,
    action: &RuleAction,
) -> Result<(), DbErr> {
    let conn = db.sea_orm();
    match action {
        RuleAction::Archive => {
            emails::Entity::update_many()
                .col_expr(emails::Column::Folder, Expr::value("ARCHIVE"))
                .filter(emails::Column::Id.eq(email_id))
                .exec(&conn)
                .await?;
        }
        RuleAction::MarkRead => {
            emails::Entity::update_many()
                .col_expr(emails::Column::IsRead, Expr::value(true))
                .filter(emails::Column::Id.eq(email_id))
                .exec(&conn)
                .await?;
        }
        RuleAction::MarkImportant => {
            emails::Entity::update_many()
                .col_expr(emails::Column::IsStarred, Expr::value(true))
                .filter(emails::Column::Id.eq(email_id))
                .exec(&conn)
                .await?;
        }
        RuleAction::Delete { permanent } => {
            if *permanent {
                emails::Entity::delete_many()
                    .filter(emails::Column::Id.eq(email_id))
                    .exec(&conn)
                    .await?;
            } else {
                // `is_trash` is INTEGER and `deleted_at` is TEXT holding an
                // RFC3339 string — both by migration 016's design, and both
                // compared as such elsewhere, so the entity mirrors the DDL
                // rather than normalizing the types.
                emails::Entity::update_many()
                    .col_expr(emails::Column::IsTrash, Expr::value(1_i32))
                    .col_expr(
                        emails::Column::DeletedAt,
                        Expr::value(Utc::now().to_rfc3339()),
                    )
                    .col_expr(emails::Column::Folder, Expr::value("TRASH"))
                    .filter(emails::Column::Id.eq(email_id))
                    .exec(&conn)
                    .await?;
            }
        }
        // The two label actions stay read-modify-writes with NO transaction, as
        // before the port. That leaves a lost-update window between the read and
        // the write when two rules touch one email's labels concurrently —
        // pre-existing, recorded as parking-lot `pl-executor-label-rmw`, and
        // deliberately ported statement-for-statement rather than fixed here.
        RuleAction::AddLabel { label } => {
            if let Some(current) = current_labels(&conn, email_id).await? {
                let already_present = current
                    .split(',')
                    .map(|s| s.trim())
                    .any(|l| l.eq_ignore_ascii_case(label));
                if !already_present {
                    let new_labels = if current.trim().is_empty() {
                        label.clone()
                    } else {
                        format!("{},{}", current.trim_end_matches(','), label)
                    };
                    set_labels(&conn, email_id, new_labels).await?;
                }
            }
        }
        RuleAction::RemoveLabel { label } => {
            if let Some(current) = current_labels(&conn, email_id).await? {
                let filtered: Vec<&str> = current
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(label))
                    .collect();
                set_labels(&conn, email_id, filtered.join(",")).await?;
            }
        }
        RuleAction::Forward { .. } => {
            // Forward requires an outbound mail service not wired in this context.
        }
    }
    Ok(())
}

/// Read one email's comma-separated `labels`; `None` means no such row.
///
/// The column is nullable, and a NULL reads as `""` — not as an error and not
/// as a missing row. That reproduces what the pre-port SQLite decode did (see
/// the `label_actions_treat_a_null_labels_column_as_empty` pin), and the one
/// code path now behaves that way on both backends. The pre-port PostgreSQL arm
/// decoded into a non-optional `String` and so would have failed on a NULL, but
/// that arm was unreachable — `Database::pool()` panicked before any PostgreSQL
/// query ran — so there is no live behavior being changed here, only a dead
/// branch being dropped in favor of the reading users actually got.
async fn current_labels(
    conn: &DatabaseConnection,
    email_id: &str,
) -> Result<Option<String>, DbErr> {
    Ok(emails::Entity::find_by_id(email_id)
        .select_only()
        .column(emails::Column::Labels)
        .into_tuple::<Option<String>>()
        .one(conn)
        .await?
        .map(Option::unwrap_or_default))
}

async fn set_labels(
    conn: &DatabaseConnection,
    email_id: &str,
    labels: String,
) -> Result<(), DbErr> {
    emails::Entity::update_many()
        .col_expr(emails::Column::Labels, Expr::value(labels))
        .filter(emails::Column::Id.eq(email_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// Evaluate all `rules` against `email` and apply every matching action.
///
/// Returns the total number of DB actions applied.  Safe to call on every
/// upserted email — all writes are idempotent so re-evaluation is harmless.
pub async fn apply_rules_to_email(db: &Database, email: &EmailMessage, rules: &[Rule]) -> u32 {
    let mut applied = 0u32;
    for rule in rules {
        if evaluate_rule(rule, email) {
            for action in &rule.actions {
                if apply_rule_action(db, &email.id, action).await.is_ok() {
                    applied += 1;
                }
            }
        }
    }
    applied
}

// ---------------------------------------------------------------------------
// Behavior pins for the SeaORM port (ADR-036). This module mutates email state
// — including a permanent DELETE — and had no coverage before the port, so
// these tests were written and confirmed green against the hand-rolled
// dual-backend implementation FIRST. They must stay green unchanged across the
// re-port: they ARE the equivalence contract.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::entities::emails;
    use crate::rules::types::{EmailField, MatchOperator, RuleCondition};
    use chrono::{DateTime, Utc};
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, EntityTrait,
    };

    /// In-memory SQLite carrying every migration the `emails` entity spans: 001
    /// creates the table, 016 adds `deleted_at`/`is_spam`/`is_trash`/`folder`,
    /// and 018/021/027 add the remaining columns the entity declares (a
    /// full-model read fails without them).
    async fn fresh_db() -> Database {
        let db = crate::db::test_sqlite_database().await;
        let conn = db.sea_orm();
        for raw in [
            include_str!("../../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../../migrations/sqlite/016_soft_delete_trash_spam.sql"),
            include_str!("../../migrations/sqlite/018_unsubscribe_headers.sql"),
            include_str!("../../migrations/sqlite/021_thread_key.sql"),
            include_str!("../../migrations/sqlite/027_is_archived.sql"),
        ] {
            // Strip line comments before splitting on ';'.
            let cleaned: String = raw
                .lines()
                .map(|l| {
                    if let Some(idx) = l.find("--") {
                        &l[..idx]
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in cleaned.split(';') {
                let s = stmt.trim();
                if !s.is_empty() {
                    conn.execute_unprepared(s).await.expect("migrate");
                }
            }
        }
        db
    }

    /// Seed one INBOX email. `labels` is `None` to store SQL NULL (the `labels`
    /// column is nullable) and `Some("")` for the ordinary empty-string case.
    async fn seed_email(conn: &DatabaseConnection, id: &str, labels: Option<&str>) {
        emails::ActiveModel {
            id: Set(id.to_owned()),
            account_id: Set("acct-1".to_owned()),
            provider: Set("gmail".to_owned()),
            subject: Set(format!("subject {id}")),
            from_addr: Set("sender@example.com".to_owned()),
            to_addrs: Set("me@example.com".to_owned()),
            received_at: Set(Utc::now().naive_utc()),
            labels: Set(labels.map(str::to_owned)),
            is_read: Set(Some(false)),
            is_starred: Set(Some(false)),
            is_spam: Set(0),
            is_trash: Set(0),
            folder: Set("INBOX".to_owned()),
            is_archived: Set(false),
            ..Default::default()
        }
        .insert(conn)
        .await
        .expect("seed email");
    }

    async fn load(conn: &DatabaseConnection, id: &str) -> Option<emails::Model> {
        emails::Entity::find_by_id(id)
            .one(conn)
            .await
            .expect("load")
    }

    async fn labels_of(conn: &DatabaseConnection, id: &str) -> Option<String> {
        load(conn, id).await.expect("row present").labels
    }

    #[tokio::test]
    async fn archive_sets_folder_on_the_target_email_only() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("")).await;
        seed_email(&conn, "e2", Some("")).await;

        apply_rule_action(&db, "e1", &RuleAction::Archive)
            .await
            .expect("archive");

        assert_eq!(load(&conn, "e1").await.expect("e1").folder, "ARCHIVE");
        assert_eq!(load(&conn, "e2").await.expect("e2").folder, "INBOX");
    }

    #[tokio::test]
    async fn mark_read_and_mark_important_set_their_own_flag_only() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("")).await;
        seed_email(&conn, "e2", Some("")).await;

        apply_rule_action(&db, "e1", &RuleAction::MarkRead)
            .await
            .expect("mark read");
        let e1 = load(&conn, "e1").await.expect("e1");
        assert_eq!(e1.is_read, Some(true));
        assert_eq!(e1.is_starred, Some(false), "MarkRead must not star");

        apply_rule_action(&db, "e1", &RuleAction::MarkImportant)
            .await
            .expect("mark important");
        let e1 = load(&conn, "e1").await.expect("e1");
        assert_eq!(e1.is_starred, Some(true));

        let e2 = load(&conn, "e2").await.expect("e2");
        assert_eq!(e2.is_read, Some(false));
        assert_eq!(e2.is_starred, Some(false));
    }

    #[tokio::test]
    async fn permanent_delete_removes_only_the_target_row() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("")).await;
        seed_email(&conn, "e2", Some("")).await;

        apply_rule_action(&db, "e1", &RuleAction::Delete { permanent: true })
            .await
            .expect("permanent delete");

        assert!(load(&conn, "e1").await.is_none(), "row must be gone");
        assert!(load(&conn, "e2").await.is_some(), "bystander must survive");
    }

    #[tokio::test]
    async fn soft_delete_trashes_the_row_and_stamps_deleted_at() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("")).await;
        seed_email(&conn, "e2", Some("")).await;

        let before = Utc::now() - chrono::Duration::seconds(1);
        apply_rule_action(&db, "e1", &RuleAction::Delete { permanent: false })
            .await
            .expect("soft delete");

        let e1 = load(&conn, "e1")
            .await
            .expect("row must survive a soft delete");
        // `is_trash` is INTEGER (not BOOLEAN) — migration 016.
        assert_eq!(e1.is_trash, 1);
        assert_eq!(e1.folder, "TRASH");
        // `deleted_at` is TEXT holding an RFC3339 timestamp — the format is
        // load-bearing for the TEXT comparisons other queries do on it.
        let stamped = DateTime::parse_from_rfc3339(
            e1.deleted_at
                .as_deref()
                .expect("deleted_at must be stamped"),
        )
        .expect("deleted_at must be RFC3339");
        assert!(stamped.with_timezone(&Utc) >= before);

        let e2 = load(&conn, "e2").await.expect("e2");
        assert_eq!(e2.is_trash, 0);
        assert_eq!(e2.folder, "INBOX");
        assert!(e2.deleted_at.is_none());
    }

    #[tokio::test]
    async fn add_label_appends_and_dedupes_case_insensitively() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "empty", Some("")).await;
        seed_email(&conn, "existing", Some("Work")).await;
        seed_email(&conn, "trailing", Some("Work,")).await;

        let add = |label: &str| RuleAction::AddLabel {
            label: label.to_owned(),
        };

        // Empty label set: the new label becomes the whole value (no leading comma).
        apply_rule_action(&db, "empty", &add("urgent"))
            .await
            .expect("add to empty");
        assert_eq!(labels_of(&conn, "empty").await.as_deref(), Some("urgent"));

        apply_rule_action(&db, "existing", &add("urgent"))
            .await
            .expect("add");
        assert_eq!(
            labels_of(&conn, "existing").await.as_deref(),
            Some("Work,urgent")
        );

        // Idempotent, case-insensitive early-out: re-adding under a different
        // case leaves the stored value byte-identical.
        apply_rule_action(&db, "existing", &add("URGENT"))
            .await
            .expect("re-add");
        assert_eq!(
            labels_of(&conn, "existing").await.as_deref(),
            Some("Work,urgent")
        );

        // A trailing comma is trimmed before appending (no empty label appears).
        apply_rule_action(&db, "trailing", &add("urgent"))
            .await
            .expect("add to trailing-comma value");
        assert_eq!(
            labels_of(&conn, "trailing").await.as_deref(),
            Some("Work,urgent")
        );
    }

    #[tokio::test]
    async fn remove_label_filters_case_insensitively_and_normalizes() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("Work, urgent ,,done")).await;
        seed_email(&conn, "e2", Some("keep")).await;

        apply_rule_action(
            &db,
            "e1",
            &RuleAction::RemoveLabel {
                label: "WORK".to_owned(),
            },
        )
        .await
        .expect("remove");
        // Removal also normalizes: every remaining label is trimmed and empty
        // segments are dropped.
        assert_eq!(labels_of(&conn, "e1").await.as_deref(), Some("urgent,done"));

        // Removing a label that is not present still rewrites the (already
        // normalized) value rather than short-circuiting — unlike AddLabel,
        // RemoveLabel has no early-out.
        apply_rule_action(
            &db,
            "e1",
            &RuleAction::RemoveLabel {
                label: "absent".to_owned(),
            },
        )
        .await
        .expect("remove absent");
        assert_eq!(labels_of(&conn, "e1").await.as_deref(), Some("urgent,done"));

        assert_eq!(labels_of(&conn, "e2").await.as_deref(), Some("keep"));
    }

    #[tokio::test]
    async fn label_actions_on_a_missing_email_are_silent_noops() {
        let db = fresh_db().await;
        let conn = db.sea_orm();

        apply_rule_action(
            &db,
            "ghost",
            &RuleAction::AddLabel {
                label: "x".to_owned(),
            },
        )
        .await
        .expect("add on a missing email must not error");
        apply_rule_action(
            &db,
            "ghost",
            &RuleAction::RemoveLabel {
                label: "x".to_owned(),
            },
        )
        .await
        .expect("remove on a missing email must not error");

        assert!(
            load(&conn, "ghost").await.is_none(),
            "no row may be created"
        );
    }

    #[tokio::test]
    async fn label_actions_treat_a_null_labels_column_as_empty() {
        // `labels` is nullable, and the pre-port SQLite path decoded a NULL into
        // an empty `String` rather than failing: AddLabel seeds the column and
        // RemoveLabel normalizes it to ''. Pinned because the port has to
        // reproduce this deliberately — SeaORM's decode is strict about NULLs,
        // so `current_labels` reads an `Option<String>` and maps NULL to "" on
        // purpose. The pre-port PostgreSQL arm would have errored on a NULL, but
        // it was unreachable dead code, so the lenient SQLite reading pinned
        // here is the only behavior users ever had — see `current_labels`.
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "nul", None).await;
        seed_email(&conn, "nul2", None).await;
        assert!(
            labels_of(&conn, "nul").await.is_none(),
            "the seed must store SQL NULL, not ''"
        );

        apply_rule_action(
            &db,
            "nul",
            &RuleAction::AddLabel {
                label: "x".to_owned(),
            },
        )
        .await
        .expect("add on NULL labels");
        assert_eq!(labels_of(&conn, "nul").await.as_deref(), Some("x"));

        apply_rule_action(
            &db,
            "nul2",
            &RuleAction::RemoveLabel {
                label: "x".to_owned(),
            },
        )
        .await
        .expect("remove on NULL labels");
        assert_eq!(labels_of(&conn, "nul2").await.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn forward_touches_no_email_state() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("keep")).await;
        let before = load(&conn, "e1").await.expect("e1");

        apply_rule_action(
            &db,
            "e1",
            &RuleAction::Forward {
                to: "team@example.com".to_owned(),
            },
        )
        .await
        .expect("forward is a no-op here");

        assert_eq!(load(&conn, "e1").await.expect("e1"), before);
    }

    fn sample_message(id: &str, subject: &str) -> EmailMessage {
        EmailMessage {
            id: id.to_owned(),
            thread_id: None,
            from: "sender@example.com".to_owned(),
            to: vec!["me@example.com".to_owned()],
            subject: subject.to_owned(),
            snippet: String::new(),
            body: None,
            body_html: None,
            labels: vec![],
            date: Utc::now(),
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        }
    }

    fn sample_rule(name: &str, needle: &str, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: format!("rule-{name}"),
            name: name.to_owned(),
            description: String::new(),
            conditions: vec![RuleCondition::FieldMatch {
                field: EmailField::Subject,
                operator: MatchOperator::Contains,
                value: needle.to_owned(),
            }],
            actions,
            priority: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn apply_rules_to_email_counts_only_matching_rules_actions() {
        let db = fresh_db().await;
        let conn = db.sea_orm();
        seed_email(&conn, "e1", Some("")).await;

        let matching = sample_rule(
            "match",
            "subject e1",
            vec![RuleAction::MarkRead, RuleAction::Archive],
        );
        let missing = sample_rule(
            "miss",
            "nothing-matches-this",
            vec![RuleAction::MarkImportant],
        );

        let applied = apply_rules_to_email(
            &db,
            &sample_message("e1", "subject e1"),
            &[matching, missing],
        )
        .await;

        assert_eq!(applied, 2, "only the matching rule's two actions run");
        let e1 = load(&conn, "e1").await.expect("e1");
        assert_eq!(e1.is_read, Some(true));
        assert_eq!(e1.folder, "ARCHIVE");
        assert_eq!(e1.is_starred, Some(false), "the unmatched rule never ran");
    }
}
