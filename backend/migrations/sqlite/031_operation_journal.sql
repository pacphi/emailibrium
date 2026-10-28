-- Migration 031: operation journal (docs/plan/mail-agent-2026-10.md section 10,
-- milestone 3: "close-out journal", generalised to also cover era-archive
-- apply/undo).
--
-- The existing cleanup_plan_operations.reverse_op column (added by ADR-030)
-- is computed at expansion time but never executed anywhere in the
-- codebase today -- there is no working undo for the general rule/cluster
-- plan system yet. Rather than inherit that gap, milestone 3's era-archive
-- and closeout tools are self-contained: they call the same EmailProvider
-- methods the orchestrator itself calls, and record what they did here so
-- it can be undone. `kind` distinguishes the two callers; `batch_key` is
-- the hash the caller confirmed before apply (same role as a plan hash).
-- `items_json` is a JSON array of {email_id, reverse: "unarchive"|"mark_unread"}
-- -- the minimal reverse actions milestone 3 actually implements.
CREATE TABLE operation_journal (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    kind TEXT NOT NULL, -- 'era_archive' | 'closeout'
    batch_key TEXT NOT NULL,
    items_json TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    undone_at TEXT
);

CREATE INDEX idx_operation_journal_batch_key ON operation_journal(batch_key);
CREATE INDEX idx_operation_journal_account ON operation_journal(account_id);
