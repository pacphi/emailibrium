-- Migration 028: IMAP connection columns for connected_accounts.
--
-- Enables connecting mailboxes via IMAP + app password (basic auth) with no
-- OAuth cloud setup. The IMAP/app password itself is stored encrypted in the
-- existing `encrypted_access_token` BLOB (reusing the OAuth token column and the
-- same AES-256-GCM path); these columns hold the non-secret connection details.
--
-- SQLite only supports one ADD COLUMN per ALTER TABLE; all columns are nullable
-- (populated only for provider = 'imap').
ALTER TABLE connected_accounts ADD COLUMN imap_host TEXT;
ALTER TABLE connected_accounts ADD COLUMN imap_port INTEGER;
ALTER TABLE connected_accounts ADD COLUMN imap_use_tls INTEGER; -- 0/1 boolean
ALTER TABLE connected_accounts ADD COLUMN smtp_host TEXT;
ALTER TABLE connected_accounts ADD COLUMN smtp_port INTEGER;
