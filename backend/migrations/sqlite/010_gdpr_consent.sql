-- GDPR consent decisions and privacy audit log (R-09: Consent Persistence).

-- Consent decisions (who consented to what, when).
CREATE TABLE IF NOT EXISTS consent_decisions (
    id TEXT PRIMARY KEY,
    consent_type TEXT NOT NULL,  -- 'cloud_ai', 'data_export', 'analytics', 'third_party'
    granted INTEGER NOT NULL DEFAULT 0,
    granted_at DATETIME,
    revoked_at DATETIME,
    ip_address TEXT,
    user_agent TEXT,
    created_at DATETIME DEFAULT (datetime('now'))
);

-- Privacy audit log (append-only).
CREATE TABLE IF NOT EXISTS privacy_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,  -- 'data_access', 'data_export', 'data_delete', 'consent_change'
    resource_type TEXT,  -- 'email', 'vector', 'account', 'settings'
    resource_id TEXT,
    actor TEXT DEFAULT 'user',
    details TEXT,  -- JSON
    created_at DATETIME DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_privacy_audit_event ON privacy_audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_privacy_audit_created ON privacy_audit_log(created_at);
