//! `brief` — the attention queue (docs/plan/mail-agent-2026-10.md section 8,
//! "Attention brief").
//!
//! **What this milestone can honestly build, and what it can't yet.** The
//! full brief orders items by SLA clock and boomerangs overdue loops back to
//! the top — both need the sender ledger (first/last seen, reply flags) and
//! `State/Review` tracking section 8 describes, which are later milestones'
//! work (the sender ledger is section 8's own future-tense; `State/` writes
//! are milestone 3+). What exists today: unread, un-archived `People`-kind
//! mail currently sitting in the inbox (the "needs you" queue's honest
//! subset), and counts of everything else filed by Kind. No SLA clock, no
//! boomerang, no protected-sender alerts (the protected list doesn't exist
//! yet either) — all three are reported as explicitly absent rather than
//! faked, per this session's standing rule against fabricating data to look
//! more complete than the schema supports.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::BriefRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::kind::Kind;
use crate::tools::readonly::validate_uuid;
use crate::tools::{ToolContext, ToolError};

pub async fn brief(ctx: Arc<ToolContext>, req: BriefRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let messages = scan_mail(&ctx, req.account_id.as_deref(), true).await?;

    let mut needs_you = Vec::new();
    let mut filed_counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();

    for m in &messages {
        *filed_counts
            .entry(m.classification.kind.label())
            .or_insert(0) += 1;
        if m.classification.kind == Kind::People && !m.is_read {
            needs_you.push(json!({
                "account_id": m.account_id,
                "from_addr": m.from_addr,
                "received_at": m.received_at.and_utc().to_rfc3339(),
                "is_broadcast": m.classification.is_broadcast,
            }));
        }
    }

    Ok(json!({
        "needs_you": needs_you,
        "needs_you_count": needs_you.len(),
        "filed_counts": filed_counts,
        "open_loops": [],
        "protected_alerts": [],
        "not_yet_available": [
            "SLA clocks and the overdue boomerang (needs the sender ledger, section 8)",
            "open_loops (needs State/Review tracking, milestone 3+)",
            "protected_alerts (needs the protected-sender list, not built yet)",
        ],
    }))
}
