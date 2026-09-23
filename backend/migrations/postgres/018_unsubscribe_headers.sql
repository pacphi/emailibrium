-- Add List-Unsubscribe header columns to support RFC 2369/8058 one-click unsubscribe.

ALTER TABLE emails ADD COLUMN list_unsubscribe TEXT DEFAULT NULL;
ALTER TABLE emails ADD COLUMN list_unsubscribe_post TEXT DEFAULT NULL;

-- Index for efficient subscription detection (non-null = has unsubscribe header).
CREATE INDEX IF NOT EXISTS idx_emails_list_unsubscribe ON emails(list_unsubscribe)
    WHERE list_unsubscribe IS NOT NULL;
