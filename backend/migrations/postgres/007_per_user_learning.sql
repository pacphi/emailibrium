-- Per-user learning models (DDD-004, item #27).

CREATE TABLE IF NOT EXISTS user_learning_models (
    user_id TEXT NOT NULL,
    category TEXT NOT NULL,
    offset_json TEXT NOT NULL,
    feedback_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, category)
);

CREATE INDEX IF NOT EXISTS idx_user_learning_user ON user_learning_models(user_id);
