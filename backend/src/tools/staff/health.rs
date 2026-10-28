//! `health` — the monthly report (docs/plan/mail-agent-2026-10.md section
//! 8: "promote/demote topics against the gate; sender drift; organization
//! proposals with expected effect; retention proposals only on trusted
//! Notifications rules with ephemeral senders ..., never on the personal
//! account; Receipts, People and security never; security (app password
//! age, 0 sends, revoke link); trusted-rule audit"). Read-only: every
//! section here reports what *would* happen, nothing is applied.
//!
//! **Topic gate.** `topic_clusters` rows are turned into
//! `topic_gate::TopicStats` (message count from the stored `email_count`,
//! distinct senders from a bounded lookup over the first 500 of the
//! cluster's `email_ids` — a real computation, capped so one very large
//! cluster can't turn a monthly report into an unbounded query) and run
//! through the existing `topic_gate::run_gate_pass`. `has_distinct_disposition`
//! is always `false`: no per-cluster disposition/retention config is
//! persisted yet, so every cluster is judged on volume alone — the plan's
//! exemption clause (section 7) isn't wired to real data today.
//!
//! **Retention proposals.** Evaluated per (trusted rule x connected
//! account) pair, not globally: the `rules` table has no account-scope
//! column at all (migration 032 added only `authority_level`/
//! `clean_streak`/`graduated_at`), so a rule can't be attributed to one
//! specific account. Proposing per account and skipping the personal one
//! (via `rules::retention::propose_retention`'s own account-role check)
//! keeps the "never on the personal account" guarantee exact without
//! having to suppress every proposal just because a personal account
//! happens to also be connected. A rule's `Kind` is derived from its
//! `actions_json`'s `AddLabel` action (matching `Kind::label()`'s exact
//! strings); a rule with no recognized `Kind/` label is ineligible, same
//! as any non-Notifications Kind. "Ephemeral sender" is approximated as
//! `kind == Notifications` itself: the Kind classifier's own rule 5
//! (`cleanup::domain::kind`) only assigns Notifications to bulk/automated,
//! no-reply-expected senders (`noreply`/`notification`/`alert` markers or
//! known platform domains) in the first place, and there is no reply or
//! open-rate ledger yet to compute a tighter threshold independently (the
//! plan's own only defined engagement threshold — section 8's
//! subscriptions table, ">= 10 messages, 0 opened in 90 days" — needs
//! per-message open tracking this schema doesn't have either). This is a
//! judgment call, not a precise reading of "ephemeral."
//!
//! **Security signals.** `app_password_age_days` uses an IMAP account's
//! `connected_accounts.created_at` as a proxy — the real app-password
//! rotation date isn't tracked separately, so this is "how long this
//! account has been connected," not "how old the current password is."
//! Zero-sends and a revoke link have no backing data at all (no sent-mail
//! ingestion, no vendor-link table) and are reported as explicitly absent.
//!
//! **Sender drift** reuses `noise_trend::sender_volume_trend` with a
//! 30-day window (vs. `review`'s 7-day one).
//!
//! **Organization proposals with expected effect** is `/mail suggest`
//! territory (section 8), not built in this milestone — reported as
//! explicitly absent rather than faked.

use std::collections::HashSet;
use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QuerySelect};
use serde_json::{json, Value};

use super::noise_trend::sender_volume_trend;
use super::params::HealthRequest;
use super::scan::scan_mail;
use crate::cleanup::domain::kind::Kind;
use crate::cleanup::domain::topic_gate::{self, TopicStats};
use crate::db::entities::{connected_accounts, emails, rules, topic_clusters};
use crate::rules::authority::AuthorityLevel;
use crate::rules::retention::{propose_retention, RuleFact};
use crate::rules::types::RuleAction;
use crate::tools::readonly::{db_error, validate_uuid};
use crate::tools::{ToolContext, ToolError};

/// Cap on how many of a cluster's `email_ids` get looked up for distinct
/// senders — see module doc.
const DISTINCT_SENDER_LOOKUP_CAP: usize = 500;
/// Sender-drift window (vs. `review`'s 7-day noise trend).
const SENDER_DRIFT_WINDOW_DAYS: i64 = 30;

#[derive(FromQueryResult)]
struct FromAddrRow {
    from_addr: String,
}

fn kind_from_actions(actions_json: &str) -> Option<Kind> {
    let actions: Vec<RuleAction> = serde_json::from_str(actions_json).ok()?;
    actions.iter().find_map(|a| match a {
        RuleAction::AddLabel { label } => kind_from_label(label),
        _ => None,
    })
}

fn kind_from_label(label: &str) -> Option<Kind> {
    [
        Kind::People,
        Kind::Subscriptions,
        Kind::Receipts,
        Kind::Notifications,
        Kind::OtherAgent,
        Kind::Other,
    ]
    .into_iter()
    .find(|kind| kind.label() == label)
}

async fn topic_gate_report(ctx: &Arc<ToolContext>) -> Result<Vec<Value>, ToolError> {
    let clusters = topic_clusters::Entity::find()
        .all(&ctx.conn())
        .await
        .map_err(|e| db_error("Loading topic clusters", e))?;

    let mut stats = Vec::with_capacity(clusters.len());
    for c in &clusters {
        let ids: Vec<String> = serde_json::from_str(&c.email_ids).unwrap_or_default();
        let capped: Vec<String> = ids.into_iter().take(DISTINCT_SENDER_LOOKUP_CAP).collect();
        let distinct_senders: HashSet<String> = if capped.is_empty() {
            HashSet::new()
        } else {
            emails::Entity::find()
                .select_only()
                .column(emails::Column::FromAddr)
                .filter(emails::Column::Id.is_in(capped))
                .into_model::<FromAddrRow>()
                .all(&ctx.conn())
                .await
                .map_err(|e| db_error("Loading cluster sender sample", e))?
                .into_iter()
                .map(|r| r.from_addr)
                .collect()
        };
        stats.push(TopicStats {
            topic: c.name.clone(),
            message_count: c.email_count.max(0) as u64,
            distinct_senders,
        });
    }

    let decisions = topic_gate::run_gate_pass(&stats, |_| false);
    Ok(decisions
        .into_iter()
        .zip(stats.iter())
        .map(|((topic, decision), s)| {
            json!({
                "topic": topic,
                "decision": format!("{decision:?}"),
                "message_count": s.message_count,
                "distinct_sender_count": s.distinct_senders.len(),
            })
        })
        .collect())
}

async fn retention_report(
    ctx: &Arc<ToolContext>,
    accounts: &[connected_accounts::Model],
) -> Result<Vec<Value>, ToolError> {
    let rule_rows = rules::Entity::find()
        .all(&ctx.conn())
        .await
        .map_err(|e| db_error("Loading rules", e))?;

    let facts: Vec<RuleFact> = rule_rows
        .iter()
        .map(|r| {
            let kind = kind_from_actions(&r.actions_json);
            RuleFact {
                rule_id: r.id.clone(),
                authority: AuthorityLevel::parse(r.authority_level.as_deref().unwrap_or("")),
                kind,
                is_security_flagged: false,
                is_ephemeral_sender: kind == Some(Kind::Notifications),
            }
        })
        .collect();

    let rule_names: std::collections::HashMap<&str, &str> = rule_rows
        .iter()
        .map(|r| (r.id.as_str(), r.name.as_str()))
        .collect();

    let mut proposals = Vec::new();
    for account in accounts {
        for fact in &facts {
            if let Some(proposal) = propose_retention(fact, account.role.as_deref()) {
                proposals.push(json!({
                    "rule_id": proposal.rule_id,
                    "rule_name": rule_names.get(proposal.rule_id.as_str()).copied().unwrap_or(""),
                    "account_id": account.id,
                    "account_email": account.email_address,
                    "trash_after_days": proposal.trash_after_days,
                    "undo_window_days": proposal.undo_window_days,
                }));
            }
        }
    }
    Ok(proposals)
}

fn trusted_rule_audit(rule_rows: &[rules::Model]) -> Vec<Value> {
    rule_rows
        .iter()
        .map(|r| {
            json!({
                "rule_id": r.id,
                "name": r.name,
                "authority_level": r.authority_level.clone().unwrap_or_else(|| "suggest".to_string()),
                "clean_streak": r.clean_streak.unwrap_or(0),
                "graduated_at": r.graduated_at,
            })
        })
        .collect()
}

fn parse_account_timestamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|naive| naive.and_utc())
        })
}

fn security_signals(accounts: &[connected_accounts::Model]) -> Vec<Value> {
    let now = chrono::Utc::now();
    accounts
        .iter()
        .filter(|a| a.provider == "imap")
        .map(|a| {
            let age_days = parse_account_timestamp(&a.created_at)
                .map(|created| (now - created).num_days());
            json!({
                "account_id": a.id,
                "account_email": a.email_address,
                "app_password_age_days": age_days,
                "signal_source": "connected_accounts.created_at (proxy — the real rotation date isn't tracked separately)",
            })
        })
        .collect()
}

pub async fn health(ctx: Arc<ToolContext>, req: HealthRequest) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let mut accounts_query = connected_accounts::Entity::find();
    if let Some(ref id) = req.account_id {
        accounts_query = accounts_query.filter(connected_accounts::Column::Id.eq(id.clone()));
    }
    let accounts = accounts_query
        .all(&ctx.conn())
        .await
        .map_err(|e| db_error("Loading connected accounts", e))?;

    let rule_rows = rules::Entity::find()
        .all(&ctx.conn())
        .await
        .map_err(|e| db_error("Loading rules", e))?;

    let topic_gate = topic_gate_report(&ctx).await?;
    let retention_proposals = retention_report(&ctx, &accounts).await?;
    let trusted_rule_audit = trusted_rule_audit(&rule_rows);
    let security = security_signals(&accounts);

    let messages = scan_mail(&ctx, req.account_id.as_deref(), false).await?;
    let sender_drift = sender_volume_trend(&messages, SENDER_DRIFT_WINDOW_DAYS, 5);

    let response = json!({
        "topic_gate": topic_gate,
        "retention_proposals": retention_proposals,
        "retention_proposals_count": retention_proposals.len(),
        "trusted_rule_audit": trusted_rule_audit,
        "security_signals": security,
        "sender_drift": sender_drift,
        "not_yet_available": [
            "organization proposals with expected effect (needs the /mail suggest engine, not built yet)",
            "zero-sends security signal (needs sent-mail ingestion, not built yet)",
            "revoke-link security signal (needs a vendor account-settings link table, not built yet)",
        ],
    });

    super::saved_reports::save_report(&ctx, "health", req.account_id.as_deref(), &response).await?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_label_round_trips_every_kind_variant() {
        for kind in [
            Kind::People,
            Kind::Subscriptions,
            Kind::Receipts,
            Kind::Notifications,
            Kind::OtherAgent,
            Kind::Other,
        ] {
            assert_eq!(kind_from_label(kind.label()), Some(kind));
        }
    }

    #[test]
    fn an_unrecognized_label_has_no_kind() {
        assert_eq!(kind_from_label("not-a-kind-label"), None);
    }

    #[test]
    fn kind_from_actions_finds_the_add_label_action() {
        let json = r#"[{"type":"markRead"},{"type":"addLabel","label":"Kind/Notifications"}]"#;
        assert_eq!(kind_from_actions(json), Some(Kind::Notifications));
    }

    #[test]
    fn kind_from_actions_is_none_with_no_recognized_label() {
        let json = r#"[{"type":"archive"}]"#;
        assert_eq!(kind_from_actions(json), None);
    }
}
