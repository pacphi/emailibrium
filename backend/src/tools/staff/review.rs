//! `review` (read-only) — new senders and unsubscribe candidates
//! (docs/plan/mail-agent-2026-10.md section 8, "Sample Monday review").
//!
//! Reporting only: no approval, no rule graduation, no writes — that's
//! milestone 4's `rule_set_authority`. "New senders" is approximated as a
//! `People`-kind sender whose earliest message in this account's *ingested*
//! history falls in the last 7 days — a real computation against what's
//! actually been scanned, not the sender ledger's first/last-seen (section
//! 8), which doesn't exist yet. It will under-report for a mailbox whose
//! ingested history doesn't reach back far enough to have seen a sender
//! before. "Unsubscribe candidates" reuses the existing subscription
//! detector's own suggestion rather than re-deriving the 90-day/10-message
//! rule section 8 describes — `InsightEngine::detect_subscriptions` already
//! computes it. "Open loops" isn't available yet, same reason as `brief`.
//!
//! `InsightEngine::detect_subscriptions` has no account scope (neither does
//! the existing `list_subscriptions` tool it backs), so unsubscribe
//! candidates are always account-agnostic regardless of this request's
//! `account_id` — a pre-existing limitation, not something new here.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::ReviewRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::sender_key::SenderKey;
use crate::tools::readonly::validate_uuid;
use crate::tools::{ToolContext, ToolError};
use crate::vectors::insights::{InsightEngine, SuggestedAction};

pub async fn review(ctx: Arc<ToolContext>, req: ReviewRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let messages = scan_mail(&ctx, req.account_id.as_deref(), false).await?;

    let now = chrono::Utc::now().naive_utc();
    let seven_days_ago = now - chrono::Duration::days(7);

    let mut first_seen: std::collections::HashMap<SenderKey, (String, chrono::NaiveDateTime)> =
        std::collections::HashMap::new();
    for m in &messages {
        if m.classification.kind != Kind::People {
            continue;
        }
        let Some(key) = SenderKey::from_address(&m.from_addr) else {
            continue;
        };
        first_seen
            .entry(key)
            .and_modify(|(_, seen)| {
                if m.received_at < *seen {
                    *seen = m.received_at;
                }
            })
            .or_insert_with(|| (m.from_addr.clone(), m.received_at));
    }

    let new_senders: Vec<Value> = first_seen
        .into_iter()
        .filter(|(_, (_, first))| *first >= seven_days_ago)
        .map(|(key, (addr, first))| {
            json!({
                "sender_key": key.as_str(),
                "from_addr": addr,
                "first_seen": first.and_utc().to_rfc3339(),
            })
        })
        .collect();

    let engine = InsightEngine::new(ctx.db.clone(), ctx.vectors()?.store.clone());
    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| crate::tools::readonly::db_error("Subscription detection", e))?;

    let unsubscribe_candidates: Vec<Value> = subs
        .iter()
        .filter(|s| s.suggested_action == SuggestedAction::Unsubscribe)
        .map(|s| {
            json!({
                "sender_address": s.sender_address,
                "sender_domain": s.sender_domain,
                "email_count": s.email_count,
                "last_seen": s.last_seen.to_rfc3339(),
            })
        })
        .collect();

    Ok(json!({
        "new_senders": new_senders,
        "new_senders_count": new_senders.len(),
        "unsubscribe_candidates": unsubscribe_candidates,
        "unsubscribe_candidates_count": unsubscribe_candidates.len(),
        "open_loops": [],
        "not_yet_available": [
            "open_loops (needs State/Review tracking, milestone 3+)",
            "rule graduations eligible this week (milestone 4's authority model)",
        ],
    }))
}
