//! Retention proposals (docs/plan/mail-agent-2026-10.md section 8, monthly
//! `/mail health`: "retention proposals only on trusted Notifications
//! rules with ephemeral senders (trash after 365 d, 30-day undo), never on
//! the personal account; Receipts, People and security never"). Pure, no
//! I/O — `staff::health` derives [`RuleFact`] and an account role string
//! from the real `rules` and `connected_accounts` tables and calls
//! [`propose_retention`] per (rule, account) pair it's evaluating; see that
//! module's doc for exactly how each field gets computed from real data.
//!
//! "Ephemeral sender" borrows the plan's only defined engagement threshold
//! (section 8's subscriptions table: "Never opened in 90 days and >= 10
//! messages") rather than inventing a new one — `staff::health` documents
//! precisely how `is_ephemeral_sender` is computed.

use crate::cleanup::domain::kind::Kind;
use crate::rules::authority::AuthorityLevel;

/// Trash after this many days (docs/plan/mail-agent-2026-10.md section 8).
pub const TRASH_AFTER_DAYS: u32 = 365;
/// Undo window before the trash is permanent (same plan line).
pub const UNDO_WINDOW_DAYS: u32 = 30;

/// Everything [`propose_retention`] needs about one rule, scoped to one
/// account it might apply to.
#[derive(Debug, Clone)]
pub struct RuleFact {
    pub rule_id: String,
    pub authority: AuthorityLevel,
    /// `None` when the rule's actions name no recognized `Kind/` label —
    /// treated as ineligible, same as any non-Notifications Kind.
    pub kind: Option<Kind>,
    /// Always `false` from every real caller today — there is no
    /// protected/security-sender list yet (see
    /// `cleanup::domain::kind::ClassificationContext`'s own doc on
    /// `is_protected_or_replied_human`). Modeled and enforced here now, not
    /// bolted on once that list exists.
    pub is_security_flagged: bool,
    pub is_ephemeral_sender: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionProposal {
    pub rule_id: String,
    pub trash_after_days: u32,
    pub undo_window_days: u32,
}

/// Propose a 365-day trash / 30-day undo retention for one rule evaluated
/// against one account, or `None` if any exclusion applies.
///
/// `account_role` is that account's declared role (migration 030 /
/// `AccountRole::as_str()`), e.g. `Some("professional")`. `None`
/// (undeclared) fails closed exactly like `Some("personal")` — an
/// undeclared account might *be* the personal one, and this proposal must
/// never risk it.
pub fn propose_retention(fact: &RuleFact, account_role: Option<&str>) -> Option<RetentionProposal> {
    match account_role {
        Some(role) if role != "personal" => {}
        _ => return None,
    }
    if fact.authority != AuthorityLevel::Trusted {
        return None;
    }
    if fact.kind != Some(Kind::Notifications) {
        return None;
    }
    if fact.is_security_flagged {
        return None;
    }
    if !fact.is_ephemeral_sender {
        return None;
    }
    Some(RetentionProposal {
        rule_id: fact.rule_id.clone(),
        trash_after_days: TRASH_AFTER_DAYS,
        undo_window_days: UNDO_WINDOW_DAYS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_fact() -> RuleFact {
        RuleFact {
            rule_id: "r1".to_string(),
            authority: AuthorityLevel::Trusted,
            kind: Some(Kind::Notifications),
            is_security_flagged: false,
            is_ephemeral_sender: true,
        }
    }

    #[test]
    fn an_eligible_rule_on_a_non_personal_account_produces_the_365_30_proposal() {
        let out = propose_retention(&eligible_fact(), Some("professional"));
        assert_eq!(
            out,
            Some(RetentionProposal {
                rule_id: "r1".to_string(),
                trash_after_days: 365,
                undo_window_days: 30,
            })
        );
    }

    #[test]
    fn a_security_flagged_sender_is_never_proposed_even_when_everything_else_is_eligible() {
        let mut fact = eligible_fact();
        fact.is_security_flagged = true;
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn a_people_kind_rule_is_never_proposed() {
        let mut fact = eligible_fact();
        fact.kind = Some(Kind::People);
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn a_receipts_kind_rule_is_never_proposed() {
        let mut fact = eligible_fact();
        fact.kind = Some(Kind::Receipts);
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn a_subscriptions_kind_rule_is_never_proposed() {
        let mut fact = eligible_fact();
        fact.kind = Some(Kind::Subscriptions);
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn a_rule_with_no_recognized_kind_label_is_never_proposed() {
        let mut fact = eligible_fact();
        fact.kind = None;
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn the_personal_account_is_never_proposed_even_when_everything_else_is_eligible() {
        assert_eq!(propose_retention(&eligible_fact(), Some("personal")), None);
    }

    #[test]
    fn an_undeclared_account_role_fails_closed_like_the_personal_account() {
        assert_eq!(propose_retention(&eligible_fact(), None), None);
    }

    #[test]
    fn a_suggest_level_rule_is_not_yet_proposed() {
        let mut fact = eligible_fact();
        fact.authority = AuthorityLevel::Suggest;
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }

    #[test]
    fn a_non_ephemeral_sender_is_not_proposed() {
        let mut fact = eligible_fact();
        fact.is_ephemeral_sender = false;
        assert_eq!(propose_retention(&fact, Some("professional")), None);
    }
}
