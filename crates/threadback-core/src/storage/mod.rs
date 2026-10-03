use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    ArtifactRef, Checkpoint, CheckpointId, Correction, DomainError, EventId, EventPage,
    EventPayload, Project, ProjectId, Sensitivity, ThreadEvent, ThreadId, ThreadState, WorkThread,
};

const MIGRATION_0001: &str = include_str!("../../../../src-tauri/migrations/0001_initial.sql");

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DomainError> {
        let connection = Connection::open(path).map_err(storage_error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = FULL;
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(storage_error)?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn open_in_memory() -> Result<Self, DomainError> {
        let connection = Connection::open_in_memory().map_err(storage_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(storage_error)?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn migrate(&self) -> Result<(), DomainError> {
        self.connection
            .execute_batch(MIGRATION_0001)
            .map_err(storage_error)
    }

    /// Runs `operation` atomically. Every write inside either commits together
    /// or rolls back. Not reentrant.
    pub fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&Database) -> Result<T, DomainError>,
    ) -> Result<T, DomainError> {
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(storage_error)?;
        match operation(self) {
            Ok(value) => {
                if let Err(error) = self.connection.execute_batch("COMMIT") {
                    let _ = self.connection.execute_batch("ROLLBACK");
                    return Err(storage_error(error));
                }
                Ok(value)
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn insert_project(&self, project: &Project) -> Result<(), DomainError> {
        self.connection
            .execute(
                "INSERT INTO projects (id, display_name, canonical_root, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project.id.0,
                    project.display_name,
                    project.canonical_root,
                    project.created_at.to_rfc3339(),
                    project.updated_at.to_rfc3339(),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn find_project(&self, id: &ProjectId) -> Result<Option<Project>, DomainError> {
        self.connection
            .query_row(
                "SELECT id, display_name, canonical_root, created_at, updated_at
                 FROM projects WHERE id = ?1",
                [&id.0],
                |row| {
                    Ok(Project {
                        id: ProjectId(row.get(0)?),
                        display_name: row.get(1)?,
                        canonical_root: row.get(2)?,
                        created_at: parse_timestamp(row.get::<_, String>(3)?)?,
                        updated_at: parse_timestamp(row.get::<_, String>(4)?)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn find_project_by_root(
        &self,
        canonical_root: &str,
    ) -> Result<Option<Project>, DomainError> {
        self.connection
            .query_row(
                "SELECT id, display_name, canonical_root, created_at, updated_at
                 FROM projects WHERE canonical_root = ?1",
                [canonical_root],
                |row| {
                    Ok(Project {
                        id: ProjectId(row.get(0)?),
                        display_name: row.get(1)?,
                        canonical_root: row.get(2)?,
                        created_at: parse_timestamp(row.get::<_, String>(3)?)?,
                        updated_at: parse_timestamp(row.get::<_, String>(4)?)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, DomainError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, display_name, canonical_root, created_at, updated_at
                 FROM projects ORDER BY display_name, id",
            )
            .map_err(storage_error)?;
        let projects = statement
            .query_map([], |row| {
                Ok(Project {
                    id: ProjectId(row.get(0)?),
                    display_name: row.get(1)?,
                    canonical_root: row.get(2)?,
                    created_at: parse_timestamp(row.get::<_, String>(3)?)?,
                    updated_at: parse_timestamp(row.get::<_, String>(4)?)?,
                })
            })
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)?;
        Ok(projects)
    }

    pub fn insert_thread(&self, thread: &WorkThread) -> Result<(), DomainError> {
        if thread.goal.trim().is_empty() {
            return Err(DomainError::invalid_input(
                "A thread requires an intended outcome.",
                "goal",
            ));
        }

        self.connection
            .execute(
                "INSERT INTO work_threads
                 (id, project_id, title, goal, state, started_at, ended_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    thread.id.0,
                    thread.project_id.0,
                    thread.title,
                    thread.goal,
                    state_to_str(&thread.state),
                    thread.started_at.to_rfc3339(),
                    thread.ended_at.map(|value| value.to_rfc3339()),
                    thread.created_at.to_rfc3339(),
                    thread.updated_at.to_rfc3339(),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn find_thread(&self, id: &ThreadId) -> Result<Option<WorkThread>, DomainError> {
        self.connection
            .query_row(
                "SELECT id, project_id, title, goal, state, started_at, ended_at, created_at, updated_at
                 FROM work_threads WHERE id = ?1",
                [&id.0],
                thread_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    /// The thread currently recording evidence, if any.
    pub fn find_live_thread(&self) -> Result<Option<WorkThread>, DomainError> {
        self.connection
            .query_row(
                "SELECT id, project_id, title, goal, state, started_at, ended_at, created_at, updated_at
                 FROM work_threads WHERE state IN ('ACTIVE', 'RESUMED') LIMIT 1",
                [],
                thread_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    /// Most recently updated first.
    pub fn list_threads(&self) -> Result<Vec<WorkThread>, DomainError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, title, goal, state, started_at, ended_at, created_at, updated_at
                 FROM work_threads ORDER BY updated_at DESC, id",
            )
            .map_err(storage_error)?;
        statement
            .query_map([], thread_from_row)
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    pub fn list_live_threads(&self) -> Result<Vec<WorkThread>, DomainError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, title, goal, state, started_at, ended_at, created_at, updated_at
                 FROM work_threads WHERE state IN ('ACTIVE', 'RESUMED') ORDER BY started_at, id",
            )
            .map_err(storage_error)?;
        statement
            .query_map([], thread_from_row)
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    pub fn latest_event_timestamp(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<DateTime<Utc>>, DomainError> {
        self.connection
            .query_row(
                "SELECT timestamp FROM events WHERE thread_id = ?1
                 ORDER BY timestamp DESC, id DESC LIMIT 1",
                [&thread_id.0],
                |row| parse_timestamp(row.get::<_, String>(0)?),
            )
            .optional()
            .map_err(storage_error)
    }

    /// Moves a thread between states only if it is still in `from`, so a stale
    /// caller cannot overwrite a concurrent transition.
    pub fn update_thread_state(
        &self,
        id: &ThreadId,
        from: &ThreadState,
        to: &ThreadState,
        ended_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let changed = self
            .connection
            .execute(
                "UPDATE work_threads SET state = ?1, ended_at = ?2, updated_at = ?3
                 WHERE id = ?4 AND state = ?5",
                params![
                    state_to_str(to),
                    ended_at.map(|value| value.to_rfc3339()),
                    updated_at.to_rfc3339(),
                    id.0,
                    state_to_str(from),
                ],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            return Err(DomainError::invalid_state(
                "The thread changed while this action was in progress. Reload and try again.",
            ));
        }
        Ok(())
    }

    pub fn latest_checkpoint_id(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<CheckpointId>, DomainError> {
        self.connection
            .query_row(
                "SELECT id FROM checkpoints WHERE thread_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [&thread_id.0],
                |row| Ok(CheckpointId(row.get(0)?)),
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn insert_event(&self, event: &ThreadEvent) -> Result<(), DomainError> {
        let payload = serde_json::to_string(&event.payload).map_err(json_error)?;
        self.connection
            .execute(
                "INSERT INTO events
                 (id, thread_id, event_type, timestamp, payload_json, sensitivity, content_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    event.id.0,
                    event.thread_id.0,
                    event_type(&event.payload),
                    event.timestamp.to_rfc3339(),
                    payload,
                    sensitivity_to_str(&event.sensitivity),
                    event.content_hash,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn list_events(
        &self,
        thread_id: &ThreadId,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EventPage, DomainError> {
        let limit = limit.clamp(1, 200);
        let requested = i64::from(limit) + 1;
        let mut events = if let Some(cursor) = cursor {
            let (timestamp, event_id) = decode_cursor(cursor)?;
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, thread_id, timestamp, payload_json, sensitivity, content_hash
                     FROM events
                     WHERE thread_id = ?1
                       AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                     ORDER BY timestamp, id LIMIT ?4",
                )
                .map_err(storage_error)?;
            statement
                .query_map(
                    params![thread_id.0, timestamp, event_id, requested],
                    event_from_row,
                )
                .map_err(storage_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage_error)?
        } else {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, thread_id, timestamp, payload_json, sensitivity, content_hash
                     FROM events WHERE thread_id = ?1
                     ORDER BY timestamp, id LIMIT ?2",
                )
                .map_err(storage_error)?;
            statement
                .query_map(params![thread_id.0, requested], event_from_row)
                .map_err(storage_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage_error)?
        };

        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next_cursor = has_more.then(|| events.last().map(encode_cursor)).flatten();

        Ok(EventPage {
            events,
            next_cursor,
        })
    }

    pub fn insert_artifact(&self, artifact: &ArtifactRef) -> Result<(), DomainError> {
        self.connection
            .execute(
                "INSERT INTO artifact_refs
                 (id, event_id, artifact_type, relative_path, content_hash, line_hint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    artifact.id.0,
                    artifact.event_id.0,
                    artifact.artifact_type,
                    artifact.relative_path,
                    artifact.content_hash,
                    artifact.line_hint,
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn insert_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), DomainError> {
        let payload = serde_json::to_string(&checkpoint.payload).map_err(json_error)?;
        let draft = checkpoint
            .draft
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(json_error)?;
        let approved = checkpoint
            .approved
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(json_error)?;
        self.connection
            .execute(
                "INSERT INTO checkpoints
                 (id, thread_id, start_event_id, end_event_id, status, model_id, prompt_version,
                  payload_json, draft_json, approved_json, created_at, approved_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    checkpoint.id.0,
                    checkpoint.thread_id.0,
                    checkpoint.start_event_id.0,
                    checkpoint.end_event_id.0,
                    checkpoint.status,
                    checkpoint.model_id,
                    checkpoint.prompt_version,
                    payload,
                    draft,
                    approved,
                    checkpoint.created_at.to_rfc3339(),
                    checkpoint.approved_at.map(|value| value.to_rfc3339()),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn find_checkpoint(&self, id: &CheckpointId) -> Result<Option<Checkpoint>, DomainError> {
        self.connection
            .query_row(
                "SELECT id, thread_id, start_event_id, end_event_id, status, model_id,
                        prompt_version, payload_json, draft_json, approved_json, created_at, approved_at
                 FROM checkpoints WHERE id = ?1",
                [&id.0],
                checkpoint_from_row,
            )
            .optional()
            .map_err(storage_error)
    }

    /// Every event of a thread, oldest first.
    pub fn list_all_events(&self, thread_id: &ThreadId) -> Result<Vec<ThreadEvent>, DomainError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, thread_id, timestamp, payload_json, sensitivity, content_hash
                 FROM events WHERE thread_id = ?1 ORDER BY timestamp, id",
            )
            .map_err(storage_error)?;
        statement
            .query_map([&thread_id.0], event_from_row)
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    /// Newest first.
    pub fn list_checkpoints(&self, thread_id: &ThreadId) -> Result<Vec<Checkpoint>, DomainError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, thread_id, start_event_id, end_event_id, status, model_id,
                        prompt_version, payload_json, draft_json, approved_json, created_at, approved_at
                 FROM checkpoints WHERE thread_id = ?1 ORDER BY created_at DESC, id DESC",
            )
            .map_err(storage_error)?;
        statement
            .query_map([&thread_id.0], checkpoint_from_row)
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    pub fn insert_correction(&self, correction: &Correction) -> Result<(), DomainError> {
        let before = serde_json::to_string(&correction.before).map_err(json_error)?;
        let after = serde_json::to_string(&correction.after).map_err(json_error)?;
        self.connection
            .execute(
                "INSERT INTO corrections
                 (id, checkpoint_id, field_path, before_json, after_json, reason, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    correction.id.0,
                    correction.checkpoint_id.0,
                    correction.field_path,
                    before,
                    after,
                    correction.reason,
                    correction.created_at.to_rfc3339(),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

fn thread_from_row(row: &Row<'_>) -> rusqlite::Result<WorkThread> {
    let ended_at: Option<String> = row.get(6)?;
    Ok(WorkThread {
        id: ThreadId(row.get(0)?),
        project_id: ProjectId(row.get(1)?),
        title: row.get(2)?,
        goal: row.get(3)?,
        state: str_to_state(row.get::<_, String>(4)?)?,
        started_at: parse_timestamp(row.get::<_, String>(5)?)?,
        ended_at: ended_at.map(parse_timestamp).transpose()?,
        created_at: parse_timestamp(row.get::<_, String>(7)?)?,
        updated_at: parse_timestamp(row.get::<_, String>(8)?)?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<ThreadEvent> {
    let payload_json: String = row.get(3)?;
    Ok(ThreadEvent {
        id: EventId(row.get(0)?),
        thread_id: ThreadId(row.get(1)?),
        timestamp: parse_timestamp(row.get(2)?)?,
        payload: serde_json::from_str(&payload_json).map_err(to_sql_error)?,
        sensitivity: str_to_sensitivity(row.get(4)?)?,
        content_hash: row.get(5)?,
    })
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<Checkpoint> {
    let payload: String = row.get(7)?;
    let draft: Option<String> = row.get(8)?;
    let approved: Option<String> = row.get(9)?;
    let approved_at: Option<String> = row.get(11)?;
    Ok(Checkpoint {
        id: CheckpointId(row.get(0)?),
        thread_id: ThreadId(row.get(1)?),
        start_event_id: EventId(row.get(2)?),
        end_event_id: EventId(row.get(3)?),
        status: row.get(4)?,
        model_id: row.get(5)?,
        prompt_version: row.get(6)?,
        payload: serde_json::from_str(&payload).map_err(to_sql_error)?,
        draft: draft
            .map(|value| serde_json::from_str(&value).map_err(to_sql_error))
            .transpose()?,
        approved: approved
            .map(|value| serde_json::from_str(&value).map_err(to_sql_error))
            .transpose()?,
        created_at: parse_timestamp(row.get(10)?)?,
        approved_at: approved_at.map(parse_timestamp).transpose()?,
    })
}

pub(crate) fn event_type(payload: &EventPayload) -> &'static str {
    match payload {
        EventPayload::ThreadStarted { .. } => "thread.started",
        EventPayload::ThreadPaused { .. } => "thread.paused",
        EventPayload::ThreadResumed { .. } => "thread.resumed",
        EventPayload::GitSnapshot(_) => "git.snapshot",
        EventPayload::FileChanged(_) => "file.changed",
        EventPayload::CommandStarted(_) => "command.started",
        EventPayload::CommandFinished(_) => "command.finished",
        EventPayload::NoteAdded { .. } => "note.added",
        EventPayload::CheckpointRequested(_) => "checkpoint.requested",
        EventPayload::CheckpointGenerated(_) => "checkpoint.generated",
        EventPayload::CheckpointApproved(_) => "checkpoint.approved",
    }
}

fn sensitivity_to_str(sensitivity: &Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Normal => "normal",
        Sensitivity::Sensitive => "sensitive",
        Sensitivity::Blocked => "blocked",
    }
}

fn str_to_sensitivity(value: String) -> rusqlite::Result<Sensitivity> {
    match value.as_str() {
        "normal" => Ok(Sensitivity::Normal),
        "sensitive" => Ok(Sensitivity::Sensitive),
        "blocked" => Ok(Sensitivity::Blocked),
        other => Err(to_sql_error(std::io::Error::other(format!(
            "unknown sensitivity: {other}"
        )))),
    }
}

fn encode_cursor(event: &ThreadEvent) -> String {
    format!("{}|{}", event.timestamp.to_rfc3339(), event.id.0)
}

fn decode_cursor(cursor: &str) -> Result<(&str, &str), DomainError> {
    cursor
        .rsplit_once('|')
        .ok_or_else(|| DomainError::invalid_input("The event cursor is malformed.", "cursor"))
}

fn state_to_str(state: &ThreadState) -> &'static str {
    match state {
        ThreadState::Active => "ACTIVE",
        ThreadState::Paused => "PAUSED",
        ThreadState::CheckpointDraft => "CHECKPOINT_DRAFT",
        ThreadState::Ready => "READY",
        ThreadState::Resumed => "RESUMED",
        ThreadState::Archived => "ARCHIVED",
        ThreadState::Failed => "FAILED",
        ThreadState::Deleted => "DELETED",
    }
}

fn str_to_state(value: String) -> rusqlite::Result<ThreadState> {
    match value.as_str() {
        "ACTIVE" => Ok(ThreadState::Active),
        "PAUSED" => Ok(ThreadState::Paused),
        "CHECKPOINT_DRAFT" => Ok(ThreadState::CheckpointDraft),
        "READY" => Ok(ThreadState::Ready),
        "RESUMED" => Ok(ThreadState::Resumed),
        "ARCHIVED" => Ok(ThreadState::Archived),
        "FAILED" => Ok(ThreadState::Failed),
        "DELETED" => Ok(ThreadState::Deleted),
        other => Err(to_sql_error(std::io::Error::other(format!(
            "unknown thread state: {other}"
        )))),
    }
}

fn parse_timestamp(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(to_sql_error)
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn storage_error(error: rusqlite::Error) -> DomainError {
    DomainError::storage(format!("Local storage operation failed: {error}"))
}

fn json_error(error: serde_json::Error) -> DomainError {
    DomainError::internal(format!("Local JSON serialization failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    fn project() -> Project {
        let now = Utc::now();
        Project {
            id: ProjectId::new(),
            display_name: "Payments".into(),
            canonical_root: "/tmp/payments".into(),
            created_at: now,
            updated_at: now,
        }
    }

    fn thread(project_id: ProjectId) -> WorkThread {
        let now = Utc::now();
        WorkThread {
            id: ThreadId::new(),
            project_id,
            title: "Duplicate capture investigation".into(),
            goal: "Find why retries create a second provider capture".into(),
            state: ThreadState::Active,
            started_at: now,
            ended_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn event(thread_id: ThreadId, timestamp: DateTime<Utc>, text: &str) -> ThreadEvent {
        ThreadEvent {
            id: EventId::new(),
            thread_id,
            timestamp,
            payload: EventPayload::NoteAdded {
                text: text.into(),
                label: crate::NoteLabel::Fact,
            },
            sensitivity: Sensitivity::Normal,
            content_hash: format!("hash-{text}"),
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
    }

    #[test]
    fn project_and_thread_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let project = project();
        db.insert_project(&project).unwrap();
        let stored_project = db.find_project(&project.id).unwrap().unwrap();
        assert_eq!(stored_project.canonical_root, project.canonical_root);

        let thread = thread(project.id);
        db.insert_thread(&thread).unwrap();
        let stored_thread = db.find_thread(&thread.id).unwrap().unwrap();
        assert_eq!(stored_thread.goal, thread.goal);
        assert_eq!(stored_thread.state, ThreadState::Active);
    }

    #[test]
    fn thread_requires_goal_before_storage() {
        let db = Database::open_in_memory().unwrap();
        let project = project();
        db.insert_project(&project).unwrap();
        let now = Utc::now();
        let thread = WorkThread {
            id: ThreadId::new(),
            project_id: project.id,
            title: "Missing goal".into(),
            goal: "  ".into(),
            state: ThreadState::Active,
            started_at: now,
            ended_at: None,
            created_at: now,
            updated_at: now,
        };
        let error = db.insert_thread(&thread).unwrap_err();
        assert_eq!(error.code, crate::ErrorCode::InvalidInput);
    }

    #[test]
    fn events_are_append_only_and_cursor_paginated() {
        let db = Database::open_in_memory().unwrap();
        let project = project();
        db.insert_project(&project).unwrap();
        let thread = thread(project.id);
        db.insert_thread(&thread).unwrap();

        let now = Utc::now();
        for (offset, text) in ["one", "two", "three"].into_iter().enumerate() {
            db.insert_event(&event(
                thread.id.clone(),
                now + Duration::seconds(offset as i64),
                text,
            ))
            .unwrap();
        }

        let first = db.list_events(&thread.id, None, 2).unwrap();
        assert_eq!(first.events.len(), 2);
        assert!(first.next_cursor.is_some());
        let second = db
            .list_events(&thread.id, first.next_cursor.as_deref(), 2)
            .unwrap();
        assert_eq!(second.events.len(), 1);
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn checkpoint_survives_database_reopen() {
        let path = std::env::temp_dir().join(format!(
            "threadback-storage-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        let project_id;
        let thread_id;
        let checkpoint_id;

        {
            let db = Database::open(&path).unwrap();
            let project = project();
            project_id = project.id.clone();
            db.insert_project(&project).unwrap();
            let thread = thread(project.id);
            thread_id = thread.id.clone();
            db.insert_thread(&thread).unwrap();
            let event = event(thread.id.clone(), Utc::now(), "focused test failed");
            db.insert_event(&event).unwrap();
            let checkpoint = Checkpoint {
                id: CheckpointId::new(),
                thread_id: thread.id,
                start_event_id: event.id.clone(),
                end_event_id: event.id,
                status: "draft".into(),
                model_id: None,
                prompt_version: None,
                payload: json!({"events": 1}),
                draft: None,
                approved: None,
                created_at: Utc::now(),
                approved_at: None,
            };
            checkpoint_id = checkpoint.id.clone();
            db.insert_checkpoint(&checkpoint).unwrap();
        }

        {
            let db = Database::open(&path).unwrap();
            assert!(db.find_project(&project_id).unwrap().is_some());
            assert!(db.find_thread(&thread_id).unwrap().is_some());
            assert_eq!(
                db.list_events(&thread_id, None, 10).unwrap().events.len(),
                1
            );
            let checkpoint = db.find_checkpoint(&checkpoint_id).unwrap().unwrap();
            assert_eq!(checkpoint.payload["events"], 1);
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }
}
