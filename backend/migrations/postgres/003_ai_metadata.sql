-- AI model metadata tracking (ADR-013).
-- Stores key-value pairs for model lifecycle state such as the
-- currently active embedding model.
CREATE TABLE IF NOT EXISTS ai_metadata (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
