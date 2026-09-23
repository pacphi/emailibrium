-- Migration 023: App-level settings key-value store for persisting user
-- preferences (e.g. selected LLM model) across server restarts.

CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
