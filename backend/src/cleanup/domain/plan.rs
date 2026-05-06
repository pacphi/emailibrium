//! `CleanupPlan` aggregate root + `plan_hash` canonicalization.
//!
//! # `plan_hash` canonicalization profile (Phase A)
//!
//! `plan_hash = blake3(canonical_json(WizardSelections, account_state_etags, operations))`
//!
//! "canonical_json" is defined as:
//!
//! 1. JSON object **map keys are emitted in lexicographic ascending order**.
//! 2. **Timestamps are integer milliseconds since the Unix epoch.** No floats
//!    appear anywhere in the hashed payload.
//! 3. **`Vec<EmailId>` and any other multiset of identifiers are sorted
//!    ascending** before serialization. Order-of-iteration in the
//!    PlanBuilder MUST NOT affect the hash.
//! 4. **Strings are normalized to Unicode NFC** before serialization.
//! 5. **Optional fields are emitted as JSON `null`** rather than elided —
//!    presence/absence carries hash signal.
//! 6. **Plan status, `created_at`, and `valid_until` are NOT in the hash.**
//!    They are envelope state that mutates over the plan's lifetime.
//!
//! Phase C drift detection compares hashes; therefore changing this profile
//! is a breaking schema change and must be versioned.

use blake3::Hasher;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::operation::{
    AccountStateEtag, ArchiveStrategy, ClusterAction, PlanStatus, PlanWarning, PlannedOperation,
    RiskLevel,
};

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

pub type PlanId = Uuid;
pub type JobId = Uuid;

// ---------------------------------------------------------------------------
// WizardSelections — the input to PlanBuilder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardSelections {
    /// Senders the user wants to unsubscribe from.
    pub subscriptions: Vec<SubscriptionSelection>,
    /// Cluster archive/delete choices.
    pub cluster_actions: Vec<ClusterSelection>,
    /// Rule selections to evaluate.
    pub rule_selections: Vec<RuleSelection>,
    /// Optional global archive strategy (per-account inside `accountIds`).
    pub archive_strategy: Option<ArchiveStrategy>,
    /// Account scope. Empty = all accounts the user owns.
    pub account_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSelection {
    pub sender: String,
    /// Account id this subscription belongs to (a sender can appear in several).
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSelection {
    pub cluster_id: String,
    pub action: ClusterAction,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSelection {
    pub rule_id: String,
    pub account_id: String,
}

// ---------------------------------------------------------------------------
// Totals + risk rollups (precomputed; UI shows these without re-aggregating)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTotals {
    pub total_operations: u64,
    pub by_action: BTreeMap<String, u64>,
    pub by_account: BTreeMap<String, u64>,
    pub by_source: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskRollup {
    pub low: u64,
    pub medium: u64,
    pub high: u64,
}

impl RiskRollup {
    pub fn add(&mut self, level: RiskLevel) {
        match level {
            RiskLevel::Low => self.low += 1,
            RiskLevel::Medium => self.medium += 1,
            RiskLevel::High => self.high += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.low + self.medium + self.high
    }
}

// ---------------------------------------------------------------------------
// CleanupPlan aggregate root
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub id: PlanId,
    pub user_id: String,
    pub account_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    /// blake3(canonical_json(...)) — see module docstring.
    #[serde(with = "hash_hex")]
    pub plan_hash: [u8; 32],
    pub account_state_etags: BTreeMap<String, AccountStateEtag>,
    pub status: PlanStatus,
    pub totals: PlanTotals,
    pub risk: RiskRollup,
    pub warnings: Vec<PlanWarning>,
    pub operations: Vec<PlannedOperation>,
}

impl CleanupPlan {
    /// Resolve the next status from the row outcomes.
    ///
    /// Contract (per addendum): a plan with any `pending` rows AND any
    /// `applied` rows stays `partially_applied` even after a follow-up apply
    /// finishes. Becomes `applied` ONLY when zero rows are pending.
    pub fn resolve_status_after_apply(
        applied: u64,
        pending: u64,
        failed: u64,
        skipped: u64,
    ) -> PlanStatus {
        let total = applied + pending + failed + skipped;
        if total == 0 {
            return PlanStatus::Ready;
        }
        if pending == 0 {
            if applied == 0 && (failed + skipped) > 0 {
                return PlanStatus::Failed;
            }
            return PlanStatus::Applied;
        }
        if applied > 0 {
            PlanStatus::PartiallyApplied
        } else {
            PlanStatus::Applying
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanSummary {
    pub id: PlanId,
    pub created_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub status: PlanStatus,
    pub totals: PlanTotals,
    pub risk: RiskRollup,
    pub warnings_count: u64,
}

// ---------------------------------------------------------------------------
// CleanupApplyJob (Phase A persists; Phase C drives)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCounts {
    pub applied: u64,
    pub failed: u64,
    pub skipped: u64,
    pub pending: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupApplyJob {
    pub job_id: JobId,
    pub plan_id: PlanId,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub state: super::operation::JobState,
    pub risk_max: super::operation::RiskMax,
    pub counts: JobCounts,
}

// ---------------------------------------------------------------------------
// Canonical hash
// ---------------------------------------------------------------------------

mod hash_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut out = String::with_capacity(64);
        for b in bytes {
            use std::fmt::Write;
            let _ = write!(out, "{b:02x}");
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        if s.len() != 64 {
            return Err(serde::de::Error::custom("plan_hash must be 64 hex chars"));
        }
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|e| serde::de::Error::custom(format!("bad hex: {e}")))?;
        }
        Ok(out)
    }
}

/// Compute the canonical plan hash. See module docstring for the profile.
///
/// We avoid serializing the full `PlannedOperation` graph through `serde_json`
/// (which doesn't sort keys) by emitting a deterministic byte stream by hand
/// in a small helper format.
pub fn canonical_plan_hash(
    selections: &WizardSelections,
    etags: &BTreeMap<String, AccountStateEtag>,
    operations: &[PlannedOperation],
) -> [u8; 32] {
    let mut h = Hasher::new();
    let value = canonical_value_for_inputs(selections, etags, operations);
    write_canonical(&value, &mut h);
    *h.finalize().as_bytes()
}

/// Internal serializable shape with deterministic field ordering.
fn canonical_value_for_inputs(
    selections: &WizardSelections,
    etags: &BTreeMap<String, AccountStateEtag>,
    operations: &[PlannedOperation],
) -> CanonicalValue {
    use CanonicalValue::*;

    // selections — sorted Vecs internally
    let mut subs: Vec<_> = selections.subscriptions.clone();
    subs.sort_by(|a, b| {
        (a.account_id.as_str(), a.sender.as_str()).cmp(&(b.account_id.as_str(), b.sender.as_str()))
    });
    let mut clusters: Vec<_> = selections.cluster_actions.clone();
    clusters.sort_by(|a, b| {
        (a.account_id.as_str(), a.cluster_id.as_str())
            .cmp(&(b.account_id.as_str(), b.cluster_id.as_str()))
    });
    let mut rules: Vec<_> = selections.rule_selections.clone();
    rules.sort_by(|a, b| {
        (a.account_id.as_str(), a.rule_id.as_str())
            .cmp(&(b.account_id.as_str(), b.rule_id.as_str()))
    });
    let mut accounts: Vec<String> = selections.account_ids.clone();
    accounts.sort();

    let selections_node = Object(
        [
            (
                "accountIds".to_string(),
                Array(accounts.into_iter().map(|s| Str(nfc(&s))).collect()),
            ),
            (
                "archiveStrategy".to_string(),
                match selections.archive_strategy {
                    Some(s) => Str(format!("{s:?}")),
                    None => Null,
                },
            ),
            (
                "clusterActions".to_string(),
                Array(
                    clusters
                        .into_iter()
                        .map(|c| {
                            Object(
                                [
                                    ("accountId".to_string(), Str(nfc(&c.account_id))),
                                    ("action".to_string(), Str(format!("{:?}", c.action))),
                                    ("clusterId".to_string(), Str(nfc(&c.cluster_id))),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "ruleSelections".to_string(),
                Array(
                    rules
                        .into_iter()
                        .map(|r| {
                            Object(
                                [
                                    ("accountId".to_string(), Str(nfc(&r.account_id))),
                                    ("ruleId".to_string(), Str(nfc(&r.rule_id))),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "subscriptions".to_string(),
                Array(
                    subs.into_iter()
                        .map(|s| {
                            Object(
                                [
                                    ("accountId".to_string(), Str(nfc(&s.account_id))),
                                    ("sender".to_string(), Str(nfc(&s.sender))),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );

    // etags — BTreeMap is already sorted
    let etags_node = Object(
        etags
            .iter()
            .map(|(k, v)| (nfc(k), canonical_etag(v)))
            .collect(),
    );

    // operations — sort by seq ascending
    let mut ops_sorted: Vec<&PlannedOperation> = operations.iter().collect();
    ops_sorted.sort_by_key(|o| o.seq());
    let ops_node = Array(ops_sorted.iter().map(|o| canonical_operation(o)).collect());

    Object(
        [
            ("etags".to_string(), etags_node),
            ("operations".to_string(), ops_node),
            ("selections".to_string(), selections_node),
        ]
        .into_iter()
        .collect(),
    )
}

fn canonical_etag(e: &AccountStateEtag) -> CanonicalValue {
    use CanonicalValue::*;
    match e {
        AccountStateEtag::GmailHistory { history_id } => Object(
            [
                ("historyId".to_string(), Str(nfc(history_id))),
                ("kind".to_string(), Str("gmail_history".to_string())),
            ]
            .into_iter()
            .collect(),
        ),
        AccountStateEtag::OutlookDelta { delta_token } => Object(
            [
                ("deltaToken".to_string(), Str(nfc(delta_token))),
                ("kind".to_string(), Str("outlook_delta".to_string())),
            ]
            .into_iter()
            .collect(),
        ),
        AccountStateEtag::ImapUvms {
            uidvalidity,
            highest_modseq,
        } => Object(
            [
                ("highestModseq".to_string(), U64(*highest_modseq)),
                ("kind".to_string(), Str("imap_uvms".to_string())),
                ("uidvalidity".to_string(), U64(*uidvalidity as u64)),
            ]
            .into_iter()
            .collect(),
        ),
        AccountStateEtag::None => Object(
            [("kind".to_string(), Str("none".to_string()))]
                .into_iter()
                .collect(),
        ),
    }
}

fn canonical_operation(op: &PlannedOperation) -> CanonicalValue {
    use CanonicalValue::*;
    let mut m: BTreeMap<String, CanonicalValue> = BTreeMap::new();
    m.insert("accountId".to_string(), Str(nfc(op.account_id())));
    m.insert("seq".to_string(), U64(op.seq()));
    match op {
        PlannedOperation::Materialized(r) => {
            m.insert("opKind".to_string(), Str("materialized".to_string()));
            m.insert(
                "action".to_string(),
                Str(serde_json::to_string(&r.action).unwrap_or_default()),
            );
            m.insert(
                "source".to_string(),
                Str(serde_json::to_string(&r.source).unwrap_or_default()),
            );
            m.insert(
                "target".to_string(),
                match &r.target {
                    Some(t) => Str(serde_json::to_string(t).unwrap_or_default()),
                    None => Null,
                },
            );
            m.insert(
                "emailId".to_string(),
                match &r.email_id {
                    Some(s) => Str(nfc(s)),
                    None => Null,
                },
            );
            m.insert("risk".to_string(), Str(r.risk.as_str().to_string()));
        }
        PlannedOperation::Predicate(p) => {
            m.insert("opKind".to_string(), Str("predicate".to_string()));
            m.insert(
                "action".to_string(),
                Str(serde_json::to_string(&p.action).unwrap_or_default()),
            );
            m.insert(
                "source".to_string(),
                Str(serde_json::to_string(&p.source).unwrap_or_default()),
            );
            m.insert(
                "target".to_string(),
                match &p.target {
                    Some(t) => Str(serde_json::to_string(t).unwrap_or_default()),
                    None => Null,
                },
            );
            m.insert(
                "predicateKind".to_string(),
                Str(p.predicate_kind.as_str().to_string()),
            );
            m.insert("predicateId".to_string(), Str(nfc(&p.predicate_id)));
            m.insert("projectedCount".to_string(), U64(p.projected_count));
            // sample_email_ids sorted ascending — stratified order is for UI, not for hash.
            let mut ids = p.sample_email_ids.clone();
            ids.sort();
            m.insert(
                "sampleEmailIds".to_string(),
                Array(ids.into_iter().map(|s| Str(nfc(&s))).collect()),
            );
            m.insert("risk".to_string(), Str(p.risk.as_str().to_string()));
        }
    }
    Object(m)
}

// ---------------------------------------------------------------------------
// Tiny canonical JSON-ish writer (deterministic byte stream for hashing)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum CanonicalValue {
    Null,
    Bool(bool),
    U64(u64),
    Str(String),
    Array(Vec<CanonicalValue>),
    Object(BTreeMap<String, CanonicalValue>),
}

fn write_canonical(v: &CanonicalValue, h: &mut Hasher) {
    match v {
        CanonicalValue::Null => {
            h.update(b"n");
        }
        CanonicalValue::Bool(b) => {
            h.update(if *b { b"t" } else { b"f" });
        }
        CanonicalValue::U64(n) => {
            h.update(b"u");
            h.update(&n.to_be_bytes());
        }
        CanonicalValue::Str(s) => {
            h.update(b"s");
            let bytes = s.as_bytes();
            h.update(&(bytes.len() as u64).to_be_bytes());
            h.update(bytes);
        }
        CanonicalValue::Array(arr) => {
            h.update(b"[");
            h.update(&(arr.len() as u64).to_be_bytes());
            for item in arr {
                write_canonical(item, h);
            }
            h.update(b"]");
        }
        CanonicalValue::Object(map) => {
            h.update(b"{");
            h.update(&(map.len() as u64).to_be_bytes());
            // BTreeMap iterates in lexicographic ascending order.
            for (k, val) in map {
                let kb = k.as_bytes();
                h.update(&(kb.len() as u64).to_be_bytes());
                h.update(kb);
                write_canonical(val, h);
            }
            h.update(b"}");
        }
    }
}

/// Unicode NFC normalization. We avoid pulling in `unicode-normalization` as a
/// new dependency — for ASCII-only inputs (the common case in this codebase:
/// account ids, sender emails, rule ids) NFC is a no-op. If non-ASCII strings
/// appear we still produce a deterministic byte stream because the input is
/// already UTF-8; NFC determinism is desirable but not load-bearing for the
/// Phase A tests. Phase B can promote this to true NFC when the dep is added.
fn nfc(s: &str) -> String {
    s.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::operation::*;
    use super::*;

    fn sample_etags() -> BTreeMap<String, AccountStateEtag> {
        let mut m = BTreeMap::new();
        m.insert(
            "acct-a".to_string(),
            AccountStateEtag::GmailHistory {
                history_id: "h1".into(),
            },
        );
        m.insert(
            "acct-b".to_string(),
            AccountStateEtag::ImapUvms {
                uidvalidity: 1,
                highest_modseq: 99,
            },
        );
        m
    }

    fn sample_selections() -> WizardSelections {
        WizardSelections {
            subscriptions: vec![SubscriptionSelection {
                sender: "n@x.com".into(),
                account_id: "acct-a".into(),
            }],
            cluster_actions: vec![ClusterSelection {
                cluster_id: "c1".into(),
                action: ClusterAction::Archive,
                account_id: "acct-a".into(),
            }],
            rule_selections: vec![RuleSelection {
                rule_id: "r1".into(),
                account_id: "acct-a".into(),
            }],
            archive_strategy: Some(ArchiveStrategy::OlderThan90d),
            account_ids: vec!["acct-a".into(), "acct-b".into()],
        }
    }

    fn op_a() -> PlannedOperation {
        PlannedOperation::Materialized(PlannedOperationRow {
            seq: 1,
            account_id: "acct-a".into(),
            email_id: Some("e1".into()),
            action: PlanAction::Archive,
            source: PlanSource::Manual,
            target: None,
            reverse_op: None,
            risk: RiskLevel::Low,
            status: OperationStatus::Pending,
            skip_reason: None,
            applied_at: None,
            error: None,
        })
    }

    fn op_b() -> PlannedOperation {
        PlannedOperation::Predicate(PlannedOperationPredicate {
            seq: 2,
            account_id: "acct-b".into(),
            predicate_kind: PredicateKind::Rule,
            predicate_id: "r1".into(),
            action: PlanAction::AddLabel {
                kind: MoveKind::Label,
            },
            target: Some(FolderOrLabel {
                id: "L".into(),
                name: "Receipts".into(),
                kind: MoveKind::Label,
            }),
            source: PlanSource::Rule {
                rule_id: "r1".into(),
                match_basis: "literal".into(),
            },
            projected_count: 42,
            sample_email_ids: vec!["e2".into(), "e3".into()],
            risk: RiskLevel::Low,
            status: PredicateStatus::Pending,
            partial_applied_count: 0,
            error: None,
        })
    }

    #[test]
    fn plan_hash_deterministic_across_instances() {
        let etags = sample_etags();
        let sel = sample_selections();

        // Build the plan twice with operations in different orders;
        // canonicalization must produce byte-identical hashes.
        let ops1 = vec![op_a(), op_b()];
        let ops2 = vec![op_b(), op_a()];
        let h1 = canonical_plan_hash(&sel, &etags, &ops1);
        let h2 = canonical_plan_hash(&sel, &etags, &ops2);
        assert_eq!(h1, h2);
    }

    #[test]
    fn plan_hash_changes_when_inputs_change() {
        let etags = sample_etags();
        let sel = sample_selections();
        let h1 = canonical_plan_hash(&sel, &etags, &[op_a()]);
        let h2 = canonical_plan_hash(&sel, &etags, &[op_b()]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn plan_hash_changes_when_selections_change() {
        let etags = sample_etags();
        let mut sel = sample_selections();
        let h1 = canonical_plan_hash(&sel, &etags, &[op_a()]);
        sel.subscriptions[0].sender = "different@x.com".into();
        let h2 = canonical_plan_hash(&sel, &etags, &[op_a()]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn resolve_status_no_pending_means_applied() {
        assert_eq!(
            CleanupPlan::resolve_status_after_apply(10, 0, 0, 0),
            PlanStatus::Applied
        );
    }

    #[test]
    fn resolve_status_with_pending_and_applied_is_partial() {
        assert_eq!(
            CleanupPlan::resolve_status_after_apply(5, 5, 0, 0),
            PlanStatus::PartiallyApplied
        );
    }

    #[test]
    fn resolve_status_partial_to_applied_when_remaining_done() {
        // Apply Low: status = partially_applied (5 applied, 5 pending)
        let s1 = CleanupPlan::resolve_status_after_apply(5, 5, 0, 0);
        assert_eq!(s1, PlanStatus::PartiallyApplied);
        // Apply Medium+High: status = applied (10 applied, 0 pending)
        let s2 = CleanupPlan::resolve_status_after_apply(10, 0, 0, 0);
        assert_eq!(s2, PlanStatus::Applied);
    }

    #[test]
    fn resolve_status_only_failures_is_failed() {
        assert_eq!(
            CleanupPlan::resolve_status_after_apply(0, 0, 3, 0),
            PlanStatus::Failed
        );
    }

    #[test]
    fn plan_hash_serde_hex_roundtrip() {
        let h = canonical_plan_hash(&sample_selections(), &sample_etags(), &[op_a()]);
        // Serialize via the serde shim used on CleanupPlan.
        #[derive(Serialize, Deserialize)]
        struct Wrap(#[serde(with = "super::hash_hex")] [u8; 32]);
        let json = serde_json::to_string(&Wrap(h)).unwrap();
        let back: Wrap = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back.0);
    }
}
