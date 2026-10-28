//! Settings checklist (docs/plan/mail-agent-2026-10.md section 8): source-
//! settings changes ranked by volume eliminated — "things no email tool
//! should handle" (Circle 22% of inflow, GitHub CI/push 423/month, etc. in
//! the measured accounts).
//!
//! Reuses milestone 1's Kind classification and sender key: every message
//! classified `Notifications` is grouped by sender key, ranked by count.
//! This is the generic ranking; the plan's richer "what to actually change
//! at the source" copy per platform is UX/copy work for a later pass — this
//! tool returns the ranked facts a checklist is built from.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::sender_key::SenderKey;
use crate::tools::context::ToolContext;
use crate::tools::registry::ToolError;
use crate::tools::staff::params::SettingsChecklistRequest;
use crate::tools::staff::scan::scan_mail;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub sender_key: String,
    pub message_count: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChecklist {
    pub items: Vec<ChecklistItem>,
    pub total_notification_messages: u32,
}

pub async fn settings_checklist(
    ctx: Arc<ToolContext>,
    req: SettingsChecklistRequest,
) -> Result<Value, ToolError> {
    let messages = scan_mail(&ctx, req.account_id.as_deref(), false).await?;

    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut total = 0u32;
    for m in messages {
        if m.classification.kind != Kind::Notifications {
            continue;
        }
        total += 1;
        let key = SenderKey::from_address(&m.from_addr)
            .map(|k| k.as_str().to_string())
            .unwrap_or(m.from_addr);
        *counts.entry(key).or_insert(0) += 1;
    }

    let mut items: Vec<ChecklistItem> = counts
        .into_iter()
        .map(|(sender_key, message_count)| ChecklistItem {
            sender_key,
            message_count,
        })
        .collect();
    // Highest volume first — "ranked by volume eliminated".
    items.sort_by(|a, b| {
        b.message_count
            .cmp(&a.message_count)
            .then(a.sender_key.cmp(&b.sender_key))
    });

    let out = SettingsChecklist {
        items,
        total_notification_messages: total,
    };
    serde_json::to_value(out)
        .map_err(|e| ToolError::Database(format!("serializing settings checklist: {e}")))
}
