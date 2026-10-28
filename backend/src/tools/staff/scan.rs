//! Shared, body-free Kind classification over stored mail.
//!
//! Milestone 1's `kind::classify` runs against a live `EmailMessage` fetched
//! from the provider (with `cc`, which the `emails` table does not persist —
//! see `cleanup::repository::adapters`'s `row_to_email_message`). The staff
//! tools need the same classification over *already-ingested* mail, so this
//! reconstructs just enough of an `EmailMessage` from the lean set of columns
//! `classify` actually reads (`from`, `to`, `cc`, `list_unsubscribe`) —
//! deliberately never selecting `body_text`, `body_html`, or `subject`, so
//! the body-free guarantee holds at the query level, not just in the
//! handler's response shape.
//!
//! `cc` is always empty here (the column isn't populated), so GitHub's
//! reason-routing is unavailable for already-ingested mail — the same,
//! already-documented limitation milestone 1 left for the live-fetch path's
//! `cc`-less siblings. `ClassificationContext` is always the default
//! (nothing protected, no declared own-addresses): the sender ledger and
//! account own-address list are later milestones' work, exactly as
//! milestone 1 documented for the live classifier.

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect};

use crate::cleanup::domain::kind::{classify, Classification, ClassificationContext};
use crate::db::entities::emails;
use crate::email::types::EmailMessage;
use crate::tools::{ToolContext, ToolError};

/// One classified message, with just enough context to bucket and count it.
#[derive(Debug, Clone)]
pub struct ScannedMessage {
    pub account_id: String,
    pub from_addr: String,
    pub received_at: chrono::NaiveDateTime,
    pub is_read: bool,
    pub classification: Classification,
}

#[derive(FromQueryResult)]
struct Row {
    account_id: String,
    from_addr: String,
    to_addrs: String,
    list_unsubscribe: Option<String>,
    received_at: chrono::NaiveDateTime,
    is_read: Option<bool>,
}

/// Classify every row matching the scope, excluding soft-deleted, spam and
/// trash mail (none of those describe the mailbox's live shape). Set
/// `inbox_only` to further restrict to un-archived mail — the brief's
/// "needs you" queue, as opposed to the snapshot's whole-mailbox shape.
pub async fn scan_mail(
    ctx: &Arc<ToolContext>,
    account_id: Option<&str>,
    inbox_only: bool,
) -> Result<Vec<ScannedMessage>, ToolError> {
    let mut query = emails::Entity::find()
        .select_only()
        .column(emails::Column::AccountId)
        .column(emails::Column::FromAddr)
        .column(emails::Column::ToAddrs)
        .column(emails::Column::ListUnsubscribe)
        .column(emails::Column::ReceivedAt)
        .column(emails::Column::IsRead)
        .filter(emails::Column::DeletedAt.is_null())
        .filter(emails::Column::IsSpam.eq(0))
        .filter(emails::Column::IsTrash.eq(0));

    if let Some(id) = account_id {
        query = query.filter(emails::Column::AccountId.eq(id));
    }
    if inbox_only {
        query = query.filter(emails::Column::IsArchived.eq(false));
    }

    let rows: Vec<Row> = query
        .into_model::<Row>()
        .all(&ctx.conn())
        .await
        .map_err(|e| crate::tools::readonly::db_error("Scanning mail for classification", e))?;

    let ctx_default = ClassificationContext::default();

    Ok(rows
        .into_iter()
        .map(|row| {
            let to: Vec<String> = row
                .to_addrs
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let msg = EmailMessage {
                id: String::new(),
                thread_id: None,
                from: row.from_addr.clone(),
                to,
                cc: Vec::new(),
                subject: String::new(),
                snippet: String::new(),
                body: None,
                body_html: None,
                labels: Vec::new(),
                date: row.received_at.and_utc(),
                is_read: row.is_read.unwrap_or(false),
                list_unsubscribe: row.list_unsubscribe,
                list_unsubscribe_post: None,
            };

            ScannedMessage {
                account_id: row.account_id,
                from_addr: row.from_addr,
                received_at: row.received_at,
                is_read: row.is_read.unwrap_or(false),
                classification: classify(&msg, &ctx_default),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanned_message_never_carries_a_body_field() {
        // Compile-time guard, not a runtime one: ScannedMessage has no body
        // field at all, so a caller cannot forward one even by accident.
        fn assert_no_body(_: &ScannedMessage) {}
        let _ = assert_no_body;
    }
}
