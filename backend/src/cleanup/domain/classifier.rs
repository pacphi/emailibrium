//! `RiskClassifier` — pure function `(PlannedOperation, AccountContext) -> RiskLevel`.
//!
//! Centralises ADR-030 §6 rules. Single source of truth for risk; used at
//! build time (assigns to operation) and at UI render time (no recomputation).

use super::operation::{PlanAction, Provider, RiskLevel, UnsubscribeMethodKind};

/// Per-account context used by the classifier to escalate to High when bulk
/// thresholds trip. Phase A only consumes a few fields; Phase D will likely
/// extend with engagement-rate from Insights.
#[derive(Debug, Clone)]
pub struct AccountContext {
    pub provider: Provider,
    pub group_size: u64,
    pub senders_in_group: u64,
    pub engagement_rate: f32,
}

impl AccountContext {
    pub fn for_provider(provider: Provider) -> Self {
        Self {
            provider,
            group_size: 0,
            senders_in_group: 0,
            engagement_rate: 0.0,
        }
    }
}

#[derive(Debug, Default)]
pub struct RiskClassifier;

impl RiskClassifier {
    pub fn new() -> Self {
        Self
    }

    /// Classify a single action for an account context.
    pub fn classify(&self, action: &PlanAction, ctx: &AccountContext) -> RiskLevel {
        let base = base_risk(action, ctx.provider);
        // Bulk-threshold escalation: any single source group > 1000 ops, or
        // > 5 senders touched in one source, or engagement_rate > 0.10 → High.
        if ctx.group_size > 1000 || ctx.senders_in_group > 5 || ctx.engagement_rate > 0.10 {
            return RiskLevel::High;
        }
        base
    }
}

fn base_risk(action: &PlanAction, provider: Provider) -> RiskLevel {
    match action {
        // Permanent delete is always High.
        PlanAction::Delete { permanent: true } => RiskLevel::High,
        // POP3 has no Trash → soft delete is also High.
        PlanAction::Delete { .. } if provider == Provider::Pop3 => RiskLevel::High,
        // Unsubscribe risk is per-method.
        PlanAction::Unsubscribe { method } => match method {
            UnsubscribeMethodKind::WebLink => RiskLevel::High,
            UnsubscribeMethodKind::Mailto => RiskLevel::Medium,
            UnsubscribeMethodKind::ListUnsubscribePost => RiskLevel::Low,
            UnsubscribeMethodKind::None => RiskLevel::Low,
        },
        // POP3 ops in general are best-effort → High.
        _ if provider == Provider::Pop3 => RiskLevel::High,
        // Move on Outlook/IMAP is Medium (cross-folder, reversible).
        PlanAction::Move { .. } if matches!(provider, Provider::Outlook | Provider::Imap) => {
            RiskLevel::Medium
        }
        // Archive on IMAP depends on server's Archive support.
        PlanAction::Archive if provider == Provider::Imap => RiskLevel::Medium,
        // Soft delete on Gmail/Outlook/IMAP — Low (Trash retention 14-30d).
        PlanAction::Delete { .. } => RiskLevel::Low,
        PlanAction::Archive => RiskLevel::Low,
        PlanAction::AddLabel { .. } => RiskLevel::Low,
        PlanAction::Move { .. } => RiskLevel::Low, // Gmail "move" = label transition
        PlanAction::MarkRead => RiskLevel::Low,
        PlanAction::Star { .. } => RiskLevel::Low,
    }
}

// ---------------------------------------------------------------------------
// Tests — table-driven per ADR-030 §6 / DDD-008 addendum
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::operation::*;
    use super::*;

    fn ctx(p: Provider) -> AccountContext {
        AccountContext::for_provider(p)
    }

    #[test]
    fn permanent_delete_is_high_on_any_provider() {
        let c = RiskClassifier::new();
        for p in [
            Provider::Gmail,
            Provider::Outlook,
            Provider::Imap,
            Provider::Pop3,
        ] {
            let r = c.classify(&PlanAction::Delete { permanent: true }, &ctx(p));
            assert_eq!(r, RiskLevel::High, "provider={p:?}");
        }
    }

    #[test]
    fn pop3_delete_is_high_even_when_soft() {
        let r = RiskClassifier::new().classify(
            &PlanAction::Delete { permanent: false },
            &ctx(Provider::Pop3),
        );
        assert_eq!(r, RiskLevel::High);
    }

    #[test]
    fn pop3_archive_is_high() {
        let r = RiskClassifier::new().classify(&PlanAction::Archive, &ctx(Provider::Pop3));
        assert_eq!(r, RiskLevel::High);
    }

    #[test]
    fn unsubscribe_risk_table() {
        let c = RiskClassifier::new();
        let p = ctx(Provider::Gmail);
        assert_eq!(
            c.classify(
                &PlanAction::Unsubscribe {
                    method: UnsubscribeMethodKind::WebLink
                },
                &p
            ),
            RiskLevel::High
        );
        assert_eq!(
            c.classify(
                &PlanAction::Unsubscribe {
                    method: UnsubscribeMethodKind::Mailto
                },
                &p
            ),
            RiskLevel::Medium
        );
        assert_eq!(
            c.classify(
                &PlanAction::Unsubscribe {
                    method: UnsubscribeMethodKind::ListUnsubscribePost
                },
                &p
            ),
            RiskLevel::Low
        );
    }

    #[test]
    fn move_on_outlook_imap_is_medium() {
        let c = RiskClassifier::new();
        for p in [Provider::Outlook, Provider::Imap] {
            assert_eq!(
                c.classify(
                    &PlanAction::Move {
                        kind: MoveKind::Folder
                    },
                    &ctx(p)
                ),
                RiskLevel::Medium
            );
        }
    }

    #[test]
    fn move_on_gmail_is_low_label_semantics() {
        let r = RiskClassifier::new().classify(
            &PlanAction::Move {
                kind: MoveKind::Label,
            },
            &ctx(Provider::Gmail),
        );
        assert_eq!(r, RiskLevel::Low);
    }

    #[test]
    fn imap_archive_is_medium() {
        let r = RiskClassifier::new().classify(&PlanAction::Archive, &ctx(Provider::Imap));
        assert_eq!(r, RiskLevel::Medium);
    }

    #[test]
    fn gmail_archive_label_markread_star_low() {
        let c = RiskClassifier::new();
        let p = ctx(Provider::Gmail);
        assert_eq!(c.classify(&PlanAction::Archive, &p), RiskLevel::Low);
        assert_eq!(
            c.classify(
                &PlanAction::AddLabel {
                    kind: MoveKind::Label
                },
                &p
            ),
            RiskLevel::Low
        );
        assert_eq!(c.classify(&PlanAction::MarkRead, &p), RiskLevel::Low);
        assert_eq!(
            c.classify(&PlanAction::Star { on: true }, &p),
            RiskLevel::Low
        );
    }

    #[test]
    fn bulk_threshold_escalates_to_high() {
        let c = RiskClassifier::new();
        let mut p = ctx(Provider::Gmail);
        p.group_size = 1500; // > 1000 → High
        assert_eq!(c.classify(&PlanAction::Archive, &p), RiskLevel::High);
    }

    #[test]
    fn senders_threshold_escalates_to_high() {
        let c = RiskClassifier::new();
        let mut p = ctx(Provider::Gmail);
        p.senders_in_group = 6; // > 5 → High
        assert_eq!(c.classify(&PlanAction::Archive, &p), RiskLevel::High);
    }

    #[test]
    fn engagement_threshold_escalates_to_high() {
        let c = RiskClassifier::new();
        let mut p = ctx(Provider::Gmail);
        p.engagement_rate = 0.20; // > 0.10 → High
        assert_eq!(
            c.classify(
                &PlanAction::Unsubscribe {
                    method: UnsubscribeMethodKind::ListUnsubscribePost
                },
                &p
            ),
            RiskLevel::High
        );
    }
}
