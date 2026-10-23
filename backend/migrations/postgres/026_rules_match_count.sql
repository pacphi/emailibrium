-- Track how many emails each rule has matched across all manual runs.
-- Dialect note (ADR-033): SQLite's datetime-affinity type -> TIMESTAMPTZ.
ALTER TABLE rules ADD COLUMN match_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE rules ADD COLUMN last_run_at TIMESTAMPTZ;
