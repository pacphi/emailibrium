-- Migration 032: rule authority levels (docs/plan/mail-agent-2026-10.md
-- section 3/10). Extends the existing rules engine (ADR-014, migration 012)
-- rather than a parallel table, per section 4's own mapping
-- (rule_set_authority -> "rules engine (ADR-014)").
--
-- authority_level: 'suggest' | 'trusted' (level 0 Observe and level 3 Act
-- are not rule states — Observe is "no rule exists yet", Act is not
-- planned). Default 'suggest': every rule starts there per section 3's
-- table ("the default for everything after onboarding").
-- clean_streak: consecutive Apply-once approvals with zero corrections
-- since the last reset; 3 graduates to 'trusted' (section 11 decisions
-- log). An Always approval graduates immediately regardless of streak.
-- graduated_at is TEXT, not a native timestamp type, matching migration
-- 030's account_role columns (ADR-035 2.5): the application writes/parses
-- the string itself.
ALTER TABLE rules ADD COLUMN authority_level TEXT DEFAULT 'suggest';
ALTER TABLE rules ADD COLUMN clean_streak INTEGER DEFAULT 0;
ALTER TABLE rules ADD COLUMN graduated_at TEXT;

-- Graduation/correction history (section 3: "Record graduation history").
CREATE TABLE IF NOT EXISTS rule_graduation_log (
    id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    event TEXT NOT NULL, -- 'always_approved' | 'three_clean_mondays' | 'correction'
    authority_before TEXT NOT NULL,
    authority_after TEXT NOT NULL,
    at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_rule_graduation_log_rule_id ON rule_graduation_log(rule_id);
