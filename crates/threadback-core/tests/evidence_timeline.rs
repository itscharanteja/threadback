use std::{fs, path::Path, process::Command};

use chrono::{Duration, Utc};
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, Database, ErrorCode, EventId, EventPayload, NoteLabel, PauseReason, Project,
    ProjectId, Sensitivity, StartThreadInput, ThreadEvent, ThreadId, ThreadService,
};

fn approve_project(db: &Database, root: &Path) -> ProjectId {
    let now = Utc::now();
    let project = Project {
        id: ProjectId::new(),
        display_name: "Payments".into(),
        canonical_root: fs::canonicalize(root)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        created_at: now,
        updated_at: now,
    };
    db.insert_project(&project).unwrap();
    project.id
}

fn input(project_id: &ProjectId) -> StartThreadInput {
    StartThreadInput {
        project_id: project_id.clone(),
        title: "Duplicate capture investigation".into(),
        goal: "Find why retries create a second provider capture".into(),
        hypothesis: None,
    }
}

fn note(text: &str, label: NoteLabel) -> AddNoteInput {
    AddNoteInput {
        text: text.into(),
        label,
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn repository() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn all_events(db: &Database, id: &ThreadId) -> Vec<ThreadEvent> {
    let service = ThreadService::new(db);
    let mut events = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = service.timeline(id, cursor.as_deref(), 200).unwrap();
        events.extend(page.events);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return events,
        }
    }
}

fn note_text(event: &ThreadEvent) -> Option<(&str, &NoteLabel)> {
    match &event.payload {
        EventPayload::NoteAdded { text, label } => Some((text, label)),
        _ => None,
    }
}

#[test]
fn note_labels_and_text_are_preserved_exactly_across_restart() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("threadback.db");
    let texts = [
        (
            "  The focused retry test made two provider calls.\n",
            NoteLabel::Fact,
        ),
        (
            "A second worker bypasses the lock — 日本語 ✓",
            NoteLabel::Hypothesis,
        ),
        (
            "Do both workers share the same lock backend?",
            NoteLabel::Question,
        ),
        ("Log the lock owner in both workers.", NoteLabel::NextStep),
        // A note that sounds like a fact is still a hypothesis if the user says so.
        ("It is definitely the middleware", NoteLabel::Hypothesis),
    ];

    let thread_id;
    {
        let db = Database::open(&path).unwrap();
        let project_id = approve_project(&db, dir.path());
        let service = ThreadService::new(&db);
        thread_id = service.start(input(&project_id)).unwrap().id;
        for (text, label) in &texts {
            service
                .add_note(&thread_id, note(text, label.clone()))
                .unwrap();
        }
    }

    let db = Database::open(&path).unwrap();
    let events = all_events(&db, &thread_id);
    let stored: Vec<_> = events.iter().filter_map(note_text).collect();
    assert_eq!(stored.len(), texts.len());
    for ((text, label), (stored_text, stored_label)) in texts.iter().zip(stored) {
        assert_eq!(*text, stored_text);
        assert_eq!(label, stored_label);
    }
}

#[test]
fn note_event_wire_shape_carries_the_label() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let thread = service.start(input(&project_id)).unwrap();
    let event = service
        .add_note(&thread.id, note("check the lock", NoteLabel::NextStep))
        .unwrap();
    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["payload"]["type"], "note_added");
    assert_eq!(value["payload"]["payload"]["label"], "next_step");
    assert_eq!(value["payload"]["payload"]["text"], "check the lock");
    assert_eq!(value["sensitivity"], "normal");
    assert_eq!(value["contentHash"].as_str().unwrap().len(), 64);
}

#[test]
fn blank_and_oversized_notes_are_rejected_and_not_stored() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let thread = service.start(input(&project_id)).unwrap();

    for text in ["", "   \n\t", &"x".repeat(8_001)] {
        let error = service
            .add_note(&thread.id, note(text, NoteLabel::Fact))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(error.field.as_deref(), Some("text"));
    }
    assert!(
        service
            .add_note(&thread.id, note(&"x".repeat(8_000), NoteLabel::Fact))
            .is_ok()
    );
    // thread.started plus the one accepted note.
    assert_eq!(all_events(&db, &thread.id).len(), 2);
}

#[test]
fn notes_are_refused_unless_the_thread_is_recording() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let thread = service.start(input(&project_id)).unwrap();

    service.pause(&thread.id, PauseReason::User).unwrap();
    let error = service
        .add_note(&thread.id, note("while paused", NoteLabel::Fact))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);

    service.resume(&thread.id).unwrap();
    service
        .add_note(&thread.id, note("after resume", NoteLabel::Fact))
        .unwrap();
    service.end(&thread.id).unwrap();
    assert!(
        service
            .add_note(&thread.id, note("after end", NoteLabel::Fact))
            .is_err()
    );

    let texts: Vec<_> = all_events(&db, &thread.id)
        .iter()
        .filter_map(|event| note_text(event).map(|(text, _)| text.to_owned()))
        .collect();
    assert_eq!(texts, ["after resume"]);
}

#[test]
fn timeline_is_chronological_across_mixed_event_types() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, repo.path());
    let service = ThreadService::new(&db);
    let thread = service.start_with_git_snapshot(input(&project_id)).unwrap();

    service
        .add_note(&thread.id, note("first", NoteLabel::Fact))
        .unwrap();
    fs::write(repo.path().join("lib.rs"), "fn main() { retry(); }\n").unwrap();
    service.capture_git(&thread.id).unwrap();
    service
        .add_note(&thread.id, note("second", NoteLabel::Hypothesis))
        .unwrap();
    service.pause(&thread.id, PauseReason::User).unwrap();

    let events = all_events(&db, &thread.id);
    let kinds: Vec<_> = events
        .iter()
        .map(|event| match &event.payload {
            EventPayload::ThreadStarted { .. } => "started",
            EventPayload::GitSnapshot(_) => "git",
            EventPayload::NoteAdded { .. } => "note",
            EventPayload::ThreadPaused { .. } => "paused",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["started", "git", "note", "git", "note", "paused"]);
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].timestamp <= pair[1].timestamp)
    );
}

#[test]
fn timeline_pages_cover_every_event_once() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let thread = service.start(input(&project_id)).unwrap();
    for index in 0..449 {
        service
            .add_note(&thread.id, note(&format!("note {index}"), NoteLabel::Fact))
            .unwrap();
    }

    let first = service.timeline(&thread.id, None, 10_000).unwrap();
    assert_eq!(first.events.len(), 200, "page size is clamped");
    assert!(first.next_cursor.is_some());

    let events = all_events(&db, &thread.id);
    assert_eq!(events.len(), 450);
    let mut ids: Vec<_> = events.iter().map(|event| event.id.0.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 450);
    let notes: Vec<_> = events
        .iter()
        .filter_map(|event| note_text(event).map(|(text, _)| text.to_owned()))
        .collect();
    let expected: Vec<_> = (0..449).map(|index| format!("note {index}")).collect();
    assert_eq!(notes, expected);
}

#[test]
fn pagination_does_not_skip_events_that_share_a_timestamp() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let thread = ThreadService::new(&db).start(input(&project_id)).unwrap();
    let shared = Utc::now() + Duration::seconds(5);
    for index in 0..7 {
        let mut event = ThreadEvent::new(
            thread.id.clone(),
            EventPayload::NoteAdded {
                text: format!("tied {index}"),
                label: NoteLabel::Fact,
            },
            Sensitivity::Normal,
        )
        .unwrap();
        event.timestamp = shared;
        event.id = EventId(format!("evt-{index}"));
        db.insert_event(&event).unwrap();
    }

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = db.list_events(&thread.id, cursor.as_deref(), 2).unwrap();
        seen.extend(page.events.into_iter().map(|event| event.id.0));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    let tied: Vec<_> = seen.iter().filter(|id| id.starts_with("evt-")).collect();
    assert_eq!(tied.len(), 7);
    assert_eq!(seen.len(), 8);
}

#[test]
fn malformed_cursor_and_unknown_thread_are_reported() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let thread = service.start(input(&project_id)).unwrap();

    let error = service
        .timeline(&thread.id, Some("not-a-cursor"), 10)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let error = service.timeline(&ThreadId::new(), None, 10).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn start_with_snapshot_records_the_first_git_state_atomically() {
    let repo = repository();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("threadback.db");
    let thread_id;
    {
        let db = Database::open(&path).unwrap();
        let project_id = approve_project(&db, repo.path());
        let mut start = input(&project_id);
        start.hypothesis = Some("a second worker bypasses the lock".into());
        thread_id = ThreadService::new(&db)
            .start_with_git_snapshot(start)
            .unwrap()
            .id;
    }

    let db = Database::open(&path).unwrap();
    let events = all_events(&db, &thread_id);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].payload,
        EventPayload::ThreadStarted { .. }
    ));
    let EventPayload::GitSnapshot(snapshot) = &events[1].payload else {
        panic!("second event should be the first snapshot");
    };
    assert_eq!(snapshot["branch"], "main");
    assert_eq!(snapshot["dirty"], false);
}

#[test]
fn start_with_snapshot_leaves_no_thread_when_git_is_unavailable() {
    let plain_folder = TempDir::new().unwrap();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, plain_folder.path());
    let service = ThreadService::new(&db);

    let error = service
        .start_with_git_snapshot(input(&project_id))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert!(service.list_threads().unwrap().is_empty());
}

#[test]
fn start_with_snapshot_still_requires_a_goal() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, repo.path());
    let mut start = input(&project_id);
    start.goal = "  ".into();
    let error = ThreadService::new(&db)
        .start_with_git_snapshot(start)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(error.field.as_deref(), Some("goal"));
    assert!(ThreadService::new(&db).list_threads().unwrap().is_empty());
}

#[test]
fn threads_list_most_recently_updated_first() {
    let db = Database::open_in_memory().unwrap();
    let project_id = approve_project(&db, TempDir::new().unwrap().path());
    let service = ThreadService::new(&db);
    let older = service.start(input(&project_id)).unwrap();
    service.pause(&older.id, PauseReason::User).unwrap();
    let newer = service.start(input(&project_id)).unwrap();

    let ids: Vec<_> = service
        .list_threads()
        .unwrap()
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    assert_eq!(ids, [newer.id, older.id]);
}
