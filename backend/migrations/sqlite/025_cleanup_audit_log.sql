-- Migration 025: Cleanup audit log (Phase D, ADR-030 §Security, DDD-005/ADR-017).
--
-- Append-only audit trail for every cleanup operation outcome (applied,
-- failed, skipped). Required for GDPR right-to-explanation. Cascades on
-- cleanup_plans (migration 024) so right-to-erasure deletes audit rows
-- alongside the plan.
--
-- ADR-030 §Security: "No plan content is logged; only counts." This table
-- DELIBERATELY does not contain email_id, email content, rule body, folder
-- paths, or sample ids. Investigators authorised to inspect the encrypted
-- cleanup_plan_operations table can join via (plan_id, seq) when needed.

CREATE TABLE IF NOT EXISTS cleanup_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,                  -- unix millis
    plan_id BLOB NOT NULL,                       -- UUID v7 from cleanup_plans.id
    job_id BLOB NOT NULL,                        -- UUID v7 from cleanup_apply_jobs.job_id
    user_id BLOB NOT NULL,
    account_id BLOB NOT NULL,
    seq INTEGER NOT NULL,
    op_kind TEXT NOT NULL,                       -- 'materialized' | 'predicate'
    action_type TEXT NOT NULL,                   -- camelCase PlanAction tag
    source_type TEXT NOT NULL,                   -- 'subscription' | 'cluster' | 'rule' | 'archiveStrategy' | 'manual'
    outcome TEXT NOT NULL,                       -- 'applied' | 'failed' | 'skipped'
    skip_reason TEXT,                            -- non-null only when outcome='skipped'
    error_code TEXT,
    error_message TEXT,
    UNIQUE (plan_id, job_id, seq, outcome)
);

CREATE INDEX IF NOT EXISTS idx_cleanup_audit_user ON cleanup_audit_log(user_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_cleanup_audit_plan ON cleanup_audit_log(plan_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_cleanup_audit_job ON cleanup_audit_log(job_id);
