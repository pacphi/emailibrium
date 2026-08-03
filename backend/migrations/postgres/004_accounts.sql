-- Migration 004: Account Management tables (DDD-005)
--
-- Dialect notes (ADR-033): encrypted_access_token/encrypted_refresh_token are SQLite's
-- binary-blob column -> BYTEA (Postgres). created_at/updated_at stay TEXT (matching the
-- SQLite schema's own type choice, not TIMESTAMP) with their SQLite now-timestamp default
-- ported to an expression producing the identical 'YYYY-MM-DD HH:MI:SS' UTC string shape,
-- since downstream Rust code parses these columns as plain strings in that exact format.

CREATE TABLE IF NOT EXISTS connected_accounts (
    id                       TEXT PRIMARY KEY NOT NULL,
    provider                 TEXT NOT NULL CHECK (provider IN ('gmail', 'outlook', 'imap', 'pop3')),
    email_address            TEXT NOT NULL UNIQUE,
    encrypted_access_token   BYTEA,
    encrypted_refresh_token  BYTEA,
    token_expires_at         TEXT,
    status                   TEXT NOT NULL DEFAULT 'connected'
                             CHECK (status IN ('connected', 'disconnected', 'error', 'suspended')),
    archive_strategy         TEXT NOT NULL DEFAULT 'delayed',
    label_prefix             TEXT NOT NULL DEFAULT 'EM/',
    created_at               TEXT NOT NULL DEFAULT (to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')),
    updated_at               TEXT NOT NULL DEFAULT (to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS sync_state (
    account_id       TEXT PRIMARY KEY NOT NULL REFERENCES connected_accounts(id) ON DELETE CASCADE,
    last_sync_at     TEXT,
    history_id       TEXT,
    next_page_token  TEXT,
    emails_synced    INTEGER NOT NULL DEFAULT 0,
    sync_failures    INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    status           TEXT NOT NULL DEFAULT 'idle'
                     CHECK (status IN ('idle', 'syncing', 'error')),
    updated_at       TEXT NOT NULL DEFAULT (to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE INDEX IF NOT EXISTS idx_connected_accounts_provider ON connected_accounts(provider);
CREATE INDEX IF NOT EXISTS idx_connected_accounts_status ON connected_accounts(status);
