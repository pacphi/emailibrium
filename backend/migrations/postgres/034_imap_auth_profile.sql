-- Migration 034: identical to the sqlite migration of the same number — see
-- that file for the full rationale.
ALTER TABLE connected_accounts ADD COLUMN imap_auth_profile TEXT;
