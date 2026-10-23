-- Fix sync_frequency to use seconds instead of minutes.
-- Existing accounts have small values (1-60) that were interpreted as minutes.
-- Convert them: any value < 60 is assumed to be minutes, multiply by 60.
UPDATE connected_accounts
SET sync_frequency = sync_frequency * 60
WHERE sync_frequency > 0 AND sync_frequency < 60;
