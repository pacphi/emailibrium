//! Screener (docs/plan/mail-agent-2026-10.md section 8): first-time human
//! senders sitting in the proposed `State/New` default.
//!
//! Milestone 1's `kind::classify` already proposes `State/New` for any
//! non-broadcast `People` message (`propose_state_new`), since the sender
//! ledger that would distinguish "first contact" from "ongoing
//! correspondent" doesn't exist yet (documented there and here, not
//! glossed over). This tool's honest scope at this milestone: group by
//! distinct sender, not message — a screener candidate is a *sender*, not
//! every message they've sent.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::cleanup::domain::kind::Kind;
use crate::tools::context::ToolContext;
use crate::tools::registry::ToolError;
use crate::tools::staff::params::ScreenerRequest;
use crate::tools::staff::scan::scan_mail;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenerCandidate {
    pub from_addr: String,
    pub message_count: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenerReport {
    pub candidates: Vec<ScreenerCandidate>,
    /// Honest caveat surfaced in the payload itself, not just a doc
    /// comment — every People sender currently proposes State/New because
    /// the sender ledger (first/last-seen) doesn't exist yet, so this list
    /// is "every distinct human sender seen", not "senders seen for the
    /// first time" until that ledger lands.
    pub caveat: &'static str,
}

pub async fn screener(ctx: Arc<ToolContext>, req: ScreenerRequest) -> Result<Value, ToolError> {
    let messages = scan_mail(&ctx, req.account_id.as_deref(), false).await?;

    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for m in messages {
        if m.classification.kind == Kind::People && m.classification.propose_state_new {
            *counts.entry(m.from_addr).or_insert(0) += 1;
        }
    }

    let mut candidates: Vec<ScreenerCandidate> = counts
        .into_iter()
        .map(|(from_addr, message_count)| ScreenerCandidate {
            from_addr,
            message_count,
        })
        .collect();
    candidates.sort_by(|a, b| b.message_count.cmp(&a.message_count).then(a.from_addr.cmp(&b.from_addr)));

    let report = ScreenerReport {
        candidates,
        caveat: "Every non-broadcast People sender proposes State/New until the sender ledger \
                 (first/last-seen) exists — this list is every distinct human sender seen, not \
                 yet narrowed to first-time contacts.",
    };
    serde_json::to_value(report).map_err(|e| ToolError::Database(format!("serializing screener report: {e}")))
}
