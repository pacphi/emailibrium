-- Migration 032: rule authority levels. Identical to the sqlite migration
-- of the same number -- see that file for the full rationale.
ALTER TABLE rules ADD COLUMN authority_level TEXT DEFAULT 'suggest';
ALTER TABLE rules ADD COLUMN clean_streak INTEGER DEFAULT 0;
ALTER TABLE rules ADD COLUMN graduated_at TEXT;

CREATE TABLE rule_graduation_log (
    id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    event TEXT NOT NULL,
    authority_before TEXT NOT NULL,
    authority_after TEXT NOT NULL,
    at TEXT NOT NULL
);

CREATE INDEX idx_rule_graduation_log_rule_id ON rule_graduation_log(rule_id);
