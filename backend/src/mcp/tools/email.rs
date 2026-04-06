//! Email-domain types for MCP tools (ADR-028 Phase 5).
//!
//! Contains request parameter structs (with `schemars::JsonSchema` for MCP
//! schema generation) and `sqlx::FromRow` row types used by the tool
//! implementations in `mcp::server`.

use rmcp::schemars;

// ---------------------------------------------------------------------------
// Input validation helpers (ADR-028 Phase 6)
// ---------------------------------------------------------------------------

pub fn validate_email_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 200 {
        return Err("Invalid email ID".to_string());
    }
    Ok(())
}

pub fn validate_query(query: &str) -> Result<(), String> {
    if query.len() > 1000 {
        return Err("Query too long (max 1000 characters)".to_string());
    }
    Ok(())
}

pub fn validate_date(date: &str) -> Result<(), String> {
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(format!("Invalid date format: {date}. Expected YYYY-MM-DD"));
    }
    Ok(())
}

pub fn validate_limit(limit: u32, max: u32) -> u32 {
    limit.min(max).max(1)
}

// ---------------------------------------------------------------------------
// Tool parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchEmailsRequest {
    #[schemars(description = "Search query text")]
    pub query: String,

    #[schemars(description = "Maximum number of results to return (default: 20)")]
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetEmailRequest {
    #[schemars(description = "Unique email identifier")]
    pub email_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRecentEmailsRequest {
    #[schemars(description = "Maximum number of recent emails to return (default: 20, max: 100)")]
    pub limit: Option<u32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CountEmailsRequest {
    #[schemars(description = "Filter by sender email address (partial match)")]
    pub from_filter: Option<String>,

    #[schemars(description = "Filter by email category")]
    pub category: Option<String>,

    #[schemars(description = "Only count emails received after this ISO 8601 date")]
    pub after: Option<String>,

    #[schemars(description = "Only count emails received before this ISO 8601 date")]
    pub before: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetEmailThreadRequest {
    #[schemars(description = "Email ID whose conversation thread to retrieve")]
    pub email_id: String,
}

// ---------------------------------------------------------------------------
// sqlx row types (Send-safe for use across .await points)
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
pub struct EmailRow {
    pub id: String,
    pub subject: String,
    pub from_name: Option<String>,
    pub from_addr: String,
    pub received_at: String,
    pub body_text: Option<String>,
    pub category: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ThreadEmailRow {
    pub id: String,
    pub subject: String,
    pub from_name: Option<String>,
    pub from_addr: String,
    pub received_at: String,
    pub category: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct InsightsRow {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SenderRow {
    pub sender: String,
    pub count: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct RuleRow {
    pub id: String,
    pub name: String,
    pub conditions_json: String,
    pub actions_json: String,
    pub enabled: i64,
}
