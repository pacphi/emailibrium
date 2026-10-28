//! `get_snapshot` — mailbox shape, archetype, and role-mismatch check
//! (docs/plan/mail-agent-2026-10.md section 10, milestone 2).
//!
//! Wraps the existing insights/counts data with the new Kind-distribution and
//! archetype detection section 6's four playbooks describe. The exit test
//! this and milestone 1's golden mailboxes satisfy together is section 10's
//! "snapshot reproduces the Oct 26 profiles; professional account flagged
//! Mixed" — this tool is the first thing that actually renders a snapshot
//! rather than only classifying individual messages.
//!
//! **Documented approximations**, same spirit as milestone 1's: section 6's
//! archetype criteria mention signals this build doesn't have yet (recruiter
//! share, a decaying-inflow curve, sent-mail analysis for household
//! detection). The rules below use the signals that exist today — human
//! share, receipts share, a GitHub-domain share, inflow rate, and account
//! history length — and are simplifications of, not a verbatim encoding of,
//! section 6's table. "Mixed" is the deliberate fallback when no playbook's
//! criteria clear their threshold, not a failure to classify.
//!
//! GitHub's reason-routing (`Classification::github_reason`) is always
//! `None` for already-ingested mail, because `cc` isn't persisted (the same
//! gap `staff::scan` documents) — this module derives its own "GitHub share"
//! straight from the sender's domain instead, independent of the
//! reason-routing milestone 1 built for the live-fetch path.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::GetSnapshotRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::sender_key::SenderKey;
use crate::db::entities::connected_accounts;
use crate::email::types::AccountRole;
use crate::tools::readonly::{db_error, validate_uuid};
use crate::tools::{ToolContext, ToolError};

/// One of section 6's four playbooks, or the deliberate "no clean fit"
/// fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Archetype {
    MachineDominatedIdentity,
    PrimaryLifeAccount,
    ProfessionalNetwork,
    LegacySignupSink,
    Mixed,
}

impl Archetype {
    fn label(&self) -> &'static str {
        match self {
            Self::MachineDominatedIdentity => "Machine-dominated identity",
            Self::PrimaryLifeAccount => "Primary life account",
            Self::ProfessionalNetwork => "Professional network",
            Self::LegacySignupSink => "Legacy / signup sink",
            Self::Mixed => "Mixed",
        }
    }

    /// The role section 6's measured examples associate with this archetype
    /// — used only to flag a likely mismatch, never to override what the
    /// user declared. `None` means this archetype has no strong role
    /// expectation, so no mismatch is ever flagged against it.
    fn expected_role(&self) -> Option<AccountRole> {
        match self {
            Self::MachineDominatedIdentity => Some(AccountRole::Business),
            Self::PrimaryLifeAccount => Some(AccountRole::Personal),
            Self::ProfessionalNetwork => Some(AccountRole::Professional),
            Self::LegacySignupSink | Self::Mixed => None,
        }
    }
}

struct Shape {
    total: usize,
    human_share: f64,
    receipts_share: f64,
    notifications_share: f64,
    github_share: f64,
    inflow_per_day: f64,
    history_days: i64,
}

fn detect_archetype(shape: &Shape) -> Archetype {
    if shape.total == 0 {
        return Archetype::Mixed;
    }
    if shape.human_share < 0.05 && shape.inflow_per_day > 10.0 {
        return Archetype::MachineDominatedIdentity;
    }
    if shape.human_share > 0.15 && shape.history_days > 5 * 365 && shape.receipts_share > 0.10 {
        return Archetype::PrimaryLifeAccount;
    }
    if shape.github_share > 0.30 && (0.05..=0.15).contains(&shape.human_share) {
        return Archetype::ProfessionalNetwork;
    }
    if shape.inflow_per_day < 3.0 && shape.notifications_share > 0.5 && shape.human_share < 0.10 {
        return Archetype::LegacySignupSink;
    }
    Archetype::Mixed
}

/// Snapshot one account's mailbox shape: counts by Kind, human/receipts/
/// GitHub share, inflow rate, history length, the detected archetype, and a
/// role-mismatch flag against the declared `AccountRole` (milestone 1).
pub async fn get_snapshot(
    ctx: Arc<ToolContext>,
    req: GetSnapshotRequest,
) -> Result<Value, ToolError> {
    validate_uuid("account_id", &req.account_id).map_err(ToolError::Invalid)?;

    let account = {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        connected_accounts::Entity::find()
            .filter(connected_accounts::Column::Id.eq(req.account_id.as_str()))
            .one(&ctx.conn())
            .await
            .map_err(|e| db_error("Loading account", e))?
            .ok_or_else(|| ToolError::NotFound(format!("No account with id {}", req.account_id)))?
    };

    let declared_role = account.role.as_deref().and_then(role_from_str);

    let messages = scan_mail(&ctx, Some(req.account_id.as_str()), false).await?;
    let total = messages.len();

    let mut kind_counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let mut human = 0usize;
    let mut receipts = 0usize;
    let mut notifications = 0usize;
    let mut github = 0usize;
    let mut earliest: Option<chrono::NaiveDateTime> = None;
    let mut last_30_days = 0usize;
    let now = chrono::Utc::now().naive_utc();
    let thirty_days_ago = now - chrono::Duration::days(30);

    for m in &messages {
        let label = m.classification.kind.label();
        *kind_counts.entry(label).or_insert(0) += 1;

        match m.classification.kind {
            Kind::People => human += 1,
            Kind::Receipts => receipts += 1,
            Kind::Notifications => {
                notifications += 1;
                if let Some(key) = SenderKey::from_address(&m.from_addr) {
                    let domain = key.as_str().rsplit_once('@').map(|(_, d)| d).unwrap_or("");
                    if domain == "github.com" || domain.ends_with(".github.com") {
                        github += 1;
                    }
                }
            }
            _ => {}
        }

        earliest = Some(earliest.map_or(m.received_at, |e| e.min(m.received_at)));
        if m.received_at >= thirty_days_ago {
            last_30_days += 1;
        }
    }

    let history_days = earliest.map(|e| (now - e).num_days()).unwrap_or(0);
    let shape = Shape {
        total,
        human_share: ratio(human, total),
        receipts_share: ratio(receipts, total),
        notifications_share: ratio(notifications, total),
        github_share: ratio(github, total),
        inflow_per_day: last_30_days as f64 / 30.0,
        history_days,
    };

    let archetype = detect_archetype(&shape);
    let role_mismatch = match (declared_role, archetype.expected_role()) {
        (Some(declared), Some(expected)) if declared != expected => Some(json!({
            "declared": declared.as_str(),
            "measured_fits": expected.as_str(),
            "message": format!(
                "You declared this account {}, but its measured shape fits '{}' better. Treat as Mixed?",
                declared.as_str(), archetype.label()
            ),
        })),
        _ => None,
    };

    Ok(json!({
        "account_id": account.id,
        "email_address": account.email_address,
        "declared_role": declared_role.map(|r| r.as_str()),
        "total_messages_scanned": total,
        "kind_distribution": kind_counts,
        "human_share": shape.human_share,
        "receipts_share": shape.receipts_share,
        "notifications_share": shape.notifications_share,
        "github_share": shape.github_share,
        "inflow_per_day_last_30d": shape.inflow_per_day,
        "history_days": shape.history_days,
        "archetype": archetype.label(),
        "role_mismatch": role_mismatch,
        "approximated": [
            "github_share is domain-based, not reason-routed (cc isn't persisted on ingested mail)",
            "archetype thresholds are a simplification of section 6's table; recruiter share, decaying-inflow curves, and sent-mail analysis aren't built yet",
        ],
    }))
}

fn ratio(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

fn role_from_str(s: &str) -> Option<AccountRole> {
    match s {
        "personal" => Some(AccountRole::Personal),
        "professional" => Some(AccountRole::Professional),
        "business" => Some(AccountRole::Business),
        "shared" => Some(AccountRole::Shared),
        "retired" => Some(AccountRole::Retired),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(
        human: f64,
        receipts: f64,
        notifications: f64,
        github: f64,
        inflow: f64,
        history_days: i64,
    ) -> Shape {
        Shape {
            total: 100,
            human_share: human,
            receipts_share: receipts,
            notifications_share: notifications,
            github_share: github,
            inflow_per_day: inflow,
            history_days,
        }
    }

    #[test]
    fn machine_dominated_needs_low_human_share_and_high_inflow() {
        let s = shape(0.02, 0.0, 0.8, 0.0, 17.0, 400);
        assert_eq!(detect_archetype(&s), Archetype::MachineDominatedIdentity);
    }

    #[test]
    fn primary_life_needs_history_and_receipts_alongside_human_share() {
        let s = shape(0.40, 0.15, 0.3, 0.0, 49.0, 20 * 365);
        assert_eq!(detect_archetype(&s), Archetype::PrimaryLifeAccount);
    }

    #[test]
    fn professional_network_needs_high_github_share_and_moderate_humans() {
        let s = shape(0.10, 0.02, 0.7, 0.62, 47.0, 365 * 15);
        assert_eq!(detect_archetype(&s), Archetype::ProfessionalNetwork);
    }

    #[test]
    fn legacy_signup_sink_needs_low_inflow_and_high_notification_share() {
        let s = shape(0.08, 0.05, 0.7, 0.0, 2.0, 3000);
        assert_eq!(detect_archetype(&s), Archetype::LegacySignupSink);
    }

    #[test]
    fn an_empty_or_ambiguous_shape_falls_back_to_mixed() {
        let s = shape(0.10, 0.10, 0.10, 0.10, 5.0, 100);
        assert_eq!(detect_archetype(&s), Archetype::Mixed);

        let empty = Shape {
            total: 0,
            human_share: 0.0,
            receipts_share: 0.0,
            notifications_share: 0.0,
            github_share: 0.0,
            inflow_per_day: 0.0,
            history_days: 0,
        };
        assert_eq!(detect_archetype(&empty), Archetype::Mixed);
    }

    #[test]
    fn mixed_and_legacy_signup_sink_never_trigger_a_role_mismatch() {
        assert_eq!(Archetype::Mixed.expected_role(), None);
        assert_eq!(Archetype::LegacySignupSink.expected_role(), None);
    }

    #[test]
    fn each_archetype_with_a_role_expectation_maps_to_the_plans_measured_example() {
        // docs/plan/mail-agent-2026-10.md section 2's three measured accounts.
        assert_eq!(
            Archetype::MachineDominatedIdentity.expected_role(),
            Some(AccountRole::Business)
        );
        assert_eq!(
            Archetype::PrimaryLifeAccount.expected_role(),
            Some(AccountRole::Personal)
        );
        assert_eq!(
            Archetype::ProfessionalNetwork.expected_role(),
            Some(AccountRole::Professional)
        );
    }
}
