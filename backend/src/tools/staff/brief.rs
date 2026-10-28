//! `brief` — the attention queue (docs/plan/mail-agent-2026-10.md section 8,
//! "Attention brief").
//!
//! **SLA clocks and the boomerang (milestone 5).** An open loop is one or
//! more unread `People`-kind inbox messages sharing a `thread_key`; its
//! clock starts at the *earliest* unread message in the thread (when the
//! loop first opened) and runs against [`sla::sla_days`]. A loop whose
//! clock has passed its SLA is flagged `"boomerang": true` and sorted to
//! the top, exactly as docs/plan/mail-agent-2026-10.md section 8 describes
//! ("once past its SLA it re-enters the inbox at the next morning brief").
//!
//! **What "topic" honestly means here.** The plan's SLA table is keyed by
//! its own small taxonomy (Clients 2 days, Household 3, Career 3,
//! Community 7). Nothing in the schema carries that taxonomy per message
//! today: `emails.category` is the pre-existing generic `EmailCategory`
//! classifier (Work/Finance/Shopping/...), and `topic_clusters.name` is
//! auto-generated from TF-IDF terms over subjects (e.g. "Cluster 1"), not
//! a user-chosen Clients/Household/Career/Community label. So every loop
//! here passes `topic: None` into `sla::sla_days`, landing on the 7-day
//! Community/default SLA for everything — a real computation against the
//! one topic value this milestone can honestly supply, not a guess at
//! which named topic a thread belongs to. This is the gap to close once a
//! per-message Topic exists.
//!
//! **What this milestone still can't build.** No protected-sender alerts
//! (the protected list doesn't exist yet — see
//! `cleanup::domain::kind::ClassificationContext`'s own doc) — reported as
//! explicitly absent rather than faked, per this project's standing rule
//! against fabricating data to look more complete than the schema
//! supports.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::params::BriefRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::sla::{loop_status, sla_days, LoopStatus};
use crate::tools::readonly::validate_uuid;
use crate::tools::{ToolContext, ToolError};

struct OpenLoopGroup {
    account_id: String,
    from_addr: String,
    since: chrono::NaiveDateTime,
    is_broadcast: bool,
}

pub async fn brief(ctx: Arc<ToolContext>, req: BriefRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let messages = scan_mail(&ctx, req.account_id.as_deref(), true).await?;

    let mut filed_counts: HashMap<&'static str, usize> = HashMap::new();
    let mut groups: HashMap<(String, String), OpenLoopGroup> = HashMap::new();

    for m in &messages {
        *filed_counts
            .entry(m.classification.kind.label())
            .or_insert(0) += 1;
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
            .or_insert_with(|| OpenLoopGroup {
                account_id: m.account_id.clone(),
                from_addr: m.from_addr.clone(),
                since: m.received_at,
                is_broadcast: m.classification.is_broadcast,
            });
    }

    let now = chrono::Utc::now();
    let sla = sla_days(None);

    let mut boomerang = Vec::new();
    let mut within_sla = Vec::new();
    for g in groups.into_values() {
        let since_utc = g.since.and_utc();
        let status = loop_status(since_utc, now, sla);
        let item = json!({
            "account_id": g.account_id,
            "from_addr": g.from_addr,
            "since": since_utc.to_rfc3339(),
            "is_broadcast": g.is_broadcast,
            "topic": Value::Null,
            "sla_days": sla,
            "boomerang": status == LoopStatus::PastSla,
        });
        match status {
            LoopStatus::PastSla => boomerang.push((g.since, item)),
            LoopStatus::WithinSla => within_sla.push((g.since, item)),
        }
    }
    boomerang.sort_by_key(|(since, _)| *since);
    within_sla.sort_by_key(|(since, _)| *since);

    let boomerang_count = boomerang.len();
    let needs_you: Vec<Value> = boomerang
        .into_iter()
        .chain(within_sla)
        .map(|(_, item)| item)
        .collect();

    let response = json!({
        "needs_you": needs_you,
        "needs_you_count": needs_you.len(),
        "boomerang_count": boomerang_count,
        "filed_counts": filed_counts,
        "protected_alerts": [],
        "not_yet_available": [
            "protected_alerts (needs the protected-sender list, not built yet)",
            "per-message topic for the SLA table (needs a Clients/Household/Career/Community taxonomy per message; every loop falls back to the 7-day default today — see module doc)",
        ],
    });

    super::saved_reports::save_report(&ctx, "brief", req.account_id.as_deref(), &response).await?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn a_loop_open_longer_than_its_sla_is_flagged_boomerang() {
        let since = chrono::Utc::now() - Duration::days(10);
        let now = chrono::Utc::now();
        assert_eq!(loop_status(since, now, sla_days(None)), LoopStatus::PastSla);
    }

    #[test]
    fn a_fresh_loop_is_not_boomeranged() {
        let since = chrono::Utc::now();
        let now = chrono::Utc::now();
        assert_eq!(
            loop_status(since, now, sla_days(None)),
            LoopStatus::WithinSla
        );
    }
}
