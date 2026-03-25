-- Ingestion checkpoint/resume support (DDD-003, audit item #26).
--
-- Tracks the state of ingestion jobs so they can resume from the last
-- successfully processed item after a failure or restart.

CREATE TABLE IF NOT EXISTS ingestion_checkpoints (
    id          TEXT PRIMARY KEY,
    batch_id    TEXT NOT NULL,
    account_id  TEXT NOT NULL,
    stage       TEXT NOT NULL DEFAULT 'syncing',  -- syncing, embedding, categorizing, clustering, analyzing, complete
    status      TEXT NOT NULL DEFAULT 'running',  -- running, paused, completed, failed
    total       INTEGER NOT NULL DEFAULT 0,
    processed   INTEGER NOT NULL DEFAULT 0,
    failed      INTEGER NOT NULL DEFAULT 0,
    last_processed_id  TEXT,                      -- ID of the last successfully processed email
    error_msg   TEXT,                             -- error message if status = 'failed'
    metadata    TEXT,                             -- JSON blob for stage-specific state
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for fast lookup by batch_id (the job ID).
CREATE INDEX IF NOT EXISTS idx_checkpoints_batch_id ON ingestion_checkpoints(batch_id);

-- Index for finding incomplete jobs for a given account.
CREATE INDEX IF NOT EXISTS idx_checkpoints_account_status ON ingestion_checkpoints(account_id, status);

-- Background jobs queue (apalis-compatible schema, audit item #28).
--
-- Stores pending, running, and completed background jobs for content
-- extraction, embedding, CLIP analysis, and email sync.
CREATE TABLE IF NOT EXISTS background_jobs (
    id          TEXT PRIMARY KEY,
    job_type    TEXT NOT NULL,    -- content_extraction, embedding, clip_embedding, sync
    payload     TEXT NOT NULL,    -- JSON-serialized job payload
    status      TEXT NOT NULL DEFAULT 'pending',  -- pending, running, completed, failed, cancelled
    priority    INTEGER NOT NULL DEFAULT 0,
    attempts    INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    error_msg   TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    scheduled_at TEXT,            -- for delayed jobs
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_jobs_status_priority ON background_jobs(status, priority);
CREATE INDEX IF NOT EXISTS idx_jobs_type_status ON background_jobs(job_type, status);
