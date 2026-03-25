-- Migration 013: Add sync settings columns to connected_accounts

ALTER TABLE connected_accounts ADD COLUMN sync_depth TEXT NOT NULL DEFAULT '30d';
ALTER TABLE connected_accounts ADD COLUMN sync_frequency INTEGER NOT NULL DEFAULT 5;
