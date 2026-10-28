-- Migration 030: account role (docs/plan/mail-agent-2026-10.md section 6/10).
--
-- The chief-of-staff onboarding asks "what is this address for?" once, and
-- the answer drives playbook defaults, protected list, privacy posture and
-- retention. This migration is the storage side only (milestone 1); the
-- onboarding question itself is a later UI milestone.
--
-- Nullable: existing accounts have no role until re-onboarded or explicitly
-- set. 'personal' | 'professional' | 'business' | 'shared' | 'retired'
-- (docs/plan/mail-agent-2026-10.md section 6's exact five options);
-- enforced in Rust (AccountRole), not a DB CHECK constraint, matching how
-- imap_encryption above is also an unconstrained TEXT column.
-- role_declared_at is TEXT, not a native timestamp type, matching every
-- other temporal column on this table (ADR-035 §2.5): the application
-- writes/parses the string itself rather than relying on dialect-specific
-- timestamp handling.
ALTER TABLE connected_accounts ADD COLUMN role TEXT;
ALTER TABLE connected_accounts ADD COLUMN role_declared_at TEXT;
