CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    email_id TEXT NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    is_inline BOOLEAN NOT NULL DEFAULT FALSE,
    content_id TEXT,
    storage_path TEXT,
    provider_attachment_id TEXT,
    fetch_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (fetch_status IN ('pending', 'fetched', 'failed')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_attachments_email_id ON attachments(email_id);
CREATE INDEX IF NOT EXISTS idx_attachments_content_id ON attachments(content_id) WHERE content_id IS NOT NULL;
