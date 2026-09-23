-- A/B evaluation infrastructure (ADR-004, item #22).

CREATE TABLE IF NOT EXISTS ab_tests (
    test_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    variant_a_config TEXT NOT NULL,
    variant_b_config TEXT NOT NULL,
    traffic_split REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'running',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    concluded_at TIMESTAMP,
    metrics_a TEXT NOT NULL DEFAULT '{}',
    metrics_b TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS ab_test_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    test_id TEXT NOT NULL REFERENCES ab_tests(test_id),
    variant TEXT NOT NULL CHECK(variant IN ('a', 'b')),
    timestamp TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    mrr REAL,
    precision_at_k REAL,
    recall_at_k REAL,
    ndcg REAL
);

CREATE INDEX IF NOT EXISTS idx_ab_results_test ON ab_test_results(test_id);
