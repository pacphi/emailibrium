-- Cloud API audit logging (ADR-008, ADR-012, item #39).

CREATE TABLE IF NOT EXISTS cloud_api_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER,
    output_tokens INTEGER,
    latency_ms INTEGER NOT NULL,
    user_id TEXT,
    request_type TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_cloud_audit_timestamp ON cloud_api_audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_cloud_audit_provider ON cloud_api_audit_log(provider);
CREATE INDEX IF NOT EXISTS idx_cloud_audit_user ON cloud_api_audit_log(user_id);
