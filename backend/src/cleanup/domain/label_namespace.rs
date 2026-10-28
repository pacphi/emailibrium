//! `Kind/`, `Topic/`, `State/` label namespaces (docs/plan/mail-agent-2026-10.md
//! §7), wired through the existing ADR-018 label plumbing
//! (`email::labels::LabelManagerService`), which already accepts arbitrary
//! label strings — no change to that abstraction was needed, only the
//! namespace convention and the adapter from a `kind::Classification` to
//! the label strings it implies.
//!
//! `Topic/` itself isn't assigned here: topic gating (the ≥200-message/
//! ≥3-sender promotion threshold) is milestone 4. `State/` is written only
//! by the user per the plan — the one exception, `State/New` as a proposed
//! default for first-contact humans, is exposed as a *candidate* label a
//! caller may choose to apply, never applied automatically by this module.

use super::kind::{Classification, Kind};

/// `State/` values. The plan: "State is written only by the user" — this
/// module never calls `LabelManagerService::apply_labels` on its own
/// initiative; it only tells a caller what the Kind classification implies.
pub const STATE_ENROLLED: &str = "State/Enrolled";
pub const STATE_REVIEW: &str = "State/Review";
pub const STATE_READ_LATER: &str = "State/ReadLater";
pub const STATE_DONE: &str = "State/Done";
pub const STATE_NEW: &str = "State/New";

/// The namespaced `Kind/...` label a classification implies, always
/// present — every `Classification` has exactly one `Kind` label.
pub fn kind_label(classification: &Classification) -> &'static str {
    match (classification.kind, classification.is_broadcast) {
        (Kind::People, true) => "Kind/People/Broadcast",
        _ => classification.kind.label(),
    }
}

/// The full set of labels a classification implies: the `Kind/` label
/// always, plus `State/New` when the classifier proposes it (a candidate,
/// not an instruction — see module docs).
pub fn labels_for(classification: &Classification) -> Vec<String> {
    let mut labels = vec![kind_label(classification).to_string()];
    if classification.propose_state_new {
        labels.push(STATE_NEW.to_string());
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::kind::GithubReason;

    #[test]
    fn simple_kind_has_one_label() {
        let c = Classification {
            kind: Kind::Notifications,
            github_reason: None,
            is_broadcast: false,
            propose_state_new: false,
        };
        assert_eq!(labels_for(&c), vec!["Kind/Notifications".to_string()]);
    }

    #[test]
    fn broadcast_people_uses_the_broadcast_label() {
        let c = Classification {
            kind: Kind::People,
            github_reason: None,
            is_broadcast: true,
            propose_state_new: false,
        };
        assert_eq!(kind_label(&c), "Kind/People/Broadcast");
    }

    #[test]
    fn first_contact_human_proposes_state_new() {
        let c = Classification {
            kind: Kind::People,
            github_reason: None,
            is_broadcast: false,
            propose_state_new: true,
        };
        let labels = labels_for(&c);
        assert_eq!(labels, vec!["Kind/People".to_string(), STATE_NEW.to_string()]);
    }

    #[test]
    fn github_reason_does_not_add_a_label_itself() {
        // The reason drives the attention brief (milestone 2+), not a
        // label — Kind/Notifications is still the only label implied.
        let c = Classification {
            kind: Kind::Notifications,
            github_reason: Some(GithubReason::NeedsYou),
            is_broadcast: false,
            propose_state_new: false,
        };
        assert_eq!(labels_for(&c), vec!["Kind/Notifications".to_string()]);
    }
}
