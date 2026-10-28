-- Migration 033: saved reports (docs/plan/mail-agent-2026-10.md section 8,
-- "also saved as a page" for the daily brief, generalised to the Monday
-- review and monthly health report too).
--
-- kind: 'brief' | 'review' | 'health'. account_id is NULL when the report
-- merges every connected account (brief/review's default scope today) --
-- unconstrained, same as operation_journal's own account_id (migration
-- 031), not a foreign key. generated_at is TEXT, matching every other
-- temporal column in this schema (ADR-035 section 2.5): the application
-- writes/parses the string itself. content_json is the tool's full JSON
-- response body, verbatim, so a saved report reads back byte-identical to
-- what the caller saw at generation time.
CREATE TABLE saved_reports (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    account_id TEXT,
    generated_at TEXT NOT NULL,
    content_json TEXT NOT NULL
);

CREATE INDEX idx_saved_reports_kind ON saved_reports(kind);
CREATE INDEX idx_saved_reports_generated_at ON saved_reports(generated_at);
