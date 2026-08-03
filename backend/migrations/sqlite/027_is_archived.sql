-- Add is_archived boolean column to emails table and backfill from labels field.
ALTER TABLE emails ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT 0;
UPDATE emails SET is_archived = 1 WHERE (',' || COALESCE(labels, '') || ',') LIKE '%,ARCHIVED,%';
CREATE INDEX IF NOT EXISTS idx_emails_is_archived ON emails(is_archived);
