-- Migration 024: Cleanup Planning subdomain (ADR-030, DDD-008 addendum).
--
-- Stores the immutable, materialized plan artifact produced by PlanBuilder
-- for the Inbox Cleaner Wizard's Plan/Apply flow.
--
-- Encryption note: ADR-016/017 references an "encryption interceptor" pattern
-- that does not exist in this repo's `db/` module today (existing tables such
-- as `topic_clusters` store JSON as plaintext TEXT). For consistency with the
-- repo's current convention, *_json columns here are stored as plaintext TEXT.
-- Switching to encrypted blobs later is additive (column-type swap +
-- serializer); see Phase A follow-up debt note.
--
-- Dialect note (ADR-033): the binary-blob column -> BYTEA (UUID v7 / blake3 hash raw bytes, unchanged shape).

CREATE TABLE IF NOT EXISTS cleanup_plans (
    id              BYTEA PRIMARY KEY,        -- UUID v7 (16 bytes)
    user_id         BYTEA NOT NULL,
    created_at      BIGINT NOT NULL,          -- unix millis
    valid_until     BIGINT NOT NULL,          -- unix millis
    plan_hash       BYTEA NOT NULL,           -- 32 bytes blake3
    status          TEXT NOT NULL,            -- draft|ready|applying|applied|partially_applied|failed|expired|cancelled
    totals_json     TEXT NOT NULL DEFAULT '{}',
    risk_json       TEXT NOT NULL DEFAULT '{}',
    warnings_json   TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS idx_cleanup_plans_user_status ON cleanup_plans(user_id, status);
CREATE INDEX IF NOT EXISTS idx_cleanup_plans_expiry ON cleanup_plans(valid_until);

CREATE TABLE IF NOT EXISTS cleanup_plan_account_etags (
    plan_id     BYTEA NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    account_id  BYTEA NOT NULL,
    etag_kind   TEXT NOT NULL,                -- 'gmail_history' | 'outlook_delta' | 'imap_uvms' | 'none'
    etag_value  TEXT,                         -- JSON-serialized AccountStateEtag payload
    PRIMARY KEY (plan_id, account_id)
);

CREATE TABLE IF NOT EXISTS cleanup_plan_operations (
    plan_id         BYTEA NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    op_kind         TEXT NOT NULL,            -- 'materialized' | 'predicate'
    account_id      BYTEA NOT NULL,
    email_id        BYTEA,                    -- materialized only
    predicate_kind  TEXT,                     -- predicate only
    predicate_id    TEXT,                     -- predicate only
    action          TEXT NOT NULL,            -- JSON-serialized PlanAction
    target_kind     TEXT,                     -- 'folder' | 'label' | NULL
    target_id       TEXT,
    target_name     TEXT,
    source_kind     TEXT NOT NULL,            -- 'subscription' | 'cluster' | 'rule' | 'strategy' | 'manual'
    source_id       TEXT,
    projected_count INTEGER,                  -- predicate only
    sample_ids_json TEXT,                     -- predicate only, JSON array of email_ids
    reverse_op_json TEXT,
    risk            TEXT NOT NULL,            -- 'low' | 'medium' | 'high'
    status          TEXT NOT NULL DEFAULT 'pending',
    skip_reason     TEXT,                     -- when status='skipped'
    applied_at      BIGINT,
    error           TEXT,
    partial_applied INTEGER NOT NULL DEFAULT 0,
    payload_json    TEXT,                     -- full serialized PlannedOperation for round-trip
    PRIMARY KEY (plan_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_cleanup_ops_status ON cleanup_plan_operations(plan_id, status);
CREATE INDEX IF NOT EXISTS idx_cleanup_ops_risk ON cleanup_plan_operations(plan_id, risk, status);
CREATE INDEX IF NOT EXISTS idx_cleanup_ops_account ON cleanup_plan_operations(plan_id, account_id);

CREATE TABLE IF NOT EXISTS cleanup_apply_jobs (
    job_id      BYTEA PRIMARY KEY,
    plan_id     BYTEA NOT NULL REFERENCES cleanup_plans(id) ON DELETE CASCADE,
    started_at  BIGINT NOT NULL,
    finished_at BIGINT,
    state       TEXT NOT NULL,
    risk_max    TEXT NOT NULL,
    counts_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_cleanup_jobs_plan ON cleanup_apply_jobs(plan_id);
