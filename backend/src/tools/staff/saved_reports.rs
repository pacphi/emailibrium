//! Saved reports (docs/plan/mail-agent-2026-10.md section 8: "also saved
//! as a page" for the daily brief, generalised to the Monday review and
//! monthly health report — see `config/tools.yaml`'s own entries for which
//! tools call [`save_report`]).
//!
//! [`save_report`] persists a verbatim copy of a report's JSON response to
//! `saved_reports` (migration 033); `list_saved_reports`/`get_saved_report`
//! are the read-back side, following the same `(ctx, Request) ->
//! Result<Value, ToolError>` shape every other staff tool uses.

use std::sync::Arc;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde_json::{json, Value};

use super::params::{GetSavedReportRequest, ListSavedReportsRequest};
use crate::db::entities::saved_reports;
use crate::tools::readonly::{db_error, validate_uuid};
use crate::tools::{ToolContext, ToolError};

/// Persist one report. `account_id: None` means a merged (all-account)
/// report, matching `saved_reports.account_id`'s own nullability.
pub async fn save_report(
    ctx: &Arc<ToolContext>,
    kind: &str,
    account_id: Option<&str>,
    content: &Value,
) -> Result<(), ToolError> {
    let model = saved_reports::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        kind: Set(kind.to_string()),
        account_id: Set(account_id.map(str::to_string)),
        generated_at: Set(Utc::now().to_rfc3339()),
        content_json: Set(content.to_string()),
    };
    model
        .insert(&ctx.conn())
        .await
        .map_err(|e| ToolError::Database(format!("saving a {kind} report: {e}")))?;
    Ok(())
}

pub async fn list_saved_reports(
    ctx: Arc<ToolContext>,
    req: ListSavedReportsRequest,
) -> Result<Value, ToolError> {
    if let Some(ref id) = req.account_id {
        validate_uuid("account_id", id).map_err(ToolError::Invalid)?;
    }

    let mut query = saved_reports::Entity::find().order_by_desc(saved_reports::Column::GeneratedAt);
    if let Some(ref kind) = req.kind {
        query = query.filter(saved_reports::Column::Kind.eq(kind.clone()));
    }
    if let Some(ref account_id) = req.account_id {
        query = query.filter(saved_reports::Column::AccountId.eq(account_id.clone()));
    }

    let limit = u64::from(req.limit.unwrap_or(20).min(200));
    let rows = query
        .limit(limit)
        .all(&ctx.conn())
        .await
        .map_err(|e| db_error("Listing saved reports", e))?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "kind": r.kind,
                "account_id": r.account_id,
                "generated_at": r.generated_at,
            })
        })
        .collect();

    Ok(json!({ "count": items.len(), "reports": items }))
}

pub async fn get_saved_report(
    ctx: Arc<ToolContext>,
    req: GetSavedReportRequest,
) -> Result<Value, ToolError> {
    let row = saved_reports::Entity::find_by_id(req.report_id.clone())
        .one(&ctx.conn())
        .await
        .map_err(|e| db_error("Loading a saved report", e))?
        .ok_or_else(|| ToolError::Invalid(format!("no saved report with id {}", req.report_id)))?;

    let content: Value = serde_json::from_str(&row.content_json)
        .map_err(|e| ToolError::Database(format!("parsing saved report content: {e}")))?;

    Ok(json!({
        "id": row.id,
        "kind": row.kind,
        "account_id": row.account_id,
        "generated_at": row.generated_at,
        "content": content,
    }))
}
