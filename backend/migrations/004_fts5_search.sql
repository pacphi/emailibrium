-- FTS5 full-text search for emails (ADR-001: Hybrid Search Architecture).
--
-- Uses the content-sync (external content) pattern so FTS5 reads from the
-- existing `emails` table.  Triggers keep the index in sync on INSERT,
-- UPDATE, and DELETE.

CREATE VIRTUAL TABLE IF NOT EXISTS email_fts USING fts5(
    email_id,
    subject,
    from_addr,
    body_text,
    labels,
    content='emails',
    content_rowid='rowid',
    tokenize='porter unicode61'
);

-- Populate FTS5 from existing rows.
INSERT INTO email_fts(email_fts) VALUES ('rebuild');

-- Keep FTS5 in sync: AFTER INSERT
CREATE TRIGGER IF NOT EXISTS emails_ai AFTER INSERT ON emails BEGIN
    INSERT INTO email_fts(rowid, email_id, subject, from_addr, body_text, labels)
    VALUES (new.rowid, new.id, new.subject, new.from_addr, new.body_text, new.labels);
END;

-- Keep FTS5 in sync: AFTER DELETE
CREATE TRIGGER IF NOT EXISTS emails_ad AFTER DELETE ON emails BEGIN
    INSERT INTO email_fts(email_fts, rowid, email_id, subject, from_addr, body_text, labels)
    VALUES ('delete', old.rowid, old.id, old.subject, old.from_addr, old.body_text, old.labels);
END;

-- Keep FTS5 in sync: AFTER UPDATE (delete old, insert new)
CREATE TRIGGER IF NOT EXISTS emails_au AFTER UPDATE ON emails BEGIN
    INSERT INTO email_fts(email_fts, rowid, email_id, subject, from_addr, body_text, labels)
    VALUES ('delete', old.rowid, old.id, old.subject, old.from_addr, old.body_text, old.labels);
    INSERT INTO email_fts(rowid, email_id, subject, from_addr, body_text, labels)
    VALUES (new.rowid, new.id, new.subject, new.from_addr, new.body_text, new.labels);
END;
