-- Offline-first sync queue and conflict log (R-02).
--
-- Buffers email operations (archive, label, delete, etc.) when the
-- network is unavailable and replays them when connectivity returns.

-- Offline operation queue (buffered when network unavailable).
CREATE TABLE IF NOT EXISTS sync_queue (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL,
    operation_type  TEXT NOT NULL,         -- 'archive', 'label', 'delete', 'mark_read', 'move', 'unsubscribe'
    target_id       TEXT NOT NULL,         -- message ID
    payload         TEXT,                  -- JSON with operation-specific data
    status          TEXT DEFAULT 'pending',-- 'pending', 'processing', 'completed', 'failed', 'conflict'
    retry_count     INTEGER DEFAULT 0,
    max_retries     INTEGER DEFAULT 3,
    created_at      DATETIME DEFAULT (datetime('now')),
    processed_at    DATETIME,
    error           TEXT
);

CREATE INDEX IF NOT EXISTS idx_sync_queue_status  ON sync_queue(status);
CREATE INDEX IF NOT EXISTS idx_sync_queue_account ON sync_queue(account_id);

-- Processing checkpoints for crash recovery (R-06).
CREATE TABLE IF NOT EXISTS processing_checkpoints (
    job_id            TEXT PRIMARY KEY,
    provider          TEXT NOT NULL,
    account_id        TEXT NOT NULL,
    last_processed_id TEXT,
    total_count       INTEGER,
    processed_count   INTEGER NOT NULL DEFAULT 0,
    state             TEXT NOT NULL DEFAULT 'running',
    error_message     TEXT,
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_proc_cp_account ON processing_checkpoints(account_id);
CREATE INDEX IF NOT EXISTS idx_proc_cp_state   ON processing_checkpoints(state);

-- Conflict log for operations that couldn't be auto-resolved.
CREATE TABLE IF NOT EXISTS sync_conflicts (
    id              TEXT PRIMARY KEY,
    queue_entry_id  TEXT NOT NULL REFERENCES sync_queue(id),
    local_state     TEXT NOT NULL,    -- JSON: what we tried to do
    remote_state    TEXT NOT NULL,    -- JSON: what the server has
    resolution      TEXT,             -- 'local_wins', 'remote_wins', 'merged', NULL if unresolved
    resolved_at     DATETIME,
    created_at      DATETIME DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sync_conflicts_queue ON sync_conflicts(queue_entry_id);
CREATE INDEX IF NOT EXISTS idx_sync_conflicts_unresolved ON sync_conflicts(resolution) WHERE resolution IS NULL;
