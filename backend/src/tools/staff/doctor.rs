//! `doctor` — connection health, write capabilities, label strategy, scan
//! estimate (docs/plan/mail-agent-2026-10.md section 10, milestone 2).
//!
//! Wraps the same account/sync-state data `list_accounts`/`get_sync_status`
//! already expose (`readonly::accounts`), adding the capability and
//! label-strategy framing section 4's architecture calls for and a rough
//! scan-time estimate against the plan's own throughput target (section 3:
//! "Header scan throughput >= 1,000 msgs/s on Gmail").

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::DoctorRequest;
use crate::email::types::ProviderKind;
use crate::email::AccountStatus;
use crate::tools::readonly::{db_error, validate_uuid};
use crate::tools::{ToolContext, ToolError};

/// Report connection health, label strategy, and a scan-time estimate for
/// one account or every connected account.
pub async fn doctor(ctx: Arc<ToolContext>, req: DoctorRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let oauth = ctx.oauth()?;
    let mut accounts = oauth
        .list_accounts()
        .await
        .map_err(|e| db_error("Listing accounts", e))?;

    if let Some(ref id) = req.account_id {
        accounts.retain(|a| &a.id == id);
        if accounts.is_empty() {
            return Err(ToolError::NotFound(format!(
                "No connected account with id {id}"
            )));
        }
    }

    let mut items = Vec::with_capacity(accounts.len());
    for account in &accounts {
        let sync = oauth.get_sync_state(&account.id).await.ok().flatten();
        let emails_synced = sync.as_ref().map(|s| s.emails_synced).unwrap_or(0);

        // Section 3's own throughput target, used as the estimate's basis
        // rather than a separately measured number — this build has no
        // historical scan-rate data to draw on yet.
        let scan_estimate_seconds = (emails_synced as f64 / 1000.0).ceil() as u64;

        let (label_strategy, capabilities) = capabilities_for(&account.provider);

        items.push(json!({
            "account_id": account.id,
            "provider": account.provider.as_str(),
            "email_address": account.email_address,
            "status": account.status.as_str(),
            "connected": account.status == AccountStatus::Connected,
            "capabilities": capabilities,
            "label_strategy": label_strategy,
            "emails_synced": emails_synced,
            "scan_estimate_seconds": scan_estimate_seconds,
            "sync_failures": sync.as_ref().map(|s| s.sync_failures).unwrap_or(0),
            "last_error": sync.as_ref().and_then(|s| s.last_error.clone()),
        }));
    }

    Ok(json!({ "count": items.len(), "accounts": items }))
}

/// What write mechanism a provider offers and the label strategy it implies
/// (docs/plan/mail-agent-2026-10.md section 4, ADR-018).
fn capabilities_for(provider: &ProviderKind) -> (&'static str, Vec<&'static str>) {
    match provider {
        ProviderKind::Gmail => (
            "Gmail labels (X-GM-LABELS over IMAP with no quota, or the Gmail API under OAuth)",
            vec!["labels", "archive", "mark_read"],
        ),
        ProviderKind::Outlook => (
            "folders — no arbitrary multi-label; Kind/Topic become a folder path",
            vec!["folders", "mark_read"],
        ),
        ProviderKind::Imap => (
            "keyword flags where the server supports them, folders otherwise",
            vec!["keywords_or_folders", "mark_read"],
        ),
        ProviderKind::Pop3 => (
            "none — POP3 has no server-side label or folder concept",
            vec!["mark_read"],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_kind_has_a_capability_mapping() {
        for provider in [
            ProviderKind::Gmail,
            ProviderKind::Outlook,
            ProviderKind::Imap,
            ProviderKind::Pop3,
        ] {
            let (strategy, caps) = capabilities_for(&provider);
            assert!(!strategy.is_empty());
            assert!(!caps.is_empty());
        }
    }
}
