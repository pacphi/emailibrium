//! Open-loop SLA clocks (docs/plan/mail-agent-2026-10.md section 8,
//! "Attention brief": "open loops past their topic SLA (Clients 2 days,
//! Household 3, Career 3, Community 7)"). Pure, no I/O.
//!
//! "Topic" here means the plan's own small taxonomy (Clients/Household/
//! Career/Community), not `emails.category` (the pre-existing generic
//! `EmailCategory` classifier: Work/Finance/Shopping/...) and not
//! `topic_clusters.name` (auto-generated from TF-IDF terms over email
//! subjects — see `vectors::clustering::generate_cluster_name` — so it
//! produces names like "Cluster 1", not "Clients"). Neither carries the
//! plan's taxonomy today, so every real caller in this milestone passes
//! `topic: None` and falls back to the Community/default SLA — see
//! `staff::brief`'s module doc for the honest state of this gap.

use chrono::{DateTime, Duration, Utc};

/// Fallback for any topic not in the plan's table, or when no topic is
/// known at all (today, always — see module doc). Community's own SLA (7
/// days) doubles as the fallback: it's already the plan's most lenient
/// topic, so defaulting to it never flags a loop sooner than the plan
/// would for a topic it actually named.
pub const DEFAULT_SLA_DAYS: i64 = 7;

/// Days a topic's open loops get before they boomerang back to the inbox.
pub fn sla_days(topic: Option<&str>) -> i64 {
    match topic.map(str::to_ascii_lowercase).as_deref() {
        Some("clients") => 2,
        Some("household") => 3,
        Some("career") => 3,
        Some("community") => 7,
        _ => DEFAULT_SLA_DAYS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStatus {
    WithinSla,
    /// Past its topic's SLA — boomerangs back into the inbox at the next
    /// morning brief (docs/plan/mail-agent-2026-10.md section 8).
    PastSla,
}

/// Whether an open loop clocked at `since` is still within its SLA as of
/// `now`. Strictly greater than `sla_days` to cross into `PastSla`: a loop
/// exactly at the boundary gets one more brief before it boomerangs.
pub fn loop_status(since: DateTime<Utc>, now: DateTime<Utc>, sla_days: i64) -> LoopStatus {
    if now.signed_duration_since(since) > Duration::days(sla_days) {
        LoopStatus::PastSla
    } else {
        LoopStatus::WithinSla
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_named_topic_gets_its_plan_specified_sla() {
        assert_eq!(sla_days(Some("Clients")), 2);
        assert_eq!(sla_days(Some("Household")), 3);
        assert_eq!(sla_days(Some("Career")), 3);
        assert_eq!(sla_days(Some("Community")), 7);
    }

    #[test]
    fn topic_matching_is_case_insensitive() {
        assert_eq!(sla_days(Some("CLIENTS")), 2);
        assert_eq!(sla_days(Some("hOuSeHold")), 3);
    }

    #[test]
    fn an_unrecognized_topic_falls_back_to_the_default() {
        assert_eq!(sla_days(Some("Obscure")), DEFAULT_SLA_DAYS);
    }

    #[test]
    fn no_topic_falls_back_to_the_default() {
        assert_eq!(sla_days(None), DEFAULT_SLA_DAYS);
        assert_eq!(
            DEFAULT_SLA_DAYS, 7,
            "default must equal Community's own SLA"
        );
    }

    #[test]
    fn exactly_at_the_sla_boundary_is_still_within_sla() {
        let since = Utc::now() - Duration::days(2);
        let now = since + Duration::days(2);
        assert_eq!(loop_status(since, now, 2), LoopStatus::WithinSla);
    }

    #[test]
    fn one_second_past_the_boundary_is_past_sla() {
        let since = Utc::now() - Duration::days(2) - Duration::seconds(1);
        let now = since + Duration::days(2) + Duration::seconds(1);
        assert_eq!(loop_status(since, now, 2), LoopStatus::PastSla);
    }

    #[test]
    fn well_within_the_window_is_within_sla() {
        let since = Utc::now();
        assert_eq!(loop_status(since, since, 7), LoopStatus::WithinSla);
    }
}
