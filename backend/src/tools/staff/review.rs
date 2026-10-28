//! `review` (read-only) — the Monday review (docs/plan/mail-agent-2026-10.md
//! section 8, "Sample Monday review").
//!
//! Reporting only: no approval, no rule graduation, no writes.
//!
//! **Multi-account merge (milestone 5).** When `account_id` is omitted,
//! every connected account's data is merged into one report and every item
//! carries the `account_id` (and, where a `connected_accounts` row exists
//! for it, `account_email`) it came from — per the plan's own header for
//! this tool ("`/mail review`, all accounts merged and tagged").
//! `InsightEngine::detect_subscriptions` (unsubscribe candidates) has no
//! account scope of its own, so those entries carry `account_id: null`
//! rather than a fabricated one — a pre-existing limitation, not something
//! new here.
//!
//! **"New senders"** is approximated as a `People`-kind sender whose
//! earliest message in this account's *ingested* history falls in the last
//! 7 days — a real computation against what's actually been scanned, not
//! the sender ledger's first/last-seen (section 8), which doesn't exist
//! yet. It will under-report for a mailbox whose ingested history doesn't
//! reach back far enough to have seen a sender before.
//!
//! **"Open loops"** (milestone 5) reuses the same thread-grouping
//! `staff::brief` uses — unread `People`-kind inbox messages sharing a
//! `thread_key`, clocked from the earliest unread message — filtered to
//! the plan's own Monday-review threshold ("last message theirs, > 7
//! days": section 8's sample review), not the daily brief's per-topic SLA.
//!
//! **"Strays"** (milestone 5) are `Kind::Other` inbox messages (unresolved
//! by any header rule) grouped by sender key, where the same sender
//! reached the inbox 3 or more times — the plan's own definition (section
//! 8's sample review: "reached an inbox unclassified 3+ times").
//!
//! **"Rules eligible to graduate to trusted"** (milestone 5) reuses
//! `rules::authority`'s own graduation constant rather than reimplementing
//! it: a `Suggest`-level rule one clean approval away from
//! `CLEAN_STREAK_TO_GRADUATE` is surfaced here; the graduation itself still
//! only happens through `rule_set_authority` (not this read-only tool).
//!
//! **"Noise trend with the three biggest levers"** (milestone 5) ranks
//! sender keys by message volume over the last 7 days against the 7 days
//! before that, over the whole mailbox (not just the inbox, since
//! Notifications/Subscriptions mail is usually filed, not left in the
//! inbox, and the trend is about total inflow) — the three highest-volume
//! senders this period, each with its volume delta from the prior period.
//!
//! **Enrolled-program check and GitHub-repos-gone-quiet** (section 8's
//! sample review, items 5-6) need data this milestone's schema doesn't
//! carry (an enrolled-programs list; a repo-participation ledger) — scoped
//! out rather than faked; see `not_yet_available`.

use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::EntityTrait;
use serde_json::{json, Value};

use super::noise_trend::sender_volume_trend;
use super::params::ReviewRequest;
use super::scan::{scan_mail, ScannedMessage};
use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::sender_key::SenderKey;
use crate::db::entities::connected_accounts;
use crate::rules::authority::{AuthorityLevel, CLEAN_STREAK_TO_GRADUATE};
use crate::tools::readonly::validate_uuid;
use crate::tools::{ToolContext, ToolError};
use crate::vectors::insights::{InsightEngine, SuggestedAction};

/// Threshold for an open loop to appear in the *weekly* review (section 8's
/// sample review: "last message theirs, > 7 days") — distinct from the
/// daily brief's per-topic SLA clock.
const OPEN_LOOP_REVIEW_THRESHOLD_DAYS: i64 = 7;
/// "Strays": the same unclassified sender reaching the inbox this many
/// times or more (section 8's sample review).
const STRAY_THRESHOLD: usize = 3;
/// Noise-trend window, each side (this week vs the week before).
const NOISE_TREND_WINDOW_DAYS: i64 = 7;

async fn account_email_map(ctx: &Arc<ToolContext>) -> Result<HashMap<String, String>, ToolError> {
    let accounts = connected_accounts::Entity::find()
        .all(&ctx.conn())
        .await
        .map_err(|e| crate::tools::readonly::db_error("Loading connected accounts", e))?;
    Ok(accounts
        .into_iter()
        .map(|a| (a.id, a.email_address))
        .collect())
}

fn new_senders_report(messages: &[ScannedMessage]) -> Vec<Value> {
    let now = chrono::Utc::now().naive_utc();
    let seven_days_ago = now - chrono::Duration::days(7);

    let mut first_seen: HashMap<(String, SenderKey), (String, chrono::NaiveDateTime)> =
        HashMap::new();
    for m in messages {
        if m.classification.kind != Kind::People {
            continue;
        }
        let Some(key) = SenderKey::from_address(&m.from_addr) else {
            continue;
        };
        first_seen
            .entry((m.account_id.clone(), key))
            .and_modify(|(_, seen)| {
                if m.received_at < *seen {
                    *seen = m.received_at;
                }
            })
            .or_insert_with(|| (m.from_addr.clone(), m.received_at));
    }

    first_seen
        .into_iter()
        .filter(|(_, (_, first))| *first >= seven_days_ago)
        .map(|((account_id, key), (addr, first))| {
            json!({
                "account_id": account_id,
                "sender_key": key.as_str(),
                "from_addr": addr,
                "first_seen": first.and_utc().to_rfc3339(),
            })
        })
        .collect()
}

struct LoopGroup {
    account_id: String,
    from_addr: String,
    since: chrono::NaiveDateTime,
}

fn open_loops_report(inbox_messages: &[ScannedMessage]) -> Vec<Value> {
    let mut groups: HashMap<(String, String), LoopGroup> = HashMap::new();
    for m in inbox_messages {
        if m.classification.kind != Kind::People || m.is_read {
            continue;
        }
        groups
            .entry((m.account_id.clone(), m.thread_key.clone()))
            .and_modify(|g| {
                if m.received_at < g.since {
                    g.since = m.received_at;
                    g.from_addr = m.from_addr.clone();
                }
            })
            .or_insert_with(|| LoopGroup {
                account_id: m.account_id.clone(),
                from_addr: m.from_addr.clone(),
                since: m.received_at,
            });
    }

    let now = chrono::Utc::now().naive_utc();
    let threshold = now - chrono::Duration::days(OPEN_LOOP_REVIEW_THRESHOLD_DAYS);
    groups
        .into_values()
        .filter(|g| g.since <= threshold)
        .map(|g| {
            let days_open = (now - g.since).num_days();
            json!({
                "account_id": g.account_id,
                "from_addr": g.from_addr,
                "since": g.since.and_utc().to_rfc3339(),
                "days_open": days_open,
            })
        })
        .collect()
}

fn strays_report(inbox_messages: &[ScannedMessage]) -> Vec<Value> {
    let mut counts: HashMap<(String, SenderKey), (String, usize)> = HashMap::new();
    for m in inbox_messages {
        if m.classification.kind != Kind::Other {
            continue;
        }
        let Some(key) = SenderKey::from_address(&m.from_addr) else {
            continue;
        };
        let entry = counts
            .entry((m.account_id.clone(), key))
            .or_insert_with(|| (m.from_addr.clone(), 0));
        entry.1 += 1;
    }

    counts
        .into_iter()
        .filter(|(_, (_, count))| *count >= STRAY_THRESHOLD)
        .map(|((account_id, key), (addr, count))| {
            json!({
                "account_id": account_id,
                "sender_key": key.as_str(),
                "from_addr": addr,
                "unclassified_count": count,
            })
        })
        .collect()
}

async fn graduation_candidates(ctx: &Arc<ToolContext>) -> Result<Vec<Value>, ToolError> {
    use crate::db::entities::rules;

    let rows = rules::Entity::find()
        .all(&ctx.conn())
        .await
        .map_err(|e| crate::tools::readonly::db_error("Loading rules", e))?;

    Ok(rows
        .into_iter()
        .filter(|r| {
            let level = AuthorityLevel::parse(r.authority_level.as_deref().unwrap_or(""));
            let streak = r.clean_streak.unwrap_or(0).max(0) as u32;
            level == AuthorityLevel::Suggest && streak + 1 == CLEAN_STREAK_TO_GRADUATE
        })
        .map(|r| {
            json!({
                "rule_id": r.id,
                "name": r.name,
                "clean_streak": r.clean_streak.unwrap_or(0),
                "clean_approvals_needed": CLEAN_STREAK_TO_GRADUATE,
            })
        })
        .collect())
}

pub async fn review(ctx: Arc<ToolContext>, req: ReviewRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let messages = scan_mail(&ctx, req.account_id.as_deref(), false).await?;
    let inbox_messages = scan_mail(&ctx, req.account_id.as_deref(), true).await?;
    let account_emails = account_email_map(&ctx).await?;

    let new_senders = new_senders_report(&messages);
    let open_loops = open_loops_report(&inbox_messages);
    let strays = strays_report(&inbox_messages);
    let noise_trend = sender_volume_trend(&messages, NOISE_TREND_WINDOW_DAYS, 3);
    let graduation_candidates = graduation_candidates(&ctx).await?;

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
                "account_id": Value::Null,
                "sender_address": s.sender_address,
                "sender_domain": s.sender_domain,
                "email_count": s.email_count,
                "last_seen": s.last_seen.to_rfc3339(),
            })
        })
        .collect();

    let tag_account_email = |v: Value| -> Value {
        let mut v = v;
        if let Some(obj) = v.as_object_mut() {
            if let Some(account_id) = obj.get("account_id").and_then(|a| a.as_str()) {
                if let Some(email) = account_emails.get(account_id) {
                    obj.insert("account_email".to_string(), json!(email));
                }
            }
        }
        v
    };

    let new_senders: Vec<Value> = new_senders.into_iter().map(tag_account_email).collect();
    let open_loops: Vec<Value> = open_loops.into_iter().map(tag_account_email).collect();
    let strays: Vec<Value> = strays.into_iter().map(tag_account_email).collect();

    let response = json!({
        "new_senders": new_senders,
        "new_senders_count": new_senders.len(),
        "unsubscribe_candidates": unsubscribe_candidates,
        "unsubscribe_candidates_count": unsubscribe_candidates.len(),
        "open_loops": open_loops,
        "open_loops_count": open_loops.len(),
        "strays": strays,
        "strays_count": strays.len(),
        "noise_trend": noise_trend,
        "graduation_candidates": graduation_candidates,
        "graduation_candidates_count": graduation_candidates.len(),
        "not_yet_available": [
            "enrolled-program check (needs an enrolled-programs list, not built yet)",
            "GitHub repos gone quiet (needs a repo-participation ledger, not built yet)",
        ],
    });

    super::saved_reports::save_report(&ctx, "review", req.account_id.as_deref(), &response).await?;

    Ok(response)
}
