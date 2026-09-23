-- Add thread awareness to emails (ADR-029).
-- thread_key groups emails belonging to the same conversation.
ALTER TABLE emails ADD COLUMN thread_key TEXT;
CREATE INDEX IF NOT EXISTS idx_emails_thread_key ON emails(thread_key);

-- Backfill thread_key for existing emails.
-- Use the email's own id as thread_key (proper derivation happens at ingestion time).
UPDATE emails SET thread_key = id WHERE thread_key IS NULL;
