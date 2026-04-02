-- Persist topic clusters so they survive server restarts.
CREATE TABLE IF NOT EXISTS topic_clusters (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    centroid    BLOB NOT NULL,
    email_ids   TEXT NOT NULL DEFAULT '[]',   -- JSON array of email IDs
    email_count INTEGER NOT NULL DEFAULT 0,
    top_terms   TEXT NOT NULL DEFAULT '[]',   -- JSON array of {word, score, count}
    representative_email_ids TEXT NOT NULL DEFAULT '[]', -- JSON array
    stability_score REAL NOT NULL DEFAULT 0.0,
    stability_runs  INTEGER NOT NULL DEFAULT 0,
    is_pinned   INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
