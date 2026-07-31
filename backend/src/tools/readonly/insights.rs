//! Subscription, cluster, and learning-metric tools.

use std::sync::Arc;

use serde_json::{json, Value};

use super::params::{
    GetInsightsRequest, GetLearningMetricsRequest, ListClustersRequest, ListRulesRequest,
    ListSubscriptionsRequest,
};
use super::validate_limit;
use crate::tools::{ToolContext, ToolError};
use crate::vectors::insights::InsightEngine;

/// Serialize a value that is known to be `Serialize`, degrading to null.
fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// List detected subscriptions and recurring senders.
///
/// The raw `List-Unsubscribe` / `List-Unsubscribe-Post` header values are
/// deliberately withheld: one-click unsubscribe URLs embed per-recipient
/// tokens and act as capability URLs. Callers get `has_unsubscribe` so they
/// can still reason about which senders are unsubscribable.
pub async fn list_subscriptions(
    ctx: Arc<ToolContext>,
    req: ListSubscriptionsRequest,
) -> Result<Value, ToolError> {
    let limit = validate_limit(req.limit.unwrap_or(50), 200) as usize;
    let min_email_count = req.min_email_count.unwrap_or(0) as u64;

    // Compare on the rendered category name so callers use the same
    // vocabulary the payload reports back.
    let category = req.category.map(|c| c.to_ascii_lowercase());
    if let Some(ref c) = category {
        const VALID: [&str; 6] = [
            "newsletter",
            "marketing",
            "notification",
            "receipt",
            "social",
            "unknown",
        ];
        if !VALID.contains(&c.as_str()) {
            return Err(ToolError::Invalid(format!(
                "Unknown category '{c}'. Expected one of: {}",
                VALID.join(", ")
            )));
        }
    }

    let engine = InsightEngine::new(ctx.db.clone(), ctx.vectors()?.store.clone());

    let subs = engine
        .detect_subscriptions()
        .await
        .map_err(|e| super::db_error("Subscription detection", e))?;

    let total_detected = subs.len();

    let items: Vec<Value> = subs
        .iter()
        .filter(|s| s.email_count >= min_email_count)
        .filter(|s| match &category {
            Some(c) => s.category.to_string() == *c,
            None => true,
        })
        .take(limit)
        .map(|s| {
            json!({
                "sender_address": s.sender_address,
                "sender_domain": s.sender_domain,
                "frequency": to_value(&s.frequency),
                "email_count": s.email_count,
                "first_seen": s.first_seen.to_rfc3339(),
                "last_seen": s.last_seen.to_rfc3339(),
                "has_unsubscribe": s.has_unsubscribe,
                "category": to_value(&s.category),
                "suggested_action": to_value(&s.suggested_action),
                "read_rate": s.read_rate,
            })
        })
        .collect();

    Ok(json!({
        "count": items.len(),
        "total_detected": total_detected,
        "subscriptions": items,
    }))
}

/// List discovered topic clusters, largest first.
///
/// The centroid vector is never included — it is large, opaque, and of no use
/// to a model.
pub async fn list_clusters(
    ctx: Arc<ToolContext>,
    req: ListClustersRequest,
) -> Result<Value, ToolError> {
    let limit = validate_limit(req.limit.unwrap_or(20), 100) as usize;
    let include_representatives = req.include_representatives.unwrap_or(true);

    let mut clusters = ctx.vectors()?.cluster_engine.get_clusters().await;
    let total = clusters.len();
    clusters.sort_by_key(|c| std::cmp::Reverse(c.email_count));
    clusters.truncate(limit);

    // One batched lookup for every representative across all clusters, rather
    // than a query per cluster.
    let meta = if include_representatives {
        let ids: Vec<String> = clusters
            .iter()
            .flat_map(|c| c.representative_email_ids.iter().cloned())
            .collect();
        super::emails::fetch_email_meta(&ctx, &ids).await
    } else {
        std::collections::HashMap::new()
    };

    let items: Vec<Value> = clusters
        .iter()
        .map(|c| {
            let terms: Vec<Value> = c
                .top_terms
                .iter()
                .map(|t| json!({ "word": t.word, "score": t.score, "count": t.count }))
                .collect();

            let mut entry = json!({
                "id": c.id,
                "name": c.name,
                "description": c.description,
                "email_count": c.email_count,
                "stability_score": c.stability_score,
                "stability_runs": c.stability_runs,
                "is_pinned": c.is_pinned,
                "top_terms": terms,
                "created_at": c.created_at.to_rfc3339(),
                "updated_at": c.updated_at.to_rfc3339(),
            });

            if include_representatives {
                let reps: Vec<Value> = c
                    .representative_email_ids
                    .iter()
                    .filter_map(|id| {
                        meta.get(id).map(|(subject, from_addr, from_name, _, _)| {
                            json!({
                                "id": id,
                                "subject": subject,
                                "from_addr": from_addr,
                                "from_name": from_name,
                            })
                        })
                    })
                    .collect();
                entry["representative_emails"] = Value::Array(reps);
            }

            entry
        })
        .collect();

    Ok(json!({ "count": items.len(), "total": total, "clusters": items }))
}

/// Report SONA learning and feedback metrics.
///
/// These counters are process-local and reset on restart — they are not
/// historical totals.
pub async fn get_learning_metrics(
    ctx: Arc<ToolContext>,
    _req: GetLearningMetricsRequest,
) -> Result<Value, ToolError> {
    let m = ctx.vectors()?.learning_engine.get_metrics().await;

    // Share of clicks that landed on the top-ranked result — the headline
    // relevance signal. Undefined rather than zero when nothing was clicked.
    let rank1_click_rate = if m.total_clicks > 0 {
        Some(m.rank1_clicks as f64 / m.total_clicks as f64)
    } else {
        None
    };

    let drift: Vec<Value> = {
        let mut entries: Vec<(&String, &f32)> = m.centroid_drift.iter().collect();
        entries.sort_by(|a, b| b.1.total_cmp(a.1));
        entries
            .into_iter()
            .map(|(category, drift)| json!({ "category": category, "drift": drift }))
            .collect()
    };

    Ok(json!({
        "total_feedback": m.total_feedback,
        "rank1_clicks": m.rank1_clicks,
        "total_clicks": m.total_clicks,
        "rank1_click_rate": rank1_click_rate,
        "centroid_drift": drift,
        "ab_control_queries": m.ab_control_queries,
        "ab_sona_queries": m.ab_sona_queries,
    }))
}

// ---------------------------------------------------------------------------
// The two pre-existing analytics tools, moved from `mcp/server.rs`
// ---------------------------------------------------------------------------

/// Mailbox analytics, as `get_insights` and `insights://summary` return them.
#[derive(Debug, serde::Serialize)]
pub struct InsightsSummary {
    pub categories: Vec<CategoryCount>,
    pub top_senders: Vec<SenderCount>,
    pub daily_volume: Vec<DailyCount>,
}

#[derive(Debug, serde::Serialize)]
pub struct CategoryCount {
    pub category: String,
    pub count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct SenderCount {
    pub sender: String,
    pub count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct DailyCount {
    pub date: String,
    pub count: i64,
}

/// A `label`/`count` pair — categories and daily buckets share this shape.
#[derive(Debug, sqlx::FromRow)]
struct LabelCountRow {
    label: String,
    count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SenderCountRow {
    sender: String,
    count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct RuleRow {
    id: String,
    name: String,
    conditions_json: String,
    actions_json: String,
    enabled: i64,
}

/// Category counts, top senders, and the last seven days of volume.
///
/// The `#[tool]` version dropped query errors on the floor and reported an
/// empty mailbox when the database was unreachable. Errors propagate now, so
/// the A5 resource layer can map a failure to `internal_error` instead of
/// serving a confidently wrong summary.
pub async fn fetch_insights(ctx: &ToolContext) -> Result<InsightsSummary, ToolError> {
    let pool = ctx.pool();

    let categories = sqlx::query_as::<_, LabelCountRow>(
        "SELECT category as label, COUNT(*) as count FROM emails \
         GROUP BY category ORDER BY count DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| super::db_error("Category counts", e))?;

    let senders = sqlx::query_as::<_, SenderCountRow>(
        "SELECT COALESCE(from_name, from_addr) as sender, COUNT(*) as count \
         FROM emails GROUP BY sender ORDER BY count DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| super::db_error("Top senders", e))?;

    let daily = sqlx::query_as::<_, LabelCountRow>(
        "SELECT DATE(received_at) as label, COUNT(*) as count FROM emails \
         WHERE received_at >= datetime('now', '-7 days') \
         GROUP BY DATE(received_at) ORDER BY label DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| super::db_error("Daily volume", e))?;

    Ok(InsightsSummary {
        categories: categories
            .into_iter()
            .map(|r| CategoryCount {
                category: r.label,
                count: r.count,
            })
            .collect(),
        top_senders: senders
            .into_iter()
            .map(|r| SenderCount {
                sender: r.sender,
                count: r.count,
            })
            .collect(),
        daily_volume: daily
            .into_iter()
            .map(|r| DailyCount {
                date: r.label,
                count: r.count,
            })
            .collect(),
    })
}

/// Email analytics for the last seven days.
pub async fn get_insights(
    ctx: Arc<ToolContext>,
    _req: GetInsightsRequest,
) -> Result<Value, ToolError> {
    Ok(json!(fetch_insights(&ctx).await?))
}

/// Every configured email rule with its conditions, actions and status.
pub async fn list_rules(ctx: Arc<ToolContext>, _req: ListRulesRequest) -> Result<Value, ToolError> {
    let rows = sqlx::query_as::<_, RuleRow>(
        "SELECT id, name, conditions_json, actions_json, enabled FROM rules ORDER BY name",
    )
    .fetch_all(ctx.pool())
    .await
    .map_err(|e| super::db_error("Rule listing", e))?;

    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.name,
                "conditions": r.conditions_json,
                "actions": r.actions_json,
                "is_active": r.enabled != 0,
            })
        })
        .collect();

    Ok(json!({ "count": items.len(), "rules": items }))
}
