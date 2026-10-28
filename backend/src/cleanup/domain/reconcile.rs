//! Reconcile mode (docs/plan/mail-agent-2026-10.md section 7): mapping an
//! account's existing, pre-chief-of-staff label taxonomy onto
//! `Kind/`/`Topic/`/`State/` without losing anything. Pure detection and
//! mapping logic — the live reconcile run (applying the mapping, deleting
//! empties only after counts reconcile) is milestone-5+ orchestration work;
//! this module decides *what* the mapping would be.

use std::collections::HashMap;

/// One label as it exists today, with enough shape to classify its
/// generation.
#[derive(Debug, Clone)]
pub struct ExistingLabel {
    pub name: String,
    pub message_count: u64,
    /// Older label-ID "generations" tend to be flatter/broader; newer ones
    /// narrower. The caller supplies this from whatever the provider
    /// exposes (Gmail label ID creation order, IMAP folder creation time) —
    /// this module only classifies, it doesn't compute age itself.
    pub relative_age_rank: u32,
}

/// What an existing label maps onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapping {
    /// A flat legacy topic that maps onto exactly one new `Topic/` value.
    FlatTopic(String),
    /// A catch-all that must split by header into more than one `Kind`/
    /// `Topic` — the header rule itself is milestone 1's `kind::classify`;
    /// this just flags that the label needs that treatment, not a single
    /// rename.
    SplitByHeader,
    /// `Org/*`-shaped — folds into the sender key, not a Topic at all
    /// (section 7: "The `Org/` axis is retired: the sender key already is
    /// the organisation").
    FoldIntoSender,
    /// No messages left under it — safe to delete immediately, no mapping
    /// needed.
    Empty,
    /// Not recognized by any of the known legacy generations; left alone
    /// rather than guessed at ("never create a fourth scheme without
    /// mapping the other three").
    Unrecognized,
}

/// The known flat-topic renames observed across the three measured
/// accounts (section 7). Account-specific vocabularies extend this; it is
/// not an exhaustive global list.
fn known_flat_topic_rename(name: &str) -> Option<&'static str> {
    match name {
        "Developer" => Some("OpenSource"),
        "$Finance" => Some("Finance"),
        "Family" => Some("Household"),
        _ => None,
    }
}

/// The known catch-all labels that must split by header rather than rename
/// 1:1 (section 7: "Newsletters -> Subscriptions + Notifications").
fn is_known_catch_all(name: &str) -> bool {
    matches!(name, "Newsletters")
}

fn is_org_shaped(name: &str) -> bool {
    name == "Org" || name.starts_with("Org/")
}

/// Classify one existing label's mapping.
pub fn classify_mapping(label: &ExistingLabel) -> Mapping {
    if label.message_count == 0 {
        return Mapping::Empty;
    }
    if is_org_shaped(&label.name) {
        return Mapping::FoldIntoSender;
    }
    if is_known_catch_all(&label.name) {
        return Mapping::SplitByHeader;
    }
    if let Some(new_name) = known_flat_topic_rename(&label.name) {
        return Mapping::FlatTopic(new_name.to_string());
    }
    Mapping::Unrecognized
}

/// A full reconcile plan for one account: every existing label's mapping,
/// keyed by its current name.
pub fn plan_reconcile(labels: &[ExistingLabel]) -> HashMap<String, Mapping> {
    labels
        .iter()
        .map(|l| (l.name.clone(), classify_mapping(l)))
        .collect()
}

/// Whether a migrated label is safe to delete: only once the new scheme's
/// count for what it mapped to matches what the old label held (section 7:
/// "delete ... migrated labels only after counts reconcile").
pub fn migrated_label_is_safe_to_delete(old_count: u64, new_scheme_count: u64) -> bool {
    new_scheme_count >= old_count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, count: u64, age_rank: u32) -> ExistingLabel {
        ExistingLabel {
            name: name.to_string(),
            message_count: count,
            relative_age_rank: age_rank,
        }
    }

    #[test]
    fn empty_labels_map_to_empty_regardless_of_name() {
        assert_eq!(classify_mapping(&label("Developer", 0, 1)), Mapping::Empty);
    }

    #[test]
    fn known_flat_topics_rename_1_to_1() {
        assert_eq!(
            classify_mapping(&label("Developer", 7455, 2)),
            Mapping::FlatTopic("OpenSource".to_string())
        );
        assert_eq!(
            classify_mapping(&label("$Finance", 10, 2)),
            Mapping::FlatTopic("Finance".to_string())
        );
        assert_eq!(
            classify_mapping(&label("Family", 10, 2)),
            Mapping::FlatTopic("Household".to_string())
        );
    }

    #[test]
    fn newsletters_catch_all_splits_by_header_rather_than_renaming() {
        assert_eq!(
            classify_mapping(&label("Newsletters", 16773, 1)),
            Mapping::SplitByHeader
        );
    }

    #[test]
    fn org_shaped_labels_fold_into_sender() {
        assert_eq!(
            classify_mapping(&label("Org", 80, 3)),
            Mapping::FoldIntoSender
        );
        assert_eq!(
            classify_mapping(&label("Org/Acme", 12, 3)),
            Mapping::FoldIntoSender
        );
    }

    #[test]
    fn an_unknown_label_is_left_alone_not_guessed_at() {
        assert_eq!(
            classify_mapping(&label("MyWeirdCustomLabel", 40, 1)),
            Mapping::Unrecognized
        );
    }

    #[test]
    fn a_full_account_plan_covers_every_label() {
        let labels = vec![
            label("Developer", 7455, 2),
            label("Newsletters", 16773, 1),
            label("Org/Acme", 12, 3),
            label("Stray", 0, 4),
        ];
        let plan = plan_reconcile(&labels);
        assert_eq!(plan.len(), 4);
        assert_eq!(
            plan["Developer"],
            Mapping::FlatTopic("OpenSource".to_string())
        );
        assert_eq!(plan["Stray"], Mapping::Empty);
    }

    #[test]
    fn a_migrated_label_is_not_deletable_until_the_new_scheme_catches_up() {
        assert!(!migrated_label_is_safe_to_delete(100, 99));
        assert!(migrated_label_is_safe_to_delete(100, 100));
        assert!(migrated_label_is_safe_to_delete(100, 101));
    }
}
