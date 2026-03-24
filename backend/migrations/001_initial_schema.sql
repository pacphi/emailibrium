-- Initial schema for Emailibrium
-- Creates core email table and vector tracking columns.

CREATE TABLE IF NOT EXISTS emails (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    message_id TEXT,
    thread_id TEXT,
    subject TEXT NOT NULL DEFAULT '',
    from_addr TEXT NOT NULL DEFAULT '',
    from_name TEXT,
    to_addrs TEXT NOT NULL DEFAULT '',
    cc_addrs TEXT DEFAULT '',
    bcc_addrs TEXT DEFAULT '',
    received_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    body_text TEXT DEFAULT '',
    body_html TEXT DEFAULT '',
    labels TEXT DEFAULT '',
    is_read BOOLEAN DEFAULT FALSE,
    is_starred BOOLEAN DEFAULT FALSE,
    has_attachments BOOLEAN DEFAULT FALSE,
    -- Vector embedding tracking (Sprint 1)
    embedding_status TEXT DEFAULT 'pending'
        CHECK (embedding_status IN ('pending', 'embedded', 'failed', 'stale')),
    embedded_at TIMESTAMP,
    embedding_model TEXT,
    vector_id TEXT,
    -- Classification
    category TEXT DEFAULT 'Uncategorized',
    category_confidence REAL,
    category_method TEXT,
    -- Timestamps
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_emails_account ON emails(account_id);
CREATE INDEX IF NOT EXISTS idx_emails_from ON emails(from_addr);
CREATE INDEX IF NOT EXISTS idx_emails_received ON emails(received_at);
CREATE INDEX IF NOT EXISTS idx_emails_embedding_status ON emails(embedding_status);
CREATE INDEX IF NOT EXISTS idx_emails_category ON emails(category);
CREATE INDEX IF NOT EXISTS idx_emails_thread ON emails(thread_id);

-- Vector backup table (ADR-003: SQLite backup for vectors)
CREATE TABLE IF NOT EXISTS vector_backups (
    vector_id TEXT PRIMARY KEY,
    email_id TEXT NOT NULL REFERENCES emails(id),
    collection TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector_data BLOB NOT NULL,
    metadata_json TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vector_backups_email ON vector_backups(email_id);
CREATE INDEX IF NOT EXISTS idx_vector_backups_collection ON vector_backups(collection);

-- Category centroids for vector-based classification (ADR-004)
CREATE TABLE IF NOT EXISTS category_centroids (
    category TEXT PRIMARY KEY,
    vector_data BLOB NOT NULL,
    dimensions INTEGER NOT NULL,
    email_count INTEGER NOT NULL DEFAULT 0,
    feedback_count INTEGER NOT NULL DEFAULT 0,
    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Search interaction history for SONA learning (Sprint 3)
CREATE TABLE IF NOT EXISTS search_interactions (
    id TEXT PRIMARY KEY,
    query_text TEXT NOT NULL,
    query_vector_id TEXT,
    result_email_id TEXT REFERENCES emails(id),
    result_rank INTEGER,
    clicked BOOLEAN DEFAULT FALSE,
    feedback TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_search_interactions_query ON search_interactions(query_text);
