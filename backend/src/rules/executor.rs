//! Rule execution — applies matched rule actions to emails via direct DB writes.
//!
//! Shared by both `POST /rules/:id/run` (manual) and the inbound sync pipeline
//! so that manual and automatic rule application are always identical.

use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::email::types::EmailMessage;
use crate::rules::rule_processor::evaluate_rule;
use crate::rules::types::{Rule, RuleAction};

/// Apply a single `RuleAction` to an email row in the local DB.
///
/// All writes are idempotent: re-applying the same action to an email that
/// was already affected leaves the DB in the same state.
pub async fn apply_rule_action(
    pool: &SqlitePool,
    email_id: &str,
    action: &RuleAction,
) -> Result<(), sqlx::Error> {
    match action {
        RuleAction::Archive => {
            sqlx::query("UPDATE emails SET folder = 'ARCHIVE' WHERE id = ?")
                .bind(email_id)
                .execute(pool)
                .await?;
        }
        RuleAction::MarkRead => {
            sqlx::query("UPDATE emails SET is_read = 1 WHERE id = ?")
                .bind(email_id)
                .execute(pool)
                .await?;
        }
        RuleAction::MarkImportant => {
            sqlx::query("UPDATE emails SET is_starred = 1 WHERE id = ?")
                .bind(email_id)
                .execute(pool)
                .await?;
        }
        RuleAction::Delete { permanent } => {
            if *permanent {
                sqlx::query("DELETE FROM emails WHERE id = ?")
                    .bind(email_id)
                    .execute(pool)
                    .await?;
            } else {
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "UPDATE emails SET is_trash = 1, deleted_at = ?, folder = 'TRASH' WHERE id = ?",
                )
                .bind(now)
                .bind(email_id)
                .execute(pool)
                .await?;
            }
        }
        RuleAction::AddLabel { label } => {
            let row = sqlx::query("SELECT labels FROM emails WHERE id = ?")
                .bind(email_id)
                .fetch_optional(pool)
                .await?;
            if let Some(row) = row {
                let current: String = row.get("labels");
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
                    sqlx::query("UPDATE emails SET labels = ? WHERE id = ?")
                        .bind(new_labels)
                        .bind(email_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        RuleAction::RemoveLabel { label } => {
            let row = sqlx::query("SELECT labels FROM emails WHERE id = ?")
                .bind(email_id)
                .fetch_optional(pool)
                .await?;
            if let Some(row) = row {
                let current: String = row.get("labels");
                let filtered: Vec<&str> = current
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(label))
                    .collect();
                sqlx::query("UPDATE emails SET labels = ? WHERE id = ?")
                    .bind(filtered.join(","))
                    .bind(email_id)
                    .execute(pool)
                    .await?;
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
pub async fn apply_rules_to_email(pool: &SqlitePool, email: &EmailMessage, rules: &[Rule]) -> u32 {
    let mut applied = 0u32;
    for rule in rules {
        if evaluate_rule(rule, email) {
            for action in &rule.actions {
                if apply_rule_action(pool, &email.id, action).await.is_ok() {
                    applied += 1;
                }
            }
        }
    }
    applied
}
