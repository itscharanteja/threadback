CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    canonical_root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_policies (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    exclusions_json TEXT NOT NULL DEFAULT '[]',
    retention_days INTEGER NOT NULL DEFAULT 14 CHECK (retention_days > 0),
    capture_git INTEGER NOT NULL DEFAULT 1 CHECK (capture_git IN (0, 1)),
    capture_commands INTEGER NOT NULL DEFAULT 1 CHECK (capture_commands IN (0, 1)),
    capture_file_metadata INTEGER NOT NULL DEFAULT 1 CHECK (capture_file_metadata IN (0, 1)),
    redaction_patterns_json TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS work_threads (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    goal TEXT NOT NULL CHECK (length(trim(goal)) > 0),
    state TEXT NOT NULL CHECK (
        state IN ('ACTIVE', 'PAUSED', 'CHECKPOINT_DRAFT', 'READY', 'RESUMED', 'ARCHIVED', 'FAILED', 'DELETED')
    ),
    started_at TEXT NOT NULL,
    ended_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES work_threads(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    sensitivity TEXT NOT NULL,
    content_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artifact_refs (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    artifact_type TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    line_hint INTEGER
);

CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES work_threads(id) ON DELETE CASCADE,
    start_event_id TEXT NOT NULL REFERENCES events(id),
    end_event_id TEXT NOT NULL REFERENCES events(id),
    status TEXT NOT NULL,
    model_id TEXT,
    prompt_version TEXT,
    payload_json TEXT NOT NULL,
    draft_json TEXT,
    approved_json TEXT,
    created_at TEXT NOT NULL,
    approved_at TEXT
);

CREATE TABLE IF NOT EXISTS corrections (
    id TEXT PRIMARY KEY,
    checkpoint_id TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    field_path TEXT NOT NULL,
    before_json TEXT NOT NULL,
    after_json TEXT NOT NULL,
    reason TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_threads_project_updated
    ON work_threads(project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_threads_state_updated
    ON work_threads(state, updated_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_thread
    ON work_threads((1)) WHERE state = 'ACTIVE';
CREATE INDEX IF NOT EXISTS idx_events_thread_time
    ON events(thread_id, timestamp, id);
CREATE INDEX IF NOT EXISTS idx_checkpoints_thread_created
    ON checkpoints(thread_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_checkpoints_status_created
    ON checkpoints(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_corrections_checkpoint_created
    ON corrections(checkpoint_id, created_at);

INSERT OR IGNORE INTO schema_migrations(version, applied_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
