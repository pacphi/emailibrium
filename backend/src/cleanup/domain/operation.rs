//! Value types for the Cleanup Planning subdomain.
//!
//! These mirror DDD-008 addendum §Aggregates §3 verbatim. All wire-bound
//! structs use `#[serde(rename_all = "camelCase")]` to match the rest of
//! Emailibrium's API surface (the codebase does NOT have a snake_case →
//! camelCase transformer; serde does the work directly).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provider + folder/label vocabulary (mirrors ADR-018 / DDD-008)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Gmail,
    Outlook,
    Imap,
    Pop3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveKind {
    Folder,
    Label,
}

/// A provider-uniform reference to a folder or label, in DDD-008 vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOrLabel {
    pub id: String,
    pub name: String,
    pub kind: MoveKind,
}

// ---------------------------------------------------------------------------
// Plan-level enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// Risk-max parameter for the apply endpoint (Phase C). Lives in Phase A
/// because the schema and API enums need to be in place from the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskMax {
    Low,
    Medium,
    High,
}

impl RiskMax {
    pub fn includes(self, level: RiskLevel) -> bool {
        matches!(
            (self, level),
            (RiskMax::Low, RiskLevel::Low)
                | (RiskMax::Medium, RiskLevel::Low | RiskLevel::Medium)
                | (RiskMax::High, _)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanStatus {
    Draft,
    Ready,
    Applying,
    Applied,
    PartiallyApplied,
    Failed,
    Expired,
    Cancelled,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::PartiallyApplied => "partially_applied",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "ready" => Some(Self::Ready),
            "applying" => Some(Self::Applying),
            "applied" => Some(Self::Applied),
            "partially_applied" => Some(Self::PartiallyApplied),
            "failed" => Some(Self::Failed),
            "expired" => Some(Self::Expired),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    Pending,
    Applied,
    Failed,
    Skipped,
}

impl OperationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

/// Reasons a row may be `Skipped`. Phase A defines all four so Phase C does
/// not have to migrate the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    StateDrift,
    Unacknowledged,
    Dedup,
    UserCancelled,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StateDrift => "state_drift",
            Self::Unacknowledged => "unacknowledged",
            Self::Dedup => "dedup",
            Self::UserCancelled => "user_cancelled",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "state_drift" => Some(Self::StateDrift),
            "unacknowledged" => Some(Self::Unacknowledged),
            "dedup" => Some(Self::Dedup),
            "user_cancelled" => Some(Self::UserCancelled),
            _ => None,
        }
    }
}

/// Lifecycle state of a predicate row (Phase A stores the column; Phase C
/// drives the transitions during expansion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PredicateStatus {
    Pending,
    Expanding,
    Expanded,
    Applied,
    PartiallyApplied,
    Failed,
    Skipped,
}

impl PredicateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Expanding => "expanding",
            Self::Expanded => "expanded",
            Self::Applied => "applied",
            Self::PartiallyApplied => "partially_applied",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    Finished,
    Cancelled,
    Failed,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Actions, sources, predicates
// ---------------------------------------------------------------------------

/// Provider-uniform action. Distinct from `rules::types::RuleAction`
/// (per-email command vocabulary). PlanBuilder translates between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlanAction {
    Archive,
    AddLabel { kind: MoveKind },
    Move { kind: MoveKind },
    Delete { permanent: bool },
    Unsubscribe { method: UnsubscribeMethodKind },
    MarkRead,
    Star { on: bool },
}

impl PlanAction {
    pub fn requires_target(&self) -> bool {
        matches!(self, Self::AddLabel { .. } | Self::Move { .. })
    }
}

/// Subset of `email::unsubscribe::UnsubscribeMethod` carried as a kind so we
/// can persist + classify risk without storing URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnsubscribeMethodKind {
    ListUnsubscribePost,
    Mailto,
    WebLink,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClusterAction {
    Archive,
    DeleteSoft,
    DeletePermanent,
    Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveStrategy {
    OlderThan30d,
    OlderThan90d,
    OlderThan1y,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlanSource {
    Subscription {
        sender: String,
    },
    Cluster {
        cluster_id: String,
        cluster_action: ClusterAction,
    },
    Rule {
        rule_id: String,
        match_basis: String, // "literal" | "semantic" | "hybrid"
    },
    ArchiveStrategy {
        strategy: ArchiveStrategy,
    },
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PredicateKind {
    Rule,
    ArchiveStrategy,
    LabelFilter,
}

impl PredicateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::ArchiveStrategy => "archive_strategy",
            Self::LabelFilter => "label_filter",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "rule" => Some(Self::Rule),
            "archive_strategy" => Some(Self::ArchiveStrategy),
            "label_filter" => Some(Self::LabelFilter),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Reverse op (Phase E will execute it; Phase A only stores it)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReverseOp {
    AddLabel {
        kind: MoveKind,
        target: FolderOrLabel,
    },
    RemoveLabel {
        kind: MoveKind,
        target: FolderOrLabel,
    },
    MoveBack {
        kind: MoveKind,
        target: FolderOrLabel,
    },
    /// Marks a row whose action is irreversible by protocol (POP3 delete,
    /// permanent delete, web-link unsubscribe, etc.).
    Irreversible,
}

// ---------------------------------------------------------------------------
// Drift / etag
// ---------------------------------------------------------------------------

/// Per-account opaque snapshot identifier. Stored as JSON in
/// `cleanup_plan_account_etags.etag_value`; the discriminator
/// `etag_kind` lives in the same row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AccountStateEtag {
    GmailHistory {
        history_id: String,
    },
    OutlookDelta {
        delta_token: String,
    },
    ImapUvms {
        uidvalidity: u32,
        highest_modseq: u64,
    },
    None,
}

impl AccountStateEtag {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::GmailHistory { .. } => "gmail_history",
            Self::OutlookDelta { .. } => "outlook_delta",
            Self::ImapUvms { .. } => "imap_uvms",
            Self::None => "none",
        }
    }
}

// ---------------------------------------------------------------------------
// Errors / warnings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCode {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlanWarning {
    LargeGroup {
        source: PlanSource,
        projected_count: u64,
    },
    TargetConflict {
        account_id: String,
        email_id: String,
        sources: Vec<PlanSource>,
    },
    PlanExceedsThreshold {
        total_count: u64,
    },
    LowConfidence {
        rule_id: String,
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Email reference — small struct used by ports/builder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailRef {
    pub id: String,
    pub account_id: String,
}

// ---------------------------------------------------------------------------
// PlannedOperation (the row-level type)
// ---------------------------------------------------------------------------

/// A materialized per-message operation row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedOperationRow {
    pub seq: u64,
    pub account_id: String,
    /// Some(email_id) for message-level rows; None for sender-level rows
    /// (e.g., `Unsubscribe`).
    pub email_id: Option<String>,
    pub action: PlanAction,
    pub source: PlanSource,
    pub target: Option<FolderOrLabel>,
    pub reverse_op: Option<ReverseOp>,
    pub risk: RiskLevel,
    pub status: OperationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<SkipReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
}

/// A predicate operation that defers per-email materialization to apply time.
///
/// Predicate rows keep their original `seq` for the lifetime of the plan.
/// Expanded children, written by Phase C's `expand_predicate`, get
/// `seq > max(plan.seq)` at expansion time. Phase B's per-row High
/// acknowledgements track the *parent* predicate's seq, never the children's.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedOperationPredicate {
    /// Stable, never re-issued. Children get higher seq values at expansion.
    pub seq: u64,
    pub account_id: String,
    pub predicate_kind: PredicateKind,
    pub predicate_id: String,
    pub action: PlanAction,
    pub target: Option<FolderOrLabel>,
    pub source: PlanSource,
    /// Estimated message count at build time. Authoritative count is the sum
    /// of materialized child rows produced at expansion time.
    pub projected_count: u64,
    /// 5-20 representative ids for UI preview. Deterministic.
    pub sample_email_ids: Vec<String>,
    pub risk: RiskLevel,
    pub status: PredicateStatus,
    pub partial_applied_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "opKind", rename_all = "camelCase")]
pub enum PlannedOperation {
    Materialized(PlannedOperationRow),
    Predicate(PlannedOperationPredicate),
}

impl PlannedOperation {
    pub fn seq(&self) -> u64 {
        match self {
            Self::Materialized(r) => r.seq,
            Self::Predicate(p) => p.seq,
        }
    }

    pub fn account_id(&self) -> &str {
        match self {
            Self::Materialized(r) => &r.account_id,
            Self::Predicate(p) => &p.account_id,
        }
    }

    pub fn risk(&self) -> RiskLevel {
        match self {
            Self::Materialized(r) => r.risk,
            Self::Predicate(p) => p.risk,
        }
    }

    pub fn action(&self) -> &PlanAction {
        match self {
            Self::Materialized(r) => &r.action,
            Self::Predicate(p) => &p.action,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_serde_camelcase() {
        let json = serde_json::to_string(&RiskLevel::Low).unwrap();
        assert_eq!(json, "\"low\"");
        let back: RiskLevel = serde_json::from_str("\"high\"").unwrap();
        assert_eq!(back, RiskLevel::High);
    }

    #[test]
    fn risk_max_includes_levels_correctly() {
        assert!(RiskMax::Low.includes(RiskLevel::Low));
        assert!(!RiskMax::Low.includes(RiskLevel::Medium));
        assert!(RiskMax::Medium.includes(RiskLevel::Low));
        assert!(RiskMax::Medium.includes(RiskLevel::Medium));
        assert!(!RiskMax::Medium.includes(RiskLevel::High));
        assert!(RiskMax::High.includes(RiskLevel::High));
    }

    #[test]
    fn plan_action_roundtrip_serde() {
        let cases = vec![
            PlanAction::Archive,
            PlanAction::AddLabel {
                kind: MoveKind::Label,
            },
            PlanAction::Move {
                kind: MoveKind::Folder,
            },
            PlanAction::Delete { permanent: true },
            PlanAction::Unsubscribe {
                method: UnsubscribeMethodKind::ListUnsubscribePost,
            },
            PlanAction::MarkRead,
            PlanAction::Star { on: true },
        ];
        for a in cases {
            let s = serde_json::to_string(&a).unwrap();
            let back: PlanAction = serde_json::from_str(&s).unwrap();
            assert_eq!(back, a);
        }
    }

    #[test]
    fn plan_source_roundtrip_serde() {
        let cases = vec![
            PlanSource::Subscription {
                sender: "news@x.com".to_string(),
            },
            PlanSource::Cluster {
                cluster_id: "c1".to_string(),
                cluster_action: ClusterAction::Archive,
            },
            PlanSource::Rule {
                rule_id: "r1".to_string(),
                match_basis: "literal".to_string(),
            },
            PlanSource::ArchiveStrategy {
                strategy: ArchiveStrategy::OlderThan90d,
            },
            PlanSource::Manual,
        ];
        for s in cases {
            let j = serde_json::to_string(&s).unwrap();
            let back: PlanSource = serde_json::from_str(&j).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn account_state_etag_roundtrip_imap() {
        let etag = AccountStateEtag::ImapUvms {
            uidvalidity: 42,
            highest_modseq: 9_999_999,
        };
        let j = serde_json::to_string(&etag).unwrap();
        // Wire is camelCase per project convention.
        assert!(j.contains("\"highestModseq\":9999999"));
        let back: AccountStateEtag = serde_json::from_str(&j).unwrap();
        assert_eq!(back, etag);
        assert_eq!(etag.kind_str(), "imap_uvms");
    }

    #[test]
    fn account_state_etag_kinds() {
        assert_eq!(
            AccountStateEtag::GmailHistory {
                history_id: "1".into()
            }
            .kind_str(),
            "gmail_history"
        );
        assert_eq!(
            AccountStateEtag::OutlookDelta {
                delta_token: "tok".into()
            }
            .kind_str(),
            "outlook_delta"
        );
        assert_eq!(AccountStateEtag::None.kind_str(), "none");
    }

    #[test]
    fn invariant_target_required_for_move_and_label() {
        assert!(PlanAction::AddLabel {
            kind: MoveKind::Label
        }
        .requires_target());
        assert!(PlanAction::Move {
            kind: MoveKind::Folder
        }
        .requires_target());
        assert!(!PlanAction::MarkRead.requires_target());
        assert!(!PlanAction::Archive.requires_target());
    }

    #[test]
    fn skip_reason_string_roundtrip() {
        for r in [
            SkipReason::StateDrift,
            SkipReason::Unacknowledged,
            SkipReason::Dedup,
            SkipReason::UserCancelled,
        ] {
            let s = r.as_str();
            assert_eq!(SkipReason::from_str_opt(s), Some(r));
        }
    }

    #[test]
    fn plan_status_string_roundtrip() {
        for st in [
            PlanStatus::Draft,
            PlanStatus::Ready,
            PlanStatus::Applying,
            PlanStatus::Applied,
            PlanStatus::PartiallyApplied,
            PlanStatus::Failed,
            PlanStatus::Expired,
            PlanStatus::Cancelled,
        ] {
            assert_eq!(PlanStatus::from_str_opt(st.as_str()), Some(st));
        }
    }
}
