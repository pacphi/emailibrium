-- Migration 030: account role (docs/plan/mail-agent-2026-10.md section 6/10).
--
-- Identical to the sqlite migration of the same number — see that file for
-- the full rationale. Postgres allows combining ADD COLUMN statements, but
-- keeping this one-per-line matches the sqlite migration for easy diffing.
-- role_declared_at is TEXT, not a native timestamp type, matching every
-- other temporal column on this table (ADR-035 §2.5).
ALTER TABLE connected_accounts ADD COLUMN role TEXT;
ALTER TABLE connected_accounts ADD COLUMN role_declared_at TEXT;
