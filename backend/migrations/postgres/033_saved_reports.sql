-- Migration 033: saved reports. Identical to the sqlite migration of the
-- same number -- see that file for the full rationale.
CREATE TABLE saved_reports (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    account_id TEXT,
    generated_at TEXT NOT NULL,
    content_json TEXT NOT NULL
);

CREATE INDEX idx_saved_reports_kind ON saved_reports(kind);
CREATE INDEX idx_saved_reports_generated_at ON saved_reports(generated_at);
