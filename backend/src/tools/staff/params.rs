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
