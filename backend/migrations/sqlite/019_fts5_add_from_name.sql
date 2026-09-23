-- Add from_name to FTS5 index so keyword searches match sender display names.
--
-- FTS5 external-content tables cannot be ALTERed — we must drop and recreate
-- the virtual table along with its sync triggers.

-- Drop existing triggers first.
DROP TRIGGER IF EXISTS emails_ai;
DROP TRIGGER IF EXISTS emails_ad;
DROP TRIGGER IF EXISTS emails_au;

-- Drop the old FTS5 table (no data loss — it is an external-content index).
DROP TABLE IF EXISTS email_fts;

-- Recreate with from_name included.
CREATE VIRTUAL TABLE IF NOT EXISTS email_fts USING fts5(
    id,
    subject,
    from_name,
    from_addr,
    body_text,
    labels,
    content='emails',
    content_rowid='rowid',
    tokenize='porter unicode61'
);

-- Rebuild FTS5 from existing rows.
INSERT INTO email_fts(email_fts) VALUES ('rebuild');

-- Sync triggers — now include from_name.
CREATE TRIGGER IF NOT EXISTS emails_ai AFTER INSERT ON emails BEGIN
    INSERT INTO email_fts(rowid, id, subject, from_name, from_addr, body_text, labels)
    VALUES (new.rowid, new.id, new.subject, new.from_name, new.from_addr, new.body_text, new.labels);
END;

CREATE TRIGGER IF NOT EXISTS emails_ad AFTER DELETE ON emails BEGIN
    INSERT INTO email_fts(email_fts, rowid, id, subject, from_name, from_addr, body_text, labels)
    VALUES ('delete', old.rowid, old.id, old.subject, old.from_name, old.from_addr, old.body_text, old.labels);
END;

CREATE TRIGGER IF NOT EXISTS emails_au AFTER UPDATE ON emails BEGIN
    INSERT INTO email_fts(email_fts, rowid, id, subject, from_name, from_addr, body_text, labels)
    VALUES ('delete', old.rowid, old.id, old.subject, old.from_name, old.from_addr, old.body_text, old.labels);
    INSERT INTO email_fts(rowid, id, subject, from_name, from_addr, body_text, labels)
    VALUES (new.rowid, new.id, new.subject, new.from_name, new.from_addr, new.body_text, new.labels);
END;
