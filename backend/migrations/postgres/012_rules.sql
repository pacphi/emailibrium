-- Rules table for the R-03 Rule Engine.
-- Dialect note (ADR-033): SQLite's datetime-affinity type -> TIMESTAMPTZ, SQLite's now-timestamp default -> now().
CREATE TABLE IF NOT EXISTS rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT DEFAULT '',
    conditions_json TEXT NOT NULL,
    actions_json TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    enabled INTEGER DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_rules_priority ON rules(priority DESC);
CREATE INDEX IF NOT EXISTS idx_rules_enabled ON rules(enabled);
