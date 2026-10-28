//! Header-rule `Kind` classifier (docs/plan/mail-agent-2026-10.md §7).
//!
//! Headers decide `Kind` deterministically for most mail; this runs *ahead
//! of* the semantic/vector classifier (`vectors::categorizer`), which keeps
//! classifying `Topic` and is unaffected by this module. Pure function, no
//! I/O, no provider calls — matching the rest of `cleanup::domain`.
//!
//! First match wins, in the order the plan specifies. Two simplifications
//! from the full spec, both because the data they'd need isn't carried on
//! `EmailMessage` yet, and both documented at the rule they affect rather
//! than silently narrowed:
//! - Rule 3 (Subscriptions) uses `List-Unsubscribe` presence alone, not the
//!   full `List-Unsubscribe` + (`List-Id` | known ESP Return-Path |
//!   `Precedence: bulk/list`) combination — those three signals aren't
//!   extracted from the wire today.
//! - Rule 4 (Receipts)'s "retailer/payment domains with no List-Unsubscribe"
//!   branch isn't implemented — there's no agreed domain list to match
//!   against yet. Only the local-part signatures are implemented.
//! - Rule 1 (protected/replied-to) and rule 2 (own addresses) take their
//!   answer as an input rather than deriving it, because the sender ledger
//!   (first/last seen, replied-to) and the account's own-address list are
//!   both later milestones' work (section 8's ledger; account settings).

use super::sender_key::SenderKey;
use crate::email::types::EmailMessage;

/// The `Kind/` namespace (docs/plan/mail-agent-2026-10.md §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Never filed. Includes the `People/Broadcast` variant for >10
    /// recipients and the `State/New` proposal for first-contact humans,
    /// both carried on [`Classification`] rather than as separate `Kind`
    /// values, since they're refinements of People, not different Kinds.
    People,
    Subscriptions,
    Receipts,
    Notifications,
    /// `Other/Agent` — mail from one of the account's own addresses.
    OtherAgent,
    /// Unresolved by any rule; queued for judgment (batching that judgment
    /// is milestone 4's `Held`-sender work, out of scope here).
    Other,
}

impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Kind::People => "Kind/People",
            Kind::Subscriptions => "Kind/Subscriptions",
            Kind::Receipts => "Kind/Receipts",
            Kind::Notifications => "Kind/Notifications",
            Kind::OtherAgent => "Kind/Other/Agent",
            Kind::Other => "Kind/Other",
        }
    }
}

/// Why a `Notifications` message routed the way it did, for GitHub mail
/// specifically — the plan's reason-routing-by-cc-address rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GithubReason {
    /// `mention@`, `review_requested@`, `assign@` — needs you.
    NeedsYou,
    /// `comment@`, `author@` — filed.
    Filed,
    /// `ci_activity@`, `push@`, `state_change@` — a source-settings
    /// candidate (the thing a notification-settings change should silence
    /// at the source, per §8's checklist).
    SourceSettingsCandidate,
}

/// Everything the classifier needs that isn't on `EmailMessage` itself,
/// because it comes from state this milestone doesn't build (the sender
/// ledger, the account's own-address list).
#[derive(Debug, Clone, Default)]
pub struct ClassificationContext {
    /// Rule 1: a protected sender, or a human the account has replied to.
    /// Defaults to `false` until the sender ledger (section 8) exists.
    pub is_protected_or_replied_human: bool,
    /// Rule 2: the account's own addresses (and declared agent addresses).
    pub own_addresses: Vec<String>,
}

/// The classifier's output: a `Kind`, plus the two refinements the plan
/// calls for that aren't separate Kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: Kind,
    /// Set only when `kind == Notifications` and the message matched the
    /// GitHub pattern with a recognized reason in a cc address.
    pub github_reason: Option<GithubReason>,
    /// Set only when `kind == People`: >10 recipients -> `People/Broadcast`
    /// instead of plain `People`.
    pub is_broadcast: bool,
    /// Set only when `kind == People` and not broadcast: the plan allows
    /// the classifier to *propose* `State/New` for a first-contact human
    /// (not apply it — `State/` stays user-set; this is a suggested
    /// default pending the ledger).
    pub propose_state_new: bool,
}

impl Classification {
    fn people(is_broadcast: bool) -> Self {
        Self {
            kind: Kind::People,
            github_reason: None,
            is_broadcast,
            propose_state_new: !is_broadcast,
        }
    }

    fn simple(kind: Kind) -> Self {
        Self {
            kind,
            github_reason: None,
            is_broadcast: false,
            propose_state_new: false,
        }
    }
}

const RECEIPT_LOCAL_PARTS: &[&str] = &[
    "order-update",
    "shipment-tracking",
    "return",
    "auto-confirm",
    "invoice",
    "receipt",
    "statement",
    "payments-messages",
];

const BULK_LOCAL_PART_MARKERS: &[&str] = &["noreply", "no-reply", "notification", "alert"];

const NOTIFICATION_PLATFORM_DOMAINS: &[&str] = &["github.com", "slack.com", "circle.so"];

/// Classify one message's `Kind`, first-rule-to-match wins.
pub fn classify(msg: &EmailMessage, ctx: &ClassificationContext) -> Classification {
    // Rule 1: protected or replied-to human -> People, never filed.
    if ctx.is_protected_or_replied_human {
        return Classification::people(false);
    }

    // Rule 2: from one of the account's own addresses -> Other/Agent.
    let from_lower = msg.from.to_ascii_lowercase();
    if ctx
        .own_addresses
        .iter()
        .any(|a| a.to_ascii_lowercase() == from_lower)
    {
        return Classification::simple(Kind::OtherAgent);
    }

    let Some(sender_key) = SenderKey::from_address(&msg.from) else {
        // No parseable address at all; nothing left to key rules off of.
        return Classification::simple(Kind::Other);
    };
    let local_part = sender_key
        .as_str()
        .split('@')
        .next()
        .unwrap_or_default()
        .to_string();
    let domain = sender_key
        .as_str()
        .rsplit_once('@')
        .map(|(_, d)| d)
        .unwrap_or_default();

    // Rule 3: List-Unsubscribe present -> Subscriptions. (Simplified: see
    // module docs — the full List-Id/ESP/Precedence combination isn't
    // checked.)
    if msg.list_unsubscribe.is_some() {
        return Classification::simple(Kind::Subscriptions);
    }

    // Rule 4: receipt-signature local-parts -> Receipts.
    if RECEIPT_LOCAL_PARTS
        .iter()
        .any(|sig| local_part.contains(sig))
    {
        return Classification::simple(Kind::Receipts);
    }

    // Rule 5: bulk/notification markers, or a known platform domain ->
    // Notifications, with GitHub reason-routing by cc address.
    let is_platform = NOTIFICATION_PLATFORM_DOMAINS
        .iter()
        .any(|d| domain == *d || domain.ends_with(&format!(".{d}")));
    let has_bulk_marker = BULK_LOCAL_PART_MARKERS
        .iter()
        .any(|m| local_part.contains(m));
    if is_platform || has_bulk_marker {
        let github_reason = if domain == "github.com" || domain.ends_with(".github.com") {
            github_reason_from_cc(&msg.cc)
        } else {
            None
        };
        return Classification {
            kind: Kind::Notifications,
            github_reason,
            is_broadcast: false,
            propose_state_new: false,
        };
    }

    // Rule 6: human-looking sender (reached only by elimination of every
    // bulk marker above) -> People, or People/Broadcast past 10 recipients.
    let recipient_count = msg.to.len() + msg.cc.len();
    if recipient_count <= 10 {
        return Classification::people(false);
    }
    Classification::people(true)
}

/// GitHub's reason-routing: the reason is encoded in the *cc* address
/// (`mention@`, `review_requested@`, `ci_activity@`, ...), not the From
/// address, which is always a `notifications@` variant.
fn github_reason_from_cc(cc: &[String]) -> Option<GithubReason> {
    for addr in cc {
        let Some(key) = SenderKey::from_address(addr) else {
            continue;
        };
        let local = key.as_str().split('@').next().unwrap_or_default();
        if matches!(local, "mention" | "review_requested" | "assign") {
            return Some(GithubReason::NeedsYou);
        }
        if matches!(local, "comment" | "author") {
            return Some(GithubReason::Filed);
        }
        if matches!(local, "ci_activity" | "push" | "state_change") {
            return Some(GithubReason::SourceSettingsCandidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn msg(from: &str) -> EmailMessage {
        EmailMessage {
            id: "1".into(),
            thread_id: None,
            from: from.into(),
            to: vec!["me@example.com".into()],
            cc: vec![],
            subject: "Subject".into(),
            snippet: String::new(),
            body: None,
            body_html: None,
            labels: vec![],
            date: Utc::now(),
            is_read: false,
            list_unsubscribe: None,
            list_unsubscribe_post: None,
        }
    }

    fn ctx() -> ClassificationContext {
        ClassificationContext::default()
    }

    #[test]
    fn protected_or_replied_human_is_always_people() {
        let mut m = msg("noreply@spammy-platform.example");
        m.list_unsubscribe = Some("<https://example.com/unsub>".into());
        let c = ClassificationContext {
            is_protected_or_replied_human: true,
            own_addresses: vec![],
        };
        assert_eq!(classify(&m, &c).kind, Kind::People);
    }

    #[test]
    fn own_address_is_other_agent() {
        let m = msg("me-automation@example.com");
        let c = ClassificationContext {
            is_protected_or_replied_human: false,
            own_addresses: vec!["me-automation@example.com".into()],
        };
        assert_eq!(classify(&m, &c).kind, Kind::OtherAgent);
    }

    #[test]
    fn list_unsubscribe_is_subscriptions() {
        let mut m = msg("newsletter@creator-college.example");
        m.list_unsubscribe = Some("<mailto:unsub@creator-college.example>".into());
        assert_eq!(classify(&m, &ctx()).kind, Kind::Subscriptions);
    }

    #[test]
    fn receipt_local_parts_are_receipts() {
        for local in [
            "order-update",
            "shipment-tracking",
            "return",
            "auto-confirm",
        ] {
            let m = msg(&format!("{local}@amazon.example"));
            assert_eq!(
                classify(&m, &ctx()).kind,
                Kind::Receipts,
                "local-part={local}"
            );
        }
    }

    #[test]
    fn noreply_marker_is_notifications() {
        let m = msg("noreply@some-saas.example");
        assert_eq!(classify(&m, &ctx()).kind, Kind::Notifications);
    }

    #[test]
    fn github_mention_needs_you() {
        let mut m = msg("notifications@github.com");
        m.cc = vec!["mention@notifications.github.com".into()];
        let r = classify(&m, &ctx());
        assert_eq!(r.kind, Kind::Notifications);
        assert_eq!(r.github_reason, Some(GithubReason::NeedsYou));
    }

    #[test]
    fn github_ci_activity_is_source_settings_candidate() {
        let mut m = msg("notifications@github.com");
        m.cc = vec!["ci_activity@notifications.github.com".into()];
        let r = classify(&m, &ctx());
        assert_eq!(r.github_reason, Some(GithubReason::SourceSettingsCandidate));
    }

    #[test]
    fn github_comment_is_filed() {
        let mut m = msg("notifications@github.com");
        m.cc = vec!["comment@notifications.github.com".into()];
        let r = classify(&m, &ctx());
        assert_eq!(r.github_reason, Some(GithubReason::Filed));
    }

    #[test]
    fn human_sender_is_people_with_proposed_new_state() {
        let m = msg("jane.doe@friendmail.example");
        let r = classify(&m, &ctx());
        assert_eq!(r.kind, Kind::People);
        assert!(!r.is_broadcast);
        assert!(r.propose_state_new);
    }

    #[test]
    fn more_than_ten_recipients_is_broadcast() {
        let mut m = msg("jane.doe@friendmail.example");
        m.to = (0..11).map(|i| format!("person{i}@example.com")).collect();
        let r = classify(&m, &ctx());
        assert_eq!(r.kind, Kind::People);
        assert!(r.is_broadcast);
        assert!(!r.propose_state_new);
    }

    #[test]
    fn unparseable_from_falls_to_other() {
        let m = msg("not an address");
        assert_eq!(classify(&m, &ctx()).kind, Kind::Other);
    }

    #[test]
    fn rule_order_subscriptions_beats_receipt_looking_local_part() {
        // A message whose local-part looks like a receipt signature but
        // also carries List-Unsubscribe must resolve to Subscriptions
        // (rule 3), since rule 3 is checked first.
        let mut m = msg("receipt@creator-college.example");
        m.list_unsubscribe = Some("<mailto:unsub@creator-college.example>".into());
        assert_eq!(classify(&m, &ctx()).kind, Kind::Subscriptions);
    }
}
