//! Rule authority levels and graduation (docs/plan/mail-agent-2026-10.md
//! section 3/10/11). Extends the existing rules engine (ADR-014, migration
//! 012/032) rather than a parallel model.
//!
//! Only levels 1 (Suggest) and 2 (Trusted rule) are states a rule occupies.
//! Level 0 (Observe) is "no rule exists yet for this sender" — not
//! represented here. Level 3 (Act on your behalf) is explicitly "not
//! planned" per the plan and has no representation at all.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityLevel {
    Suggest,
    Trusted,
}

impl AuthorityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::Trusted => "trusted",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "trusted" => Self::Trusted,
            // Unrecognized/missing values fail closed to the lower-trust
            // state rather than silently granting Trusted.
            _ => Self::Suggest,
        }
    }
}

/// How the user responded to one suggestion from this rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// "Apply once", with no correction to what was applied.
    ApplyOnceClean,
    /// "Always" — graduates immediately regardless of streak.
    Always,
    /// The user corrected what the rule proposed. Resets the clean streak;
    /// does not demote an already-Trusted rule (the plan specifies
    /// graduation criteria, not automatic demotion).
    Correction,
}

/// One rule's authority state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleAuthority {
    pub level: AuthorityLevel,
    pub clean_streak: u32,
}

impl RuleAuthority {
    pub fn new(level: AuthorityLevel, clean_streak: u32) -> Self {
        Self {
            level,
            clean_streak,
        }
    }

    pub fn default_for_new_rule() -> Self {
        Self {
            level: AuthorityLevel::Suggest,
            clean_streak: 0,
        }
    }
}

/// Consecutive clean Apply-once approvals needed to graduate without an
/// explicit "Always" (section 11 decisions log: "three consecutive clean
/// Apply-once Mondays graduate automatically").
pub const CLEAN_STREAK_TO_GRADUATE: u32 = 3;

/// What recording one approval event changed, for the caller to persist and
/// log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraduationOutcome {
    pub new_state: RuleAuthority,
    /// `Some(event)` only on the call that actually crosses into Trusted —
    /// this is what gets written to `rule_graduation_log`, not every call.
    pub graduation_event: Option<&'static str>,
}

/// Apply one approval event to a rule's current authority state.
///
/// Pure function: no I/O, no clock — the caller persists `new_state` and,
/// when `graduation_event` is `Some`, appends a `rule_graduation_log` row
/// with its own timestamp.
pub fn record_approval(current: RuleAuthority, approval: Approval) -> GraduationOutcome {
    match (current.level, approval) {
        // Already trusted: further approvals/corrections don't change the
        // level (no demotion modeled) and don't need a growing streak.
        (AuthorityLevel::Trusted, _) => GraduationOutcome {
            new_state: current,
            graduation_event: None,
        },

        (AuthorityLevel::Suggest, Approval::Always) => GraduationOutcome {
            new_state: RuleAuthority::new(AuthorityLevel::Trusted, 0),
            graduation_event: Some("always_approved"),
        },

        (AuthorityLevel::Suggest, Approval::Correction) => GraduationOutcome {
            new_state: RuleAuthority::new(AuthorityLevel::Suggest, 0),
            graduation_event: None,
        },

        (AuthorityLevel::Suggest, Approval::ApplyOnceClean) => {
            let streak = current.clean_streak + 1;
            if streak >= CLEAN_STREAK_TO_GRADUATE {
                GraduationOutcome {
                    new_state: RuleAuthority::new(AuthorityLevel::Trusted, 0),
                    graduation_event: Some("three_clean_mondays"),
                }
            } else {
                GraduationOutcome {
                    new_state: RuleAuthority::new(AuthorityLevel::Suggest, streak),
                    graduation_event: None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_rule_starts_at_suggest_with_no_streak() {
        let r = RuleAuthority::default_for_new_rule();
        assert_eq!(r.level, AuthorityLevel::Suggest);
        assert_eq!(r.clean_streak, 0);
    }

    #[test]
    fn always_graduates_immediately_regardless_of_streak() {
        let r = RuleAuthority::new(AuthorityLevel::Suggest, 0);
        let out = record_approval(r, Approval::Always);
        assert_eq!(out.new_state.level, AuthorityLevel::Trusted);
        assert_eq!(out.graduation_event, Some("always_approved"));
    }

    #[test]
    fn three_consecutive_clean_approvals_graduate_automatically() {
        let mut r = RuleAuthority::default_for_new_rule();
        for i in 1..=2 {
            let out = record_approval(r, Approval::ApplyOnceClean);
            assert_eq!(out.new_state.level, AuthorityLevel::Suggest, "streak {i}");
            assert_eq!(out.new_state.clean_streak, i);
            assert_eq!(out.graduation_event, None);
            r = out.new_state;
        }
        let third = record_approval(r, Approval::ApplyOnceClean);
        assert_eq!(third.new_state.level, AuthorityLevel::Trusted);
        assert_eq!(third.graduation_event, Some("three_clean_mondays"));
    }

    #[test]
    fn a_correction_resets_the_streak_without_graduating() {
        let r = RuleAuthority::new(AuthorityLevel::Suggest, 2);
        let out = record_approval(r, Approval::Correction);
        assert_eq!(out.new_state.level, AuthorityLevel::Suggest);
        assert_eq!(out.new_state.clean_streak, 0);
        assert_eq!(out.graduation_event, None);
    }

    #[test]
    fn a_correction_after_two_clean_approvals_means_a_third_clean_one_does_not_graduate() {
        let r = RuleAuthority::new(AuthorityLevel::Suggest, 2);
        let after_correction = record_approval(r, Approval::Correction).new_state;
        let out = record_approval(after_correction, Approval::ApplyOnceClean);
        assert_eq!(
            out.new_state.level,
            AuthorityLevel::Suggest,
            "streak reset, this is only 1"
        );
        assert_eq!(out.new_state.clean_streak, 1);
    }

    #[test]
    fn an_already_trusted_rule_does_not_change_level_on_further_events() {
        let r = RuleAuthority::new(AuthorityLevel::Trusted, 0);
        assert_eq!(
            record_approval(r, Approval::Correction).new_state.level,
            AuthorityLevel::Trusted
        );
        assert_eq!(
            record_approval(r, Approval::ApplyOnceClean).new_state.level,
            AuthorityLevel::Trusted
        );
    }

    #[test]
    fn parse_fails_closed_to_suggest_on_unknown_values() {
        assert_eq!(AuthorityLevel::parse("trusted"), AuthorityLevel::Trusted);
        assert_eq!(AuthorityLevel::parse("suggest"), AuthorityLevel::Suggest);
        assert_eq!(AuthorityLevel::parse("garbage"), AuthorityLevel::Suggest);
        assert_eq!(AuthorityLevel::parse(""), AuthorityLevel::Suggest);
    }
}
