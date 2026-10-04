use chrono::Utc;

use crate::{
    AddNoteInput, Database, DomainError, EventPage, EventPayload, ProjectBoundary, ProjectId,
    Sensitivity, StartThreadInput, ThreadEvent, ThreadId, ThreadState, WorkThread,
    capture_snapshot,
    threads::lifecycle::{LifecycleAction, next_state},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    User,
    System,
    /// The application stopped without pausing the thread. Set only by
    /// startup recovery.
    Interrupted,
}

impl PauseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Applies lifecycle rules to stored threads. Each operation runs in a single
/// transaction, so a state change and its event are committed together or not
/// at all.
pub struct ThreadService<'a> {
    database: &'a Database,
}

impl<'a> ThreadService<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub fn start(&self, input: StartThreadInput) -> Result<WorkThread, DomainError> {
        input.validate()?;
        self.start_with(input, None)
    }

    /// Starts a thread and records its first Git snapshot in the same
    /// transaction. The snapshot is taken first, so a project that is not a
    /// readable Git repository fails without leaving a thread behind.
    pub fn start_with_git_snapshot(
        &self,
        input: StartThreadInput,
    ) -> Result<WorkThread, DomainError> {
        input.validate()?;
        let project = self
            .database
            .find_project(&input.project_id)?
            .ok_or_else(|| DomainError::not_found("The selected project is not approved."))?;
        let payload = snapshot_payload(&project.canonical_root)?;
        self.start_with(input, Some(payload))
    }

    fn start_with(
        &self,
        input: StartThreadInput,
        snapshot: Option<serde_json::Value>,
    ) -> Result<WorkThread, DomainError> {
        self.database.with_transaction(|db| {
            require_project(db, &input.project_id)?;
            ensure_no_other_live_thread(db, None)?;

            let redactor = crate::privacy::Redactor::builtin();
            let now = Utc::now();
            let thread = WorkThread {
                id: ThreadId::new(),
                project_id: input.project_id.clone(),
                title: redactor.redact(input.title.trim()).text,
                goal: redactor.redact(input.goal.trim()).text,
                state: ThreadState::Active,
                started_at: now,
                ended_at: None,
                created_at: now,
                updated_at: now,
            };
            db.insert_thread(&thread)?;
            db.insert_event(&ThreadEvent::new(
                thread.id.clone(),
                EventPayload::ThreadStarted {
                    goal: thread.goal.clone(),
                    hypothesis: input
                        .hypothesis
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned),
                },
                Sensitivity::Normal,
            )?)?;
            if let Some(payload) = snapshot {
                db.insert_event(&ThreadEvent::new(
                    thread.id.clone(),
                    EventPayload::GitSnapshot(payload),
                    Sensitivity::Normal,
                )?)?;
            }
            Ok(thread)
        })
    }

    pub fn pause(&self, id: &ThreadId, reason: PauseReason) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::Pause, |_| {
            Ok(Some(EventPayload::ThreadPaused {
                reason: reason.as_str().into(),
            }))
        })
    }

    /// `PAUSED` returns to `ACTIVE`; `READY` begins a new evidence interval.
    pub fn resume(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::Resume, |db| {
            Ok(Some(EventPayload::ThreadResumed {
                prior_checkpoint_id: db.latest_checkpoint_id(id)?,
            }))
        })
    }

    pub fn end(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::End, |_| Ok(None))
    }

    pub fn begin_checkpoint(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::BeginCheckpoint, |_| Ok(None))
    }

    pub fn mark_ready(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::MarkReady, |_| Ok(None))
    }

    pub fn mark_failed(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::MarkFailed, |_| Ok(None))
    }

    pub fn retry_checkpoint(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::Retry, |_| Ok(None))
    }

    pub fn archive(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        self.transition(id, LifecycleAction::Archive, |_| Ok(None))
    }

    /// Appends an event to a recording thread. The event is committed before
    /// this returns, so a returned event is an acknowledged one.
    pub fn record(
        &self,
        id: &ThreadId,
        payload: EventPayload,
        sensitivity: Sensitivity,
    ) -> Result<ThreadEvent, DomainError> {
        self.database.with_transaction(|db| {
            let thread = require_thread(db, id)?;
            require_recording_state(&thread)?;
            let event = ThreadEvent::new(thread.id, payload, sensitivity)?;
            db.insert_event(&event)?;
            Ok(event)
        })
    }

    /// Captures a read-only Git snapshot of the thread's project and records
    /// it. Git runs outside the transaction so the database is not locked
    /// while a subprocess executes.
    pub fn capture_git(&self, id: &ThreadId) -> Result<ThreadEvent, DomainError> {
        let thread = self.require_recording(id)?;
        let project = self
            .database
            .find_project(&thread.project_id)?
            .ok_or_else(|| DomainError::not_found("The thread's project is no longer approved."))?;
        let payload = snapshot_payload(&project.canonical_root)?;
        self.record(id, EventPayload::GitSnapshot(payload), Sensitivity::Normal)
    }

    /// Records a user note. The label and text are stored exactly as given.
    pub fn add_note(&self, id: &ThreadId, note: AddNoteInput) -> Result<ThreadEvent, DomainError> {
        note.validate()?;
        self.record(
            id,
            EventPayload::NoteAdded {
                text: note.text,
                label: note.label,
            },
            Sensitivity::Normal,
        )
    }

    /// One page of the thread's evidence, oldest first.
    pub fn timeline(
        &self,
        id: &ThreadId,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EventPage, DomainError> {
        require_thread(self.database, id)?;
        self.database.list_events(id, cursor, limit)
    }

    pub fn list_threads(&self) -> Result<Vec<WorkThread>, DomainError> {
        self.database.list_threads()
    }

    /// Errors unless the thread exists and is recording evidence. Collectors
    /// call this before creating any event.
    pub fn require_recording(&self, id: &ThreadId) -> Result<WorkThread, DomainError> {
        let thread = require_thread(self.database, id)?;
        require_recording_state(&thread)?;
        Ok(thread)
    }

    fn transition(
        &self,
        id: &ThreadId,
        action: LifecycleAction,
        event: impl FnOnce(&Database) -> Result<Option<EventPayload>, DomainError>,
    ) -> Result<WorkThread, DomainError> {
        self.database
            .with_transaction(|db| apply_transition(db, id, action, event))
    }
}

/// Moves a thread to its next state and records the optional event. Runs
/// inside the caller's transaction so a larger operation can include it.
pub(crate) fn apply_transition(
    db: &Database,
    id: &ThreadId,
    action: LifecycleAction,
    event: impl FnOnce(&Database) -> Result<Option<EventPayload>, DomainError>,
) -> Result<WorkThread, DomainError> {
    let mut thread = require_thread(db, id)?;
    let next = next_state(&thread.state, action)?;
    if next.accepts_evidence() {
        ensure_no_other_live_thread(db, Some(&thread.id))?;
    }

    let now = Utc::now();
    let ended_at = match (action, &next) {
        (LifecycleAction::Resume, ThreadState::Resumed) => None,
        (LifecycleAction::End | LifecycleAction::Archive, _) => thread.ended_at.or(Some(now)),
        _ => thread.ended_at,
    };
    db.update_thread_state(&thread.id, &thread.state, &next, ended_at, now)?;
    if let Some(payload) = event(db)? {
        db.insert_event(&ThreadEvent::new(
            thread.id.clone(),
            payload,
            Sensitivity::Normal,
        )?)?;
    }

    thread.state = next;
    thread.ended_at = ended_at;
    thread.updated_at = now;
    Ok(thread)
}

fn snapshot_payload(canonical_root: &str) -> Result<serde_json::Value, DomainError> {
    let boundary = ProjectBoundary::new(canonical_root, &[])?;
    let snapshot = capture_snapshot(&boundary)?;
    serde_json::to_value(&snapshot)
        .map_err(|error| DomainError::internal(format!("Snapshot serialization failed: {error}")))
}

fn require_recording_state(thread: &WorkThread) -> Result<(), DomainError> {
    if thread.state.accepts_evidence() {
        Ok(())
    } else {
        Err(DomainError::invalid_state(
            "This thread is not active, so it is not recording evidence.",
        ))
    }
}

fn require_project(db: &Database, id: &ProjectId) -> Result<(), DomainError> {
    db.find_project(id)?
        .map(|_| ())
        .ok_or_else(|| DomainError::not_found("The selected project is not approved."))
}

pub(crate) fn require_thread(db: &Database, id: &ThreadId) -> Result<WorkThread, DomainError> {
    db.find_thread(id)?
        .ok_or_else(|| DomainError::not_found("That thread no longer exists."))
}

fn ensure_no_other_live_thread(db: &Database, this: Option<&ThreadId>) -> Result<(), DomainError> {
    match db.find_live_thread()? {
        Some(live) if Some(&live.id) != this => Err(DomainError::invalid_state(
            "Another thread is already recording. Pause or end it first.",
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckpointId, ErrorCode, EventId, Project};
    use chrono::Utc;

    fn setup() -> (Database, ProjectId) {
        let db = Database::open_in_memory().unwrap();
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            display_name: "Payments".into(),
            canonical_root: "/tmp/payments".into(),
            created_at: now,
            updated_at: now,
        };
        db.insert_project(&project).unwrap();
        (db, project.id)
    }

    fn input(project_id: &ProjectId) -> StartThreadInput {
        StartThreadInput {
            project_id: project_id.clone(),
            title: " Duplicate capture investigation ".into(),
            goal: " Find why retries create a second provider capture ".into(),
            hypothesis: Some("A second worker bypasses the lock".into()),
        }
    }

    fn event_types(db: &Database, id: &ThreadId) -> Vec<&'static str> {
        db.list_events(id, None, 100)
            .unwrap()
            .events
            .iter()
            .map(|event| match event.payload {
                EventPayload::ThreadStarted { .. } => "started",
                EventPayload::ThreadPaused { .. } => "paused",
                EventPayload::ThreadResumed { .. } => "resumed",
                _ => "other",
            })
            .collect()
    }

    fn insert_checkpoint(db: &Database, thread_id: &ThreadId) -> CheckpointId {
        let event_id: EventId = db.list_events(thread_id, None, 1).unwrap().events[0]
            .id
            .clone();
        let checkpoint = crate::Checkpoint {
            id: CheckpointId::new(),
            thread_id: thread_id.clone(),
            start_event_id: event_id.clone(),
            end_event_id: event_id,
            status: "draft".into(),
            model_id: None,
            prompt_version: None,
            payload: serde_json::json!({}),
            draft: None,
            approved: None,
            created_at: Utc::now(),
            approved_at: None,
        };
        db.insert_checkpoint(&checkpoint).unwrap();
        checkpoint.id
    }

    #[test]
    fn start_creates_active_thread_with_started_event() {
        let (db, project_id) = setup();
        let thread = ThreadService::new(&db).start(input(&project_id)).unwrap();
        assert_eq!(thread.state, ThreadState::Active);
        assert_eq!(thread.title, "Duplicate capture investigation");
        assert_eq!(
            thread.goal,
            "Find why retries create a second provider capture"
        );

        let stored = db.find_thread(&thread.id).unwrap().unwrap();
        assert_eq!(stored.state, ThreadState::Active);
        assert_eq!(event_types(&db, &thread.id), ["started"]);
    }

    #[test]
    fn start_records_goal_and_hypothesis_in_the_event() {
        let (db, project_id) = setup();
        let thread = ThreadService::new(&db).start(input(&project_id)).unwrap();
        let events = db.list_events(&thread.id, None, 10).unwrap().events;
        match &events[0].payload {
            EventPayload::ThreadStarted { goal, hypothesis } => {
                assert_eq!(goal, "Find why retries create a second provider capture");
                assert_eq!(
                    hypothesis.as_deref(),
                    Some("A second worker bypasses the lock")
                );
            }
            other => panic!("unexpected payload {other:?}"),
        }
        assert_eq!(events[0].content_hash.len(), 64);
    }

    #[test]
    fn start_requires_title_and_goal() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        for (title, goal, field) in [("  ", "goal", "title"), ("title", "  ", "goal")] {
            let mut bad = input(&project_id);
            bad.title = title.into();
            bad.goal = goal.into();
            let error = service.start(bad).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput);
            assert_eq!(error.field.as_deref(), Some(field));
        }
    }

    #[test]
    fn start_requires_an_approved_project() {
        let (db, _) = setup();
        let error = ThreadService::new(&db)
            .start(input(&ProjectId::new()))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn blank_hypothesis_is_not_recorded() {
        let (db, project_id) = setup();
        let mut blank = input(&project_id);
        blank.hypothesis = Some("   ".into());
        let thread = ThreadService::new(&db).start(blank).unwrap();
        let events = db.list_events(&thread.id, None, 10).unwrap().events;
        assert!(matches!(
            &events[0].payload,
            EventPayload::ThreadStarted {
                hypothesis: None,
                ..
            }
        ));
    }

    #[test]
    fn only_one_thread_can_record_at_a_time() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let first = service.start(input(&project_id)).unwrap();
        let error = service.start(input(&project_id)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);

        service.pause(&first.id, PauseReason::User).unwrap();
        let second = service.start(input(&project_id)).unwrap();

        let error = service.resume(&first.id).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(
            db.find_thread(&first.id).unwrap().unwrap().state,
            ThreadState::Paused
        );
        assert_eq!(
            db.find_thread(&second.id).unwrap().unwrap().state,
            ThreadState::Active
        );
    }

    #[test]
    fn pause_and_resume_round_trip_records_events() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();

        let paused = service.pause(&thread.id, PauseReason::System).unwrap();
        assert_eq!(paused.state, ThreadState::Paused);
        assert!(!paused.state.accepts_evidence());
        assert!(service.require_recording(&thread.id).is_err());

        let resumed = service.resume(&thread.id).unwrap();
        assert_eq!(resumed.state, ThreadState::Active);
        assert!(service.require_recording(&thread.id).is_ok());
        assert_eq!(
            event_types(&db, &thread.id),
            ["started", "paused", "resumed"]
        );

        let events = db.list_events(&thread.id, None, 10).unwrap().events;
        assert!(matches!(
            &events[1].payload,
            EventPayload::ThreadPaused { reason } if reason == "system"
        ));
    }

    #[test]
    fn end_sets_ended_at_and_moves_to_checkpoint_draft() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        let ended = service.end(&thread.id).unwrap();
        assert_eq!(ended.state, ThreadState::CheckpointDraft);
        assert!(ended.ended_at.is_some());
        let stored = db.find_thread(&thread.id).unwrap().unwrap();
        assert_eq!(stored.ended_at, ended.ended_at);
        assert!(service.require_recording(&thread.id).is_err());
    }

    #[test]
    fn save_my_place_keeps_thread_open_but_stops_recording() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        let drafting = service.begin_checkpoint(&thread.id).unwrap();
        assert_eq!(drafting.state, ThreadState::CheckpointDraft);
        assert!(drafting.ended_at.is_none());
    }

    #[test]
    fn full_checkpoint_cycle_resumes_with_prior_checkpoint_and_clears_end() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        service.end(&thread.id).unwrap();
        let checkpoint_id = insert_checkpoint(&db, &thread.id);
        service.mark_ready(&thread.id).unwrap();

        let resumed = service.resume(&thread.id).unwrap();
        assert_eq!(resumed.state, ThreadState::Resumed);
        assert!(resumed.ended_at.is_none());
        assert!(service.require_recording(&thread.id).is_ok());

        let events = db.list_events(&thread.id, None, 10).unwrap().events;
        match &events.last().unwrap().payload {
            EventPayload::ThreadResumed {
                prior_checkpoint_id,
            } => {
                assert_eq!(prior_checkpoint_id.as_ref(), Some(&checkpoint_id));
            }
            other => panic!("unexpected payload {other:?}"),
        }
    }

    #[test]
    fn failed_checkpoint_can_retry_then_become_ready() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        service.begin_checkpoint(&thread.id).unwrap();
        assert_eq!(
            service.mark_failed(&thread.id).unwrap().state,
            ThreadState::Failed
        );
        assert_eq!(
            service.retry_checkpoint(&thread.id).unwrap().state,
            ThreadState::CheckpointDraft
        );
        assert_eq!(
            service.mark_ready(&thread.id).unwrap().state,
            ThreadState::Ready
        );
    }

    #[test]
    fn archive_is_terminal_except_for_deletion() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        service.pause(&thread.id, PauseReason::User).unwrap();
        let archived = service.archive(&thread.id).unwrap();
        assert_eq!(archived.state, ThreadState::Archived);
        assert!(archived.ended_at.is_some());
        assert!(service.resume(&thread.id).is_err());
        assert!(service.pause(&thread.id, PauseReason::User).is_err());
    }

    #[test]
    fn rejected_transition_changes_nothing() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        let before = db.find_thread(&thread.id).unwrap().unwrap();

        for result in [
            service.resume(&thread.id),
            service.mark_ready(&thread.id),
            service.retry_checkpoint(&thread.id),
            service.archive(&thread.id),
        ] {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidState);
        }
        let after = db.find_thread(&thread.id).unwrap().unwrap();
        assert_eq!(after.state, before.state);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(event_types(&db, &thread.id), ["started"]);
    }

    #[test]
    fn unknown_thread_is_not_found() {
        let (db, _) = setup();
        let error = ThreadService::new(&db).end(&ThreadId::new()).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn failed_event_insert_rolls_back_the_state_change() {
        let (db, project_id) = setup();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project_id)).unwrap();
        let result = db.with_transaction(|db| {
            db.update_thread_state(
                &thread.id,
                &ThreadState::Active,
                &ThreadState::Paused,
                None,
                Utc::now(),
            )?;
            Err::<(), _>(DomainError::internal("simulated failure after the update"))
        });
        assert!(result.is_err());
        assert_eq!(
            db.find_thread(&thread.id).unwrap().unwrap().state,
            ThreadState::Active
        );
    }

    #[test]
    fn stale_state_update_is_rejected() {
        let (db, project_id) = setup();
        let thread = ThreadService::new(&db).start(input(&project_id)).unwrap();
        let error = db
            .update_thread_state(
                &thread.id,
                &ThreadState::Paused,
                &ThreadState::Active,
                None,
                Utc::now(),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
    }

    #[test]
    fn capture_git_records_a_snapshot_event_for_the_project_repository() {
        use std::process::Command;
        let dir = tempfile::TempDir::new().unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(dir.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();

        let db = Database::open_in_memory().unwrap();
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            display_name: "Repo".into(),
            canonical_root: std::fs::canonicalize(dir.path())
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            created_at: now,
            updated_at: now,
        };
        db.insert_project(&project).unwrap();
        let service = ThreadService::new(&db);
        let thread = service.start(input(&project.id)).unwrap();

        let event = service.capture_git(&thread.id).unwrap();
        let EventPayload::GitSnapshot(value) = &event.payload else {
            panic!("expected a git snapshot, got {:?}", event.payload);
        };
        assert_eq!(value["branch"], "main");
        assert_eq!(value["dirty"], true);
        assert_eq!(value["changedPaths"][0]["path"], "lib.rs");
        assert_eq!(event_types(&db, &thread.id), ["started", "other"]);

        service.pause(&thread.id, PauseReason::User).unwrap();
        let error = service.capture_git(&thread.id).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
    }
}
