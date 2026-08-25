-- AI consent and audit log tables (ADR-012: Generative AI Integration).

CREATE TABLE IF NOT EXISTS ai_consent (
    provider TEXT PRIMARY KEY,
    consented_at TIMESTAMP NOT NULL,
    revoked_at TIMESTAMP,
    acknowledgment TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    input_token_count INTEGER,
    output_token_count INTEGER,
    input_hash TEXT,
    latency_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON ai_audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_provider ON ai_audit_log(provider);
