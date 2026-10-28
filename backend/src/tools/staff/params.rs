//! Request parameter structs for the read-only chief-of-staff tools.

use rmcp::schemars;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DoctorRequest {
    #[schemars(
        description = "Restrict the report to this account UUID. Omit for every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetSnapshotRequest {
    #[schemars(
        description = "The account UUID to snapshot. A snapshot describes one mailbox's shape; pass each account separately to compare them."
    )]
    pub account_id: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct BriefRequest {
    #[schemars(
        description = "Restrict the brief to this account UUID. Omit to merge every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ReviewRequest {
    #[schemars(
        description = "Restrict the review to this account UUID. Omit to merge every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct IdentityMapRequest {}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ScreenerRequest {
    #[schemars(
        description = "Restrict the screener to this account UUID. Omit to merge every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SettingsChecklistRequest {
    #[schemars(
        description = "Restrict the checklist to this account UUID. Omit to merge every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EraArchivePreviewRequest {
    #[schemars(description = "The account UUID to preview the oldest era (>5 years) for.")]
    pub account_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EraArchiveApplyRequest {
    #[schemars(description = "The account UUID this applies to.")]
    pub account_id: String,
    #[schemars(
        description = "The exact batch_key a prior era_archive_preview call returned. A call without the current key is rejected, never silently ignored."
    )]
    pub batch_key: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EraArchiveUndoRequest {
    #[schemars(description = "The account UUID this applies to.")]
    pub account_id: String,
    #[schemars(description = "The batch_key an era_archive_apply call returned.")]
    pub batch_key: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CloseoutPreviewRequest {
    #[schemars(description = "The account UUID to preview a close-out for.")]
    pub account_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CloseoutApplyRequest {
    #[schemars(description = "The account UUID this applies to.")]
    pub account_id: String,
    #[schemars(
        description = "The exact batch_key a prior closeout_preview call returned. A call without the current key is rejected, never silently ignored."
    )]
    pub batch_key: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CloseoutUndoRequest {
    #[schemars(description = "The account UUID this applies to.")]
    pub account_id: String,
    #[schemars(description = "The batch_key a closeout_apply call returned.")]
    pub batch_key: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct HealthRequest {
    #[schemars(
        description = "Restrict the health report to this account UUID. Omit to merge every connected account."
    )]
    pub account_id: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListSavedReportsRequest {
    #[schemars(
        description = "Restrict to this report kind: \"brief\", \"review\", or \"health\". Omit for every kind."
    )]
    pub kind: Option<String>,
    #[schemars(
        description = "Restrict to reports generated for this account UUID. Omit for every account, including merged (all-account) reports."
    )]
    pub account_id: Option<String>,
    #[schemars(
        description = "Maximum number of reports to return, most recent first. Defaults to 20."
    )]
    pub limit: Option<u32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetSavedReportRequest {
    #[schemars(description = "The id a list_saved_reports call returned.")]
    pub report_id: String,
}
