-- ADR-029 Phase A: Set persistent BM25 column weights for FTS5 ranking.
-- Columns: id(0), subject(10), from_name(5), from_addr(3), body_text(1), labels(2)
INSERT INTO email_fts(email_fts, rank) VALUES('rank', 'bm25(0.0, 10.0, 5.0, 3.0, 1.0, 2.0)');
