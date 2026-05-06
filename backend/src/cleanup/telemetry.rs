//! Cleanup telemetry events (Phase D, ADR-030 §Security).
//!
//! Structured event surface for cleanup planning + apply. We do NOT add a
//! new analytics SDK; events are emitted via `tracing::info!` against the
//! `cleanup.telemetry` target. Downstream pipelines (the existing
//! log-scrub-aware tracing infrastructure) pick them up.
//!
//! ## Privacy contract — ADR-030 §Security
//!
//! "No plan content is logged; only counts."
//!
//! - Raw `user_id` MUST NOT appear in payloads. We carry an 8-byte
//!   truncated blake3 hex of the user id (`hash_user_id`) which is
//!   sufficient to correlate sessions but not to re-identify the user.
//!   ADR-017 GDPR alignment.
//! - `account_id` is similarly hashed where it appears.
//! - Counts, ids (plan_id, job_id) and durations are allowed; email ids,
//!   subject lines, sender addresses, rule bodies, and folder paths are
//!   NOT.
//!
//! The frontend submits `CleanupPlanReviewed` via
//! `POST /api/v1/cleanup/telemetry`; the other variants are server-side
//! only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cleanup::domain::operation::{JobState, RiskMax, SkipReason};
use crate::cleanup::domain::plan::{JobId, PlanId};

// ---------------------------------------------------------------------------
// Event enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum CleanupTelemetryEvent {
    CleanupPlanBuilt {
        plan_id: PlanId,
        user_id_hash: String,
        total_operations: u64,
        risk_low: u64,
        risk_medium: u64,
        risk_high: u64,
        accounts: u64,
        warnings_count: u64,
        build_duration_ms: u64,
    },
    CleanupPlanReviewed {
        plan_id: PlanId,
        user_id_hash: String,
        time_on_review_ms: u64,
        expanded_groups: u64,
        samples_viewed: u64,
    },
    CleanupApplyStarted {
        plan_id: PlanId,
        job_id: JobId,
        user_id_hash: String,
        risk_max: RiskMax,
        ack_high_count: u64,
        ack_medium_count: u64,
    },
    CleanupApplyFinished {
        plan_id: PlanId,
        job_id: JobId,
        user_id_hash: String,
        applied: u64,
        failed: u64,
        skipped: u64,
        skipped_by_reason: BTreeMap<SkipReason, u64>,
        duration_ms: u64,
        status: JobState,
    },
    CleanupPlanRefreshed {
        plan_id: PlanId,
        user_id_hash: String,
        account_id_hash: String,
        reason: String,
    },
}

impl CleanupTelemetryEvent {
    /// Human-readable event name for log targeting / metric binding.
    pub fn name(&self) -> &'static str {
        match self {
            Self::CleanupPlanBuilt { .. } => "cleanup_plan_built",
            Self::CleanupPlanReviewed { .. } => "cleanup_plan_reviewed",
            Self::CleanupApplyStarted { .. } => "cleanup_apply_started",
            Self::CleanupApplyFinished { .. } => "cleanup_apply_finished",
            Self::CleanupPlanRefreshed { .. } => "cleanup_plan_refreshed",
        }
    }
}

// ---------------------------------------------------------------------------
// Emitter
// ---------------------------------------------------------------------------

/// Thin emitter wrapping `tracing` so callers don't have to know about the
/// event surface. Sized so an `Arc<TelemetryEmitter>` lives on `AppState`.
#[derive(Debug, Default, Clone)]
pub struct TelemetryEmitter;

impl TelemetryEmitter {
    pub fn new() -> Self {
        Self
    }

    /// Emit a structured cleanup telemetry event. Serialized to JSON by
    /// the tracing subscriber's JSON layer where configured; `Debug` is
    /// used otherwise. The existing log-scrub middleware
    /// (`backend/src/middleware/log_scrub.rs`) handles redaction across
    /// every tracing target — we don't bypass it here.
    pub fn emit(&self, event: CleanupTelemetryEvent) {
        let name = event.name();
        // We serialize as JSON so downstream log shippers see structured
        // fields rather than Debug-formatted Rust. If serialization fails
        // (it shouldn't — every variant is plain owned data) we fall
        // back to Debug.
        match serde_json::to_string(&event) {
            Ok(payload) => {
                tracing::info!(
                    target: "cleanup.telemetry",
                    event = name,
                    payload = %payload,
                    "{name}"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "cleanup.telemetry",
                    event = name,
                    error = %e,
                    payload = ?event,
                    "telemetry serialize failed"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// User-id hashing
// ---------------------------------------------------------------------------

/// 8-byte truncated blake3 hex of a user id. Sufficient to correlate
/// sessions, not enough to re-identify the user (collision probability
/// remains low at the per-user-per-month query volume we expect).
/// ADR-017 GDPR alignment.
pub fn hash_user_id(user_id: &str) -> String {
    let h = blake3::hash(user_id.as_bytes());
    let bytes = h.as_bytes();
    let mut out = String::with_capacity(16);
    for b in &bytes[..8] {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Same primitive for account ids (used by `CleanupPlanRefreshed`).
pub fn hash_account_id(account_id: &str) -> String {
    hash_user_id(account_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn hash_user_id_is_8_bytes_hex() {
        let h = hash_user_id("user-1");
        assert_eq!(h.len(), 16, "8 bytes = 16 hex chars");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_user_id_stable() {
        assert_eq!(hash_user_id("user-1"), hash_user_id("user-1"));
        assert_ne!(hash_user_id("user-1"), hash_user_id("user-2"));
    }

    #[test]
    fn hash_user_id_does_not_leak_input() {
        let raw = "fastnsilver@gmail.com";
        let h = hash_user_id(raw);
        assert!(!h.contains("fastnsilver"));
        assert!(!h.contains("@"));
    }

    #[test]
    fn telemetry_event_serializes_with_snake_case_tag() {
        let event = CleanupTelemetryEvent::CleanupPlanBuilt {
            plan_id: Uuid::now_v7(),
            user_id_hash: hash_user_id("u"),
            total_operations: 10,
            risk_low: 7,
            risk_medium: 2,
            risk_high: 1,
            accounts: 1,
            warnings_count: 0,
            build_duration_ms: 250,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"cleanup_plan_built\""));
        assert!(json.contains("\"total_operations\":10"));
        // No raw user id in the wire payload.
        assert!(!json.contains("\"u\""));
    }

    #[test]
    fn telemetry_event_review_roundtrips() {
        let event = CleanupTelemetryEvent::CleanupPlanReviewed {
            plan_id: Uuid::now_v7(),
            user_id_hash: hash_user_id("u"),
            time_on_review_ms: 30_000,
            expanded_groups: 3,
            samples_viewed: 7,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CleanupTelemetryEvent = serde_json::from_str(&json).unwrap();
        match back {
            CleanupTelemetryEvent::CleanupPlanReviewed {
                time_on_review_ms,
                expanded_groups,
                samples_viewed,
                ..
            } => {
                assert_eq!(time_on_review_ms, 30_000);
                assert_eq!(expanded_groups, 3);
                assert_eq!(samples_viewed, 7);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn emitter_does_not_panic() {
        let e = TelemetryEmitter::new();
        e.emit(CleanupTelemetryEvent::CleanupPlanRefreshed {
            plan_id: Uuid::now_v7(),
            user_id_hash: hash_user_id("u"),
            account_id_hash: hash_account_id("acct-a"),
            reason: "manual".to_string(),
        });
    }
}
