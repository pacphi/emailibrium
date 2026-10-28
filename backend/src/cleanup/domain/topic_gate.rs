//! Topic gates (docs/plan/mail-agent-2026-10.md section 7): a `Topic/` label
//! appears in an account's sidebar only if it earns its place by volume.
//! Pure logic — the monthly promote/demote pass (milestone 10's cadence
//! scheduling) calls this; this module has no I/O.

use std::collections::HashSet;

/// Measured facts about one candidate topic, over the account's mail.
#[derive(Debug, Clone)]
pub struct TopicStats {
    pub topic: String,
    pub message_count: u64,
    pub distinct_senders: HashSet<String>,
}

/// Minimum messages OR minimum distinct senders to earn a place, per
/// section 7 ("≥ 200 messages or ≥ 3 distinct senders").
pub const MIN_MESSAGES: u64 = 200;
pub const MIN_DISTINCT_SENDERS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Earns its own `Topic/<name>` label.
    Promoted,
    /// Collapses into its parent (or `Other` if it has none).
    Demoted,
}

/// Whether one candidate topic clears the gate.
///
/// A topic that already has a distinct disposition or retention policy
/// from its neighbors is exempt from the volume gate even below threshold
/// (section 7: "...and a distinct disposition or retention from its
/// neighbours"); `has_distinct_disposition` carries that exemption in from
/// the caller, which knows the account's disposition/retention config —
/// this module only knows volume.
pub fn gate(stats: &TopicStats, has_distinct_disposition: bool) -> GateDecision {
    if has_distinct_disposition {
        return GateDecision::Promoted;
    }
    if stats.message_count >= MIN_MESSAGES || stats.distinct_senders.len() >= MIN_DISTINCT_SENDERS {
        GateDecision::Promoted
    } else {
        GateDecision::Demoted
    }
}

/// One promote/demote pass over every candidate topic for an account.
pub fn run_gate_pass(
    candidates: &[TopicStats],
    has_distinct_disposition: impl Fn(&str) -> bool,
) -> Vec<(&str, GateDecision)> {
    candidates
        .iter()
        .map(|s| {
            (
                s.topic.as_str(),
                gate(s, has_distinct_disposition(&s.topic)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn senders(n: usize) -> HashSet<String> {
        (0..n).map(|i| format!("sender{i}@example.com")).collect()
    }

    #[test]
    fn a_topic_at_or_above_200_messages_is_promoted_even_with_one_sender() {
        let s = TopicStats {
            topic: "OpenSource".into(),
            message_count: 200,
            distinct_senders: senders(1),
        };
        assert_eq!(gate(&s, false), GateDecision::Promoted);
    }

    #[test]
    fn a_topic_below_200_messages_but_at_3_distinct_senders_is_promoted() {
        let s = TopicStats {
            topic: "Household".into(),
            message_count: 50,
            distinct_senders: senders(3),
        };
        assert_eq!(gate(&s, false), GateDecision::Promoted);
    }

    #[test]
    fn a_topic_below_both_thresholds_is_demoted() {
        let s = TopicStats {
            topic: "Obscure".into(),
            message_count: 50,
            distinct_senders: senders(2),
        };
        assert_eq!(gate(&s, false), GateDecision::Demoted);
    }

    #[test]
    fn a_distinct_disposition_exempts_a_topic_from_the_volume_gate() {
        let s = TopicStats {
            topic: "Finance".into(),
            message_count: 1,
            distinct_senders: senders(1),
        };
        assert_eq!(gate(&s, true), GateDecision::Promoted);
    }

    #[test]
    fn exactly_at_the_199_message_2_sender_boundary_is_demoted() {
        let s = TopicStats {
            topic: "Borderline".into(),
            message_count: 199,
            distinct_senders: senders(2),
        };
        assert_eq!(gate(&s, false), GateDecision::Demoted);
    }
}
