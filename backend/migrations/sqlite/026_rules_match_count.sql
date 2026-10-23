-- Track how many emails each rule has matched across all manual runs.
ALTER TABLE rules ADD COLUMN match_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE rules ADD COLUMN last_run_at DATETIME;
