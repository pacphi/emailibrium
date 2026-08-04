//! Rule execution — applies matched rule actions to emails via direct DB writes.
//!
//! Shared by both `POST /rules/:id/run` (manual) and the inbound sync pipeline
//! so that manual and automatic rule application are always identical.

use chrono::Utc;

use crate::db::{audited_sql, Database};
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
) -> Result<(), sqlx::Error> {
    match action {
        RuleAction::Archive => {
            let sql = db.adapt("UPDATE emails SET folder = 'ARCHIVE' WHERE id = ?");
            match db {
                Database::Sqlite(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
                Database::Postgres(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        RuleAction::MarkRead => {
            // `is_read` is a real BOOLEAN column in both dialects; Postgres rejects an
            // integer literal (`= 1`) there, so bind a `bool` instead of embedding the
            // literal in the SQL text (see ADR-035).
            let sql = db.adapt("UPDATE emails SET is_read = ? WHERE id = ?");
            match db {
                Database::Sqlite(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(true)
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
                Database::Postgres(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(true)
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        RuleAction::MarkImportant => {
            let sql = db.adapt("UPDATE emails SET is_starred = ? WHERE id = ?");
            match db {
                Database::Sqlite(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(true)
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
                Database::Postgres(pool) => {
                    sqlx::query(audited_sql(&sql))
                        .bind(true)
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        RuleAction::Delete { permanent } => {
            if *permanent {
                let sql = db.adapt("DELETE FROM emails WHERE id = ?");
                match db {
                    Database::Sqlite(pool) => {
                        sqlx::query(audited_sql(&sql))
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                    Database::Postgres(pool) => {
                        sqlx::query(audited_sql(&sql))
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                }
            } else {
                let now = Utc::now().to_rfc3339();
                // `is_trash` is INTEGER (not BOOLEAN) in both dialects, so the `1` literal
                // is fine here — unlike `is_read`/`is_starred` above.
                let sql = db.adapt(
                    "UPDATE emails SET is_trash = 1, deleted_at = ?, folder = 'TRASH' WHERE id = ?",
                );
                match db {
                    Database::Sqlite(pool) => {
                        sqlx::query(audited_sql(&sql))
                            .bind(&now)
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                    Database::Postgres(pool) => {
                        sqlx::query(audited_sql(&sql))
                            .bind(&now)
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                }
            }
        }
        RuleAction::AddLabel { label } => {
            let select_sql = db.adapt("SELECT labels FROM emails WHERE id = ?");
            let row: Option<(String,)> = match db {
                Database::Sqlite(pool) => {
                    sqlx::query_as(audited_sql(&select_sql))
                        .bind(email_id)
                        .fetch_optional(pool)
                        .await?
                }
                Database::Postgres(pool) => {
                    sqlx::query_as(audited_sql(&select_sql))
                        .bind(email_id)
                        .fetch_optional(pool)
                        .await?
                }
            };
            if let Some((current,)) = row {
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
                    let update_sql = db.adapt("UPDATE emails SET labels = ? WHERE id = ?");
                    match db {
                        Database::Sqlite(pool) => {
                            sqlx::query(audited_sql(&update_sql))
                                .bind(new_labels)
                                .bind(email_id)
                                .execute(pool)
                                .await?;
                        }
                        Database::Postgres(pool) => {
                            sqlx::query(audited_sql(&update_sql))
                                .bind(new_labels)
                                .bind(email_id)
                                .execute(pool)
                                .await?;
                        }
                    }
                }
            }
        }
        RuleAction::RemoveLabel { label } => {
            let select_sql = db.adapt("SELECT labels FROM emails WHERE id = ?");
            let row: Option<(String,)> = match db {
                Database::Sqlite(pool) => {
                    sqlx::query_as(audited_sql(&select_sql))
                        .bind(email_id)
                        .fetch_optional(pool)
                        .await?
                }
                Database::Postgres(pool) => {
                    sqlx::query_as(audited_sql(&select_sql))
                        .bind(email_id)
                        .fetch_optional(pool)
                        .await?
                }
            };
            if let Some((current,)) = row {
                let filtered: Vec<&str> = current
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(label))
                    .collect();
                let update_sql = db.adapt("UPDATE emails SET labels = ? WHERE id = ?");
                match db {
                    Database::Sqlite(pool) => {
                        sqlx::query(audited_sql(&update_sql))
                            .bind(filtered.join(","))
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                    Database::Postgres(pool) => {
                        sqlx::query(audited_sql(&update_sql))
                            .bind(filtered.join(","))
                            .bind(email_id)
                            .execute(pool)
                            .await?;
                    }
                }
            }
        }
        RuleAction::Forward { .. } => {
            // Forward requires an outbound mail service not wired in this context.
        }
    }
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
