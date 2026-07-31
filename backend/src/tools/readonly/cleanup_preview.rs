//! Strictly dry-run cleanup plan preview (ADR-030).
//!
//! `PlanBuilder::build` is a pure read: it reaches the outside world only
//! through the read-only ports in `cleanup::domain::ports` and evaluates rules
//! with `RuleExecutionMode::EvaluateOnly`. Persistence is a separate step —
//! the REST handler follows `build` with `cleanup_plan_repo.save`. This tool
//! deliberately omits that call, so nothing is written, no provider is
//! contacted, and no plan id becomes addressable.
//!
//! Do not add a `save`, `cancel`, `replace_account_rows`, or apply call here.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::Row;

use super::params::PreviewCleanupPlanRequest;
use super::{validate_limit, validate_user_id, validate_uuid};
use crate::cleanup::domain::builder::PlanBuilder;
use crate::cleanup::domain::classifier::RiskClassifier;
use crate::cleanup::domain::operation::{
    ArchiveStrategy, ClusterAction, PlanAction, PlannedOperation, Provider,
};
use crate::cleanup::domain::plan::{
    ClusterSelection, RuleSelection, SubscriptionSelection, WizardSelections,
};
use crate::cleanup::domain::ports::{
    AccountStateProvider, ClusterRepository, EmailRepository, RuleEvaluator, SubscriptionRepository,
};
use crate::cleanup::repository::{
    SqlxAccountStateProvider, SqlxClusterRepository, SqlxEmailRepository, SqlxRuleEvaluator,
    SqlxSubscriptionRepository,
};
use crate::tools::{ToolContext, ToolError};

/// Build a cleanup plan in memory and summarize it without persisting.
pub async fn preview_cleanup_plan(
    ctx: Arc<ToolContext>,
    req: PreviewCleanupPlanRequest,
) -> Result<Value, ToolError> {
    let user_id = req.user_id.clone();
    validate_user_id(&user_id).map_err(ToolError::Invalid)?;
    let sample_limit = validate_limit(req.sample_limit.unwrap_or(10), 50) as usize;

    let selections = build_selections(req).map_err(ToolError::Invalid)?;

    let builder = plan_builder(ctx.pool())
        .await
        .map_err(|e| super::db_error("Preparing the cleanup plan builder", e))?;

    let plan = builder
        .build(&user_id, selections)
        .await
        .map_err(|e| super::db_error("Building the cleanup plan", e))?;

    let samples: Vec<Value> = plan
        .operations
        .iter()
        .take(sample_limit)
        .map(operation_summary)
        .collect();

    Ok(json!({
        "persisted": false,
        "dry_run": true,
        "user_id": user_id,
        "account_ids": plan.account_ids,
        "status": plan.status.as_str(),
        "valid_until": plan.valid_until.to_rfc3339(),
        "totals": serde_json::to_value(&plan.totals).unwrap_or(Value::Null),
        "risk": serde_json::to_value(&plan.risk).unwrap_or(Value::Null),
        "warnings": serde_json::to_value(&plan.warnings).unwrap_or(Value::Null),
        "operation_count": plan.operations.len(),
        "sample_operations": samples,
        "note": "Preview only — nothing was saved and no mailbox was modified. \
                 Create a real plan via POST /api/v1/cleanup/plan to apply it.",
    }))
}

/// Assemble a `PlanBuilder` over the read-only SQLx adapters.
///
/// Mirrors the REST wiring helper (`cleanup/api/plan.rs`), but takes a pool
/// rather than `AppState`: the plan-build path needs nothing else, and
/// depending only on the pool keeps this tool reachable from the library
/// crate once `cleanup::{domain, repository}` move there.
async fn plan_builder(pool: &sqlx::SqlitePool) -> Result<PlanBuilder, sqlx::Error> {
    // Load per-account providers up front so the lookup closure stays sync.
    let rows = sqlx::query("SELECT id, provider FROM connected_accounts")
        .fetch_all(pool)
        .await?;

    let provider_map: Arc<HashMap<String, Provider>> = Arc::new(
        rows.iter()
            .map(|r| {
                let id: String = r.get("id");
                let provider: String = r.get("provider");
                let provider = match provider.as_str() {
                    "outlook" => Provider::Outlook,
                    "imap" => Provider::Imap,
                    "pop3" => Provider::Pop3,
                    _ => Provider::Gmail,
                };
                (id, provider)
            })
            .collect(),
    );

    Ok(PlanBuilder {
        emails: Arc::new(SqlxEmailRepository { pool: pool.clone() }) as Arc<dyn EmailRepository>,
        subs: Arc::new(SqlxSubscriptionRepository { pool: pool.clone() })
            as Arc<dyn SubscriptionRepository>,
        clusters: Arc::new(SqlxClusterRepository { pool: pool.clone() })
            as Arc<dyn ClusterRepository>,
        rules: Arc::new(SqlxRuleEvaluator { pool: pool.clone() }) as Arc<dyn RuleEvaluator>,
        accounts: Arc::new(SqlxAccountStateProvider { pool: pool.clone() })
            as Arc<dyn AccountStateProvider>,
        classifier: Arc::new(RiskClassifier::new()),
        provider_for: Arc::new(move |account_id: &str| {
            *provider_map.get(account_id).unwrap_or(&Provider::Gmail)
        }),
        plan_ttl_minutes: 30,
    })
}

/// Translate the tool's flat arguments into domain `WizardSelections`.
fn build_selections(req: PreviewCleanupPlanRequest) -> Result<WizardSelections, String> {
    for account_id in &req.account_ids {
        validate_uuid("account_id", account_id)?;
    }

    let mut subscriptions = Vec::with_capacity(req.unsubscribe_senders.len());
    for sel in req.unsubscribe_senders {
        validate_uuid("account_id", &sel.account_id)?;
        if sel.sender.trim().is_empty() {
            return Err("sender must not be empty".to_string());
        }
        subscriptions.push(SubscriptionSelection {
            sender: sel.sender,
            account_id: sel.account_id,
        });
    }

    let mut cluster_actions = Vec::with_capacity(req.cluster_actions.len());
    for sel in req.cluster_actions {
        validate_uuid("account_id", &sel.account_id)?;
        if sel.cluster_id.trim().is_empty() {
            return Err("cluster_id must not be empty".to_string());
        }
        cluster_actions.push(ClusterSelection {
            cluster_id: sel.cluster_id,
            action: parse_cluster_action(&sel.action)?,
            account_id: sel.account_id,
        });
    }

    let mut rule_selections = Vec::with_capacity(req.rule_selections.len());
    for sel in req.rule_selections {
        validate_uuid("account_id", &sel.account_id)?;
        if sel.rule_id.trim().is_empty() {
            return Err("rule_id must not be empty".to_string());
        }
        rule_selections.push(RuleSelection {
            rule_id: sel.rule_id,
            account_id: sel.account_id,
        });
    }

    let archive_strategy = req
        .archive_strategy
        .as_deref()
        .map(parse_archive_strategy)
        .transpose()?;

    Ok(WizardSelections {
        subscriptions,
        cluster_actions,
        rule_selections,
        archive_strategy,
        account_ids: req.account_ids,
    })
}

fn parse_cluster_action(raw: &str) -> Result<ClusterAction, String> {
    match raw {
        "archive" => Ok(ClusterAction::Archive),
        "deleteSoft" | "delete_soft" => Ok(ClusterAction::DeleteSoft),
        "deletePermanent" | "delete_permanent" => Ok(ClusterAction::DeletePermanent),
        "label" => Ok(ClusterAction::Label),
        other => Err(format!(
            "Unknown cluster action '{other}'. Expected archive, deleteSoft, deletePermanent, or label"
        )),
    }
}

fn parse_archive_strategy(raw: &str) -> Result<ArchiveStrategy, String> {
    match raw {
        "olderThan30d" | "older_than_30d" => Ok(ArchiveStrategy::OlderThan30d),
        "olderThan90d" | "older_than_90d" => Ok(ArchiveStrategy::OlderThan90d),
        "olderThan1y" | "older_than_1y" => Ok(ArchiveStrategy::OlderThan1y),
        "custom" => Ok(ArchiveStrategy::Custom),
        other => Err(format!(
            "Unknown archive strategy '{other}'. Expected olderThan30d, olderThan90d, olderThan1y, or custom"
        )),
    }
}

/// Project one planned operation onto a redacted summary.
///
/// Built by hand rather than via `Serialize` because `PlanAction::Unsubscribe`
/// carries the raw `List-Unsubscribe` / `List-Unsubscribe-Post` headers, whose
/// one-click URLs embed per-recipient tokens.
fn operation_summary(op: &PlannedOperation) -> Value {
    match op {
        PlannedOperation::Materialized(row) => json!({
            "op_kind": "materialized",
            "seq": row.seq,
            "account_id": row.account_id,
            "email_id": row.email_id,
            "action": action_summary(&row.action),
            "source": serde_json::to_value(&row.source).unwrap_or(Value::Null),
            "risk": serde_json::to_value(row.risk).unwrap_or(Value::Null),
            "status": serde_json::to_value(row.status).unwrap_or(Value::Null),
        }),
        PlannedOperation::Predicate(p) => json!({
            "op_kind": "predicate",
            "seq": p.seq,
            "account_id": p.account_id,
            "predicate_kind": serde_json::to_value(p.predicate_kind).unwrap_or(Value::Null),
            "predicate_id": p.predicate_id,
            "action": action_summary(&p.action),
            "source": serde_json::to_value(&p.source).unwrap_or(Value::Null),
            "projected_count": p.projected_count,
            "sample_email_ids": p.sample_email_ids,
            "risk": serde_json::to_value(p.risk).unwrap_or(Value::Null),
            "status": serde_json::to_value(p.status).unwrap_or(Value::Null),
        }),
    }
}

/// Describe an action without echoing any unsubscribe URL.
fn action_summary(action: &PlanAction) -> Value {
    match action {
        PlanAction::Archive => json!({ "kind": "archive" }),
        PlanAction::AddLabel { kind } => json!({
            "kind": "addLabel",
            "move_kind": serde_json::to_value(kind).unwrap_or(Value::Null),
        }),
        PlanAction::Move { kind } => json!({
            "kind": "move",
            "move_kind": serde_json::to_value(kind).unwrap_or(Value::Null),
        }),
        PlanAction::Delete { permanent } => json!({ "kind": "delete", "permanent": permanent }),
        PlanAction::Unsubscribe { method, .. } => json!({
            "kind": "unsubscribe",
            "method": serde_json::to_value(method).unwrap_or(Value::Null),
        }),
        PlanAction::MarkRead => json!({ "kind": "markRead" }),
        PlanAction::Star { on } => json!({ "kind": "star", "on": on }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cluster_action_accepts_both_casings() {
        assert_eq!(
            parse_cluster_action("deleteSoft").unwrap(),
            ClusterAction::DeleteSoft
        );
        assert_eq!(
            parse_cluster_action("delete_soft").unwrap(),
            ClusterAction::DeleteSoft
        );
    }

    #[test]
    fn parse_cluster_action_rejects_unknown() {
        assert!(parse_cluster_action("nuke").is_err());
    }

    #[test]
    fn parse_archive_strategy_rejects_unknown() {
        assert!(parse_archive_strategy("yesterday").is_err());
        assert_eq!(
            parse_archive_strategy("olderThan90d").unwrap(),
            ArchiveStrategy::OlderThan90d
        );
    }

    #[test]
    fn action_summary_omits_unsubscribe_urls() {
        let action = PlanAction::Unsubscribe {
            method: crate::cleanup::domain::operation::UnsubscribeMethodKind::WebLink,
            list_unsubscribe_header: Some("https://example.com/u?token=SECRET".to_string()),
            list_unsubscribe_post: Some("List-Unsubscribe=One-Click".to_string()),
        };

        let summary = action_summary(&action).to_string();

        assert!(!summary.contains("SECRET"));
        assert!(!summary.contains("One-Click"));
        assert!(summary.contains("unsubscribe"));
    }

    #[test]
    fn build_selections_rejects_non_uuid_account() {
        let req = PreviewCleanupPlanRequest {
            account_ids: vec!["not-a-uuid".to_string()],
            ..Default::default()
        };

        assert!(build_selections(req).is_err());
    }
}
