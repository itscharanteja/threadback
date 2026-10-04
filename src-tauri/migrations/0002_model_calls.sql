-- One row per request to the model. It holds numbers, a model name, a host,
-- and fixed labels: never the evidence sent, the reply, or any credential.
CREATE TABLE IF NOT EXISTS model_calls (
    id TEXT PRIMARY KEY,
    checkpoint_id TEXT REFERENCES checkpoints(id) ON DELETE CASCADE,
    at TEXT NOT NULL,
    model_id TEXT NOT NULL,
    endpoint_host TEXT NOT NULL,
    status TEXT NOT NULL,
    http_status INTEGER,
    latency_ms INTEGER NOT NULL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    total_tokens INTEGER,
    finish_reason TEXT,
    structured_output TEXT NOT NULL,
    attempt INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_model_calls_at ON model_calls(at);
CREATE INDEX IF NOT EXISTS idx_model_calls_checkpoint ON model_calls(checkpoint_id);

INSERT OR IGNORE INTO schema_migrations(version, applied_at)
VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
