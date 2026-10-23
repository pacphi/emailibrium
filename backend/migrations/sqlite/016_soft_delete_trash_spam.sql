-- Soft-delete and spam/trash tracking for emails
ALTER TABLE emails ADD COLUMN deleted_at TEXT DEFAULT NULL;
ALTER TABLE emails ADD COLUMN is_spam INTEGER NOT NULL DEFAULT 0;
ALTER TABLE emails ADD COLUMN is_trash INTEGER NOT NULL DEFAULT 0;
ALTER TABLE emails ADD COLUMN folder TEXT NOT NULL DEFAULT 'INBOX';

-- Fast filtering: most queries exclude trash/spam
CREATE INDEX idx_emails_active ON emails(account_id)
  WHERE deleted_at IS NULL AND is_trash = 0 AND is_spam = 0;
CREATE INDEX idx_emails_trash ON emails(account_id, deleted_at) WHERE is_trash = 1;
CREATE INDEX idx_emails_spam ON emails(account_id) WHERE is_spam = 1;
CREATE INDEX idx_emails_folder ON emails(account_id, folder);
