//! Request parameter structs for the read-only tools.
//!
//! These carry `schemars::JsonSchema` so the registry can publish an MCP
//! input schema for each tool. Optional fields are genuinely optional: every
//! handler applies its own default so an empty argument object is always a
//! valid call.

use rmcp::schemars;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListAccountsRequest {}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetSyncStatusRequest {
    #[schemars(
        description = "Restrict per-account sync state to this account UUID. Omit for all accounts."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListSubscriptionsRequest {
    #[schemars(description = "Maximum number of subscriptions to return (default: 50, max: 200)")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Filter to one category: newsletter, marketing, notification, receipt, social, or unknown"
    )]
    pub category: Option<String>,

    #[schemars(description = "Only include senders with at least this many emails")]
    pub min_email_count: Option<u32>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListClustersRequest {
    #[schemars(description = "Maximum number of clusters to return (default: 20, max: 100)")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Include representative email metadata for each cluster (default: true)"
    )]
    pub include_representatives: Option<bool>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetLearningMetricsRequest {}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FindSimilarEmailsRequest {
    #[schemars(description = "Email ID to find semantically similar emails for")]
    pub email_id: String,

    #[schemars(description = "Maximum number of similar emails to return (default: 10, max: 50)")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Minimum similarity score in 0.0..=1.0 to include a result (default: 0.5)"
    )]
    pub min_score: Option<f32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListAttachmentsRequest {
    #[schemars(description = "Email ID whose attachments to list")]
    pub email_id: String,

    #[schemars(
        description = "Include inline (CID) attachments such as embedded images (default: false)"
    )]
    pub include_inline: Option<bool>,
}

// ---------------------------------------------------------------------------
// preview_cleanup_plan
// ---------------------------------------------------------------------------

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PreviewCleanupPlanRequest {
    #[schemars(
        description = "Account UUIDs to scope the preview to. Empty means every connected account."
    )]
    #[serde(default)]
    pub account_ids: Vec<String>,

    #[schemars(description = "Senders to preview an unsubscribe operation for")]
    #[serde(default)]
    pub unsubscribe_senders: Vec<SubscriptionSelectionArg>,

    #[schemars(description = "Per-cluster archive/delete/label choices to preview")]
    #[serde(default)]
    pub cluster_actions: Vec<ClusterSelectionArg>,

    #[schemars(description = "Rules to evaluate (evaluate-only; rules are never applied)")]
    #[serde(default)]
    pub rule_selections: Vec<RuleSelectionArg>,

    #[schemars(
        description = "Global archive cutoff: one of olderThan30d, olderThan90d, olderThan1y, custom"
    )]
    pub archive_strategy: Option<String>,

    #[schemars(
        description = "Owner the in-memory plan is built for. Letters, digits, '-' and '_' only."
    )]
    pub user_id: String,

    #[schemars(
        description = "How many sample operations to include in the response (default: 10, max: 50)"
    )]
    pub sample_limit: Option<u32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SubscriptionSelectionArg {
    #[schemars(description = "Sender address to unsubscribe from")]
    pub sender: String,

    #[schemars(description = "Account UUID this sender belongs to")]
    pub account_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ClusterSelectionArg {
    #[schemars(description = "Cluster ID to act on")]
    pub cluster_id: String,

    #[schemars(description = "Account UUID this cluster's emails belong to")]
    pub account_id: String,

    #[schemars(description = "One of: archive, deleteSoft, deletePermanent, label")]
    pub action: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RuleSelectionArg {
    #[schemars(description = "Rule ID to evaluate")]
    pub rule_id: String,

    #[schemars(description = "Account UUID to evaluate the rule against")]
    pub account_id: String,
}

// ---------------------------------------------------------------------------
// The seven pre-existing tools, moved from `mcp/tools/email.rs`
// ---------------------------------------------------------------------------
//
// Field names, defaults and `schemars` descriptions are carried over verbatim:
// the published JSON Schema must not change when a tool moves off the `#[tool]`
// macro and onto the registry.

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchEmailsRequest {
    #[schemars(description = "Search query text")]
    pub query: String,

    #[schemars(description = "Maximum number of results to return (default: 20)")]
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    20
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetEmailRequest {
    #[schemars(description = "Unique email identifier")]
    pub email_id: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRecentEmailsRequest {
    #[schemars(description = "Maximum number of recent emails to return (default: 20, max: 100)")]
    pub limit: Option<u32>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
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

/// Parameterless tools still need a request type so the registry can publish an
/// (empty) input schema for them.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetInsightsRequest {}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRulesRequest {}
