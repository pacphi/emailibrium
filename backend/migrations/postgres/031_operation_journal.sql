-- Migration 031: operation journal. Identical to the sqlite migration of
-- the same number -- see that file for the full rationale.
CREATE TABLE operation_journal (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    batch_key TEXT NOT NULL,
    items_json TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    undone_at TEXT
);

CREATE INDEX idx_operation_journal_batch_key ON operation_journal(batch_key);
CREATE INDEX idx_operation_journal_account ON operation_journal(account_id);
