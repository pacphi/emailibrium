use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attention {
    NeedsReply,
    NeedsAction,
    Waiting,
    ReadLater,
    Review,
    Done,
}

impl Attention {
    fn label(self) -> String {
        format!("State/{self:?}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Inbox,
    Filed,
    Sent,
    Drafts,
    Outbox,
    Spam,
    Trash,
    Chat,
}

#[derive(Debug, Clone)]
pub enum Organization {
    Approved(String),
    Individual,
    Held,
}

#[derive(Debug, Clone)]
pub struct MessageFacts {
    pub account: String,
    pub message_id: String,
    pub revision: String,
    pub location: Location,
    pub received_at: DateTime<Utc>,
    pub unread: bool,
    pub current_labels: BTreeSet<String>,
    /// Names resolved from the workflow's verified provider-resource registry.
    /// A matching prefix alone never establishes ownership.
    pub owned_attention_labels: BTreeSet<String>,
    pub organization: Organization,
    pub unresolved_attention: Option<Attention>,
    pub unpaid_bill: bool,
    pub already_urgent: bool,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub message_id: String,
    pub policy_version: String,
    pub topics: Vec<String>,
    pub attention: Attention,
    pub urgent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingPlan {
    pub account: String,
    pub message_id: String,
    pub expected_revision: String,
    pub policy_version: String,
    pub add_labels: BTreeSet<String>,
    pub remove_labels: BTreeSet<String>,
    pub remove_from_inbox: bool,
    pub attention: Attention,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Excluded,
    File(FilingPlan),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("invalid policy definition")]
    InvalidPolicy,
    #[error("account mapping is not approved for this policy version")]
    UnapprovedAccount,
    #[error("proposal identity or policy version mismatch")]
    ProposalMismatch,
    #[error("missing provider message identity or revision")]
    MissingIdentity,
    #[error("unknown, empty or duplicate topic selection")]
    InvalidTopics,
    #[error("invalid organization mapping")]
    InvalidOrganization,
    #[error("invalid tracked obligation state")]
    InvalidObligation,
}

pub struct Policy {
    version: String,
    topics: BTreeSet<String>,
    account_mappings: BTreeMap<String, String>,
}

impl Policy {
    pub fn new(
        version: String,
        topics: Vec<String>,
        account_mappings: BTreeMap<String, String>,
    ) -> Result<Self, PolicyError> {
        if version.trim().is_empty()
            || topics.len() != 25
            || topics.iter().any(|topic| !safe_component(topic))
            || account_mappings
                .iter()
                .any(|(account, version)| account.trim().is_empty() || version.trim().is_empty())
        {
            return Err(PolicyError::InvalidPolicy);
        }
        let topics: BTreeSet<_> = topics.into_iter().collect();
        if topics.len() != 25 {
            return Err(PolicyError::InvalidPolicy);
        }
        Ok(Self {
            version,
            topics,
            account_mappings,
        })
    }

    pub fn plan(
        &self,
        facts: &MessageFacts,
        proposal: &Proposal,
        now: DateTime<Utc>,
    ) -> Result<Outcome, PolicyError> {
        if self.account_mappings.get(&facts.account) != Some(&self.version) {
            return Err(PolicyError::UnapprovedAccount);
        }
        if !matches!(facts.location, Location::Inbox | Location::Filed) {
            return Ok(Outcome::Excluded);
        }
        if facts.message_id.trim().is_empty() || facts.revision.trim().is_empty() {
            return Err(PolicyError::MissingIdentity);
        }
        if proposal.message_id != facts.message_id || proposal.policy_version != self.version {
            return Err(PolicyError::ProposalMismatch);
        }
        let topics: BTreeSet<_> = proposal.topics.iter().collect();
        if topics.is_empty()
            || topics.len() != proposal.topics.len()
            || topics.iter().any(|topic| !self.topics.contains(*topic))
        {
            return Err(PolicyError::InvalidTopics);
        }
        if let Some(state) = facts.unresolved_attention {
            if !matches!(
                state,
                Attention::NeedsReply | Attention::NeedsAction | Attention::Waiting
            ) {
                return Err(PolicyError::InvalidObligation);
            }
        }

        let mut desired: BTreeSet<String> = topics
            .iter()
            .map(|topic| format!("Topic/{topic}"))
            .collect();
        let mut reasons = Vec::new();
        let identity_held = matches!(&facts.organization, Organization::Held);
        match &facts.organization {
            Organization::Approved(name) => {
                if !safe_component(name) {
                    return Err(PolicyError::InvalidOrganization);
                }
                desired.insert(format!("Org/{name}"));
            }
            Organization::Held => reasons.push("organization_review"),
            Organization::Individual => {}
        }

        let urgent = proposal.urgent || facts.already_urgent;
        let ambiguous = facts.ambiguous || identity_held;
        let mut attention = proposal.attention;
        if ambiguous {
            attention = Attention::Review;
            reasons.push("classification_review");
        }
        if facts.unpaid_bill {
            attention = Attention::NeedsAction;
            reasons.push("unpaid_bill_preserved");
        }
        if let Some(state) = facts.unresolved_attention {
            attention = state;
            reasons.push("open_obligation_preserved");
        }
        if urgent {
            desired.insert("Priority/Urgent".into());
            if attention == Attention::Done {
                attention = Attention::NeedsAction;
            }
            reasons.push("urgent_preserved");
        }
        // Received age and current unread state are the only retention clock.
        // An obligation, bill, urgency or unresolved ambiguity always wins.
        if facts.unread
            && now.signed_duration_since(facts.received_at) > chrono::Duration::days(90)
            && !ambiguous
            && !urgent
            && !facts.unpaid_bill
            && facts.unresolved_attention.is_none()
            && matches!(attention, Attention::ReadLater | Attention::Review)
        {
            attention = Attention::Done;
            reasons.push("unread_retention_90_days");
        }
        let state_label = attention.label();
        desired.insert(state_label.clone());
        let known_states: BTreeSet<_> = [
            Attention::NeedsReply,
            Attention::NeedsAction,
            Attention::Waiting,
            Attention::ReadLater,
            Attention::Review,
            Attention::Done,
        ]
        .into_iter()
        .map(Attention::label)
        .collect();
        let remove_labels = facts
            .current_labels
            .iter()
            .filter(|label| {
                facts.owned_attention_labels.contains(*label)
                    && known_states.contains(*label)
                    && **label != state_label
            })
            .cloned()
            .collect();
        if reasons.is_empty() {
            reasons.push("approved_classification");
        }
        Ok(Outcome::File(FilingPlan {
            account: facts.account.clone(),
            message_id: facts.message_id.clone(),
            expected_revision: facts.revision.clone(),
            policy_version: self.version.clone(),
            add_labels: desired.difference(&facts.current_labels).cloned().collect(),
            remove_labels,
            remove_from_inbox: facts.location == Location::Inbox,
            attention,
            reasons,
        }))
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn topics() -> Vec<String> {
        (0..25).map(|i| format!("Topic{i}")).collect()
    }
    fn policy() -> Policy {
        Policy::new(
            "approved-v1".into(),
            topics(),
            BTreeMap::from([("Business".into(), "approved-v1".into())]),
        )
        .unwrap()
    }
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
    fn facts() -> MessageFacts {
        MessageFacts {
            account: "Business".into(),
            message_id: "exact-message".into(),
            revision: "rev-1".into(),
            location: Location::Inbox,
            received_at: now() - Duration::days(3),
            unread: true,
            current_labels: BTreeSet::from([
                "UNREAD".into(),
                "Legacy/Keep".into(),
                "State/Waiting".into(),
                "State/Custom".into(),
            ]),
            owned_attention_labels: BTreeSet::from(["State/Waiting".into()]),
            organization: Organization::Approved("ExampleCo".into()),
            unresolved_attention: None,
            unpaid_bill: false,
            already_urgent: false,
            ambiguous: false,
        }
    }
    fn proposal() -> Proposal {
        Proposal {
            message_id: "exact-message".into(),
            policy_version: "approved-v1".into(),
            topics: vec!["Topic1".into(), "Topic2".into()],
            attention: Attention::ReadLater,
            urgent: false,
        }
    }
    fn file(f: &MessageFacts, p: &Proposal) -> FilingPlan {
        match policy().plan(f, p, now()).unwrap() {
            Outcome::File(plan) => plan,
            Outcome::Excluded => panic!("unexpected exclusion"),
        }
    }

    #[test]
    fn policy_requires_exactly_25_unique_topics() {
        assert!(matches!(
            Policy::new("v".into(), vec!["Only".into(); 25], BTreeMap::new()),
            Err(PolicyError::InvalidPolicy)
        ));
    }
    #[test]
    fn multi_topic_filing_preserves_unread_and_unowned_labels() {
        let p = file(&facts(), &proposal());
        assert!(p.add_labels.contains("Topic/Topic1") && p.add_labels.contains("Topic/Topic2"));
        assert!(p.add_labels.contains("Org/ExampleCo"));
        assert_eq!(p.remove_labels, BTreeSet::from(["State/Waiting".into()]));
        assert!(p.remove_from_inbox);
        assert_eq!(p.expected_revision, "rev-1");
        assert!(!p.remove_labels.contains("UNREAD") && !p.remove_labels.contains("Legacy/Keep"));
    }
    #[test]
    fn ineligible_locations_never_produce_write_plans() {
        for location in [
            Location::Sent,
            Location::Drafts,
            Location::Outbox,
            Location::Spam,
            Location::Trash,
            Location::Chat,
        ] {
            let mut f = facts();
            f.location = location;
            assert_eq!(
                policy().plan(&f, &proposal(), now()).unwrap(),
                Outcome::Excluded
            );
        }
    }
    #[test]
    fn wrong_account_version_message_or_revision_is_rejected() {
        let mut f = facts();
        f.account = "Unapproved".into();
        assert_eq!(
            policy().plan(&f, &proposal(), now()),
            Err(PolicyError::UnapprovedAccount)
        );
        let mut p = proposal();
        p.policy_version = "new-unapproved".into();
        assert_eq!(
            policy().plan(&facts(), &p, now()),
            Err(PolicyError::ProposalMismatch)
        );
        p = proposal();
        p.message_id = "different-message".into();
        assert_eq!(
            policy().plan(&facts(), &p, now()),
            Err(PolicyError::ProposalMismatch)
        );
        f = facts();
        f.revision.clear();
        assert_eq!(
            policy().plan(&f, &proposal(), now()),
            Err(PolicyError::MissingIdentity)
        );
    }
    #[test]
    fn invented_or_duplicate_topics_cannot_expand_the_taxonomy() {
        for topics in [
            vec![],
            vec!["Unapproved".into()],
            vec!["Topic1".into(), "Topic1".into()],
        ] {
            let mut p = proposal();
            p.topics = topics;
            assert_eq!(
                policy().plan(&facts(), &p, now()),
                Err(PolicyError::InvalidTopics)
            );
        }
    }
    #[test]
    fn unread_retention_is_strictly_older_than_90_days() {
        let mut f = facts();
        f.received_at = now() - Duration::days(90);
        assert_eq!(file(&f, &proposal()).attention, Attention::ReadLater);
        f.received_at -= Duration::seconds(1);
        assert_eq!(file(&f, &proposal()).attention, Attention::Done);
        f.unread = false;
        assert_eq!(file(&f, &proposal()).attention, Attention::ReadLater);
    }
    #[test]
    fn unresolved_obligations_cannot_be_completed_by_a_model_or_retention() {
        for state in [
            Attention::NeedsReply,
            Attention::NeedsAction,
            Attention::Waiting,
        ] {
            let mut f = facts();
            f.received_at = now() - Duration::days(400);
            f.unresolved_attention = Some(state);
            let mut p = proposal();
            p.attention = Attention::Done;
            assert_eq!(file(&f, &p).attention, state);
        }
    }
    #[test]
    fn unpaid_bill_stays_actionable_and_urgent_stays_visible() {
        let mut f = facts();
        f.received_at = now() - Duration::days(400);
        f.unpaid_bill = true;
        assert_eq!(file(&f, &proposal()).attention, Attention::NeedsAction);
        f.unpaid_bill = false;
        f.already_urgent = true;
        let p = file(&f, &proposal());
        assert!(p.add_labels.contains("Priority/Urgent"));
        assert_ne!(p.attention, Attention::Done);
        assert!(p.remove_from_inbox);
    }
    #[test]
    fn ambiguous_identity_stays_review_without_unapproved_org_label() {
        let mut f = facts();
        f.received_at = now() - Duration::days(400);
        f.organization = Organization::Held;
        let p = file(&f, &proposal());
        assert_eq!(p.attention, Attention::Review);
        assert!(p.add_labels.iter().all(|l| !l.starts_with("Org/")));
        assert!(p.remove_from_inbox);
    }
    #[test]
    fn filed_mail_is_classified_without_requesting_an_inbox_move() {
        let mut f = facts();
        f.location = Location::Filed;
        assert!(!file(&f, &proposal()).remove_from_inbox);
    }
    #[test]
    fn unsafe_label_components_and_invalid_obligation_states_fail_closed() {
        let mut f = facts();
        f.organization = Organization::Approved("Example/Injected".into());
        assert_eq!(
            policy().plan(&f, &proposal(), now()),
            Err(PolicyError::InvalidOrganization)
        );
        f = facts();
        f.unresolved_attention = Some(Attention::Done);
        assert_eq!(
            policy().plan(&f, &proposal(), now()),
            Err(PolicyError::InvalidObligation)
        );
    }
    #[test]
    fn mapping_versions_are_checked_per_account() {
        let p = Policy::new(
            "approved-v1".into(),
            topics(),
            BTreeMap::from([("Business".into(), "stale-v0".into())]),
        )
        .unwrap();
        assert_eq!(
            p.plan(&facts(), &proposal(), now()),
            Err(PolicyError::UnapprovedAccount)
        );
    }
    #[test]
    fn review_identity_does_not_erase_an_existing_reply_obligation() {
        let mut f = facts();
        f.organization = Organization::Held;
        f.unresolved_attention = Some(Attention::NeedsReply);
        let p = file(&f, &proposal());
        assert_eq!(p.attention, Attention::NeedsReply);
        assert!(p.reasons.contains(&"organization_review"));
        assert!(p.add_labels.iter().all(|x| !x.starts_with("Org/")));
    }
    #[test]
    fn repeating_a_verified_plan_has_no_additive_label_work() {
        let mut f = facts();
        let first = file(&f, &proposal());
        f.current_labels.extend(first.add_labels);
        f.current_labels
            .retain(|x| !first.remove_labels.contains(x));
        f.location = Location::Filed;
        let second = file(&f, &proposal());
        assert!(second.add_labels.is_empty());
        assert!(second.remove_labels.is_empty());
        assert!(!second.remove_from_inbox);
    }

    #[test]
    fn reserved_looking_labels_are_not_removed_without_recorded_ownership() {
        let mut f = facts();
        f.owned_attention_labels.clear();
        assert!(file(&f, &proposal()).remove_labels.is_empty());
    }
}
