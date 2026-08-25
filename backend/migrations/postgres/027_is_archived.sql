-- Add is_archived boolean column to emails table and backfill from labels field.
-- Dialect note (ADR-033): Postgres's real BOOLEAN type rejects an integer literal
-- default/assignment (unlike SQLite, where BOOLEAN is just INTEGER affinity) --
-- 0/1 -> FALSE/TRUE.
ALTER TABLE emails ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE emails SET is_archived = TRUE WHERE (',' || COALESCE(labels, '') || ',') LIKE '%,ARCHIVED,%';
CREATE INDEX IF NOT EXISTS idx_emails_is_archived ON emails(is_archived);
