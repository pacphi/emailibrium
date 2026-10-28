//! `identity_map` — which senders appear across more than one connected
//! account (docs/plan/mail-agent-2026-10.md section 4: "One People graph
//! across accounts", and section 8's quarterly `/mail map`).
//!
//! Matches by milestone 1's sender key, not by raw address, so plus-tag and
//! display-name variants of the same sender key together correctly. The
//! plan's full identity map also flags *active-only* migration-nudge
//! candidates (section 8's "Migration nudges") — that needs the sender
//! ledger's last-active tracking, which doesn't exist yet, so this reports
//! the raw cross-account overlap only, with every account the sender
//! touched and how many messages in each, and leaves the nudge judgment for
//! the milestone that has the ledger to make it with.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::IdentityMapRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::sender_key::SenderKey;
use crate::tools::{ToolContext, ToolError};

pub async fn identity_map(
    ctx: Arc<ToolContext>,
    _req: IdentityMapRequest,
) -> Result<Value, ToolError> {
    let messages = scan_mail(&ctx, None, false).await?;

    let mut by_sender: std::collections::HashMap<
        SenderKey,
        std::collections::HashMap<String, usize>,
    > = std::collections::HashMap::new();

    for m in &messages {
        let Some(key) = SenderKey::from_address(&m.from_addr) else {
            continue;
        };
        *by_sender
            .entry(key)
            .or_default()
            .entry(m.account_id.clone())
            .or_insert(0) += 1;
    }

    let mut cross_account: Vec<Value> = by_sender
        .into_iter()
        .filter(|(_, accounts)| accounts.len() > 1)
        .map(|(key, accounts)| {
            let total: usize = accounts.values().sum();
            let accounts_json: Vec<Value> = accounts
                .into_iter()
                .map(
                    |(account_id, count)| json!({ "account_id": account_id, "email_count": count }),
                )
                .collect();
            json!({
                "sender_key": key.as_str(),
                "accounts": accounts_json,
                "total_email_count": total,
            })
        })
        .collect();

    // Largest overlap first — the senders most worth a migration decision.
    cross_account.sort_by(|a, b| {
        let a_total = a["total_email_count"].as_u64().unwrap_or(0);
        let b_total = b["total_email_count"].as_u64().unwrap_or(0);
        b_total.cmp(&a_total)
    });

    Ok(json!({
        "cross_account_senders": cross_account,
        "cross_account_sender_count": cross_account.len(),
        "not_yet_available": [
            "active-only migration-nudge candidates (needs the sender ledger's last-active tracking, section 8)",
        ],
    }))
}
