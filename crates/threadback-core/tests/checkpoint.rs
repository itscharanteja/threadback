use std::{fs, path::Path, process::Command, time::Instant};

use chrono::Utc;
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, CaptureLimits, CheckpointPayload, CheckpointRequestedPayload, CheckpointService,
    CheckpointTrigger, CommandRequest, Database, ErrorCode, EventPayload, NoteLabel, PauseReason,
    Project, ProjectId, RunIo, STATUS_DRAFT, StartThreadInput, ThreadId, ThreadService,
    ThreadState, run_command,
};

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
    fs::write(dir.path().join("retry.ts"), "export const retry = 1;\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn approve(db: &Database, root: &Path) -> ProjectId {
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

fn start(db: &Database, project_id: &ProjectId) -> ThreadId {
    ThreadService::new(db)
        .start_with_git_snapshot(StartThreadInput {
            project_id: project_id.clone(),
            title: "Duplicate capture investigation".into(),
            goal: "Find why retries create a second provider capture".into(),
            hypothesis: None,
        })
        .unwrap()
        .id
}

fn note(db: &Database, id: &ThreadId, text: &str, label: NoteLabel) {
    ThreadService::new(db)
        .add_note(
            id,
            AddNoteInput {
                text: text.into(),
                label,
            },
        )
        .unwrap();
}

fn run(db: &Database, cwd: &Path, program: &str, args: &[&str]) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut notify = |message: &str| panic!("unexpected notice: {message}");
    run_command(
        Some(db),
        &CommandRequest {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
            limits: CaptureLimits::default(),
        },
        RunIo {
            stdout: &mut out,
            stderr: &mut err,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap();
}

fn payload(checkpoint: &threadback_core::Checkpoint) -> CheckpointPayload {
    serde_json::from_value(checkpoint.payload.clone()).unwrap()
}

#[test]
fn save_my_place_creates_a_draft_and_moves_the_thread_to_checkpoint_draft() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve(&db, repo.path());
    let id = start(&db, &project_id);

    note(
        &db,
        &id,
        "The focused retry test made two provider calls.",
        NoteLabel::Fact,
    );
    run(
        &db,
        repo.path(),
        "sh",
        &["-c", "echo '2 provider calls'; exit 1"],
    );
    fs::write(repo.path().join("retry.ts"), "export const retry = 2;\n").unwrap();
    note(
        &db,
        &id,
        "A second worker bypasses the lock",
        NoteLabel::Hypothesis,
    );

    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::Manual)
        .unwrap();

    assert_eq!(checkpoint.status, STATUS_DRAFT);
    assert!(checkpoint.draft.is_none() && checkpoint.approved.is_none());
    assert!(checkpoint.model_id.is_none());
    let thread = db.find_thread(&id).unwrap().unwrap();
    assert_eq!(thread.state, ThreadState::CheckpointDraft);
    assert!(thread.ended_at.is_none());

    let payload = payload(&checkpoint);
    assert_eq!(
        payload.goal,
        "Find why retries create a second provider capture"
    );
    assert!(payload.final_snapshot.captured);
    let kinds: Vec<_> = payload.entries.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "thread.started",
            "git.snapshot", // the initial snapshot, still different from the final one
            "note.added",
            "command.started",
            "command.finished",
            "note.added",
            "git.snapshot", // the final snapshot, taken by Save My Place
        ]
    );

    // The final snapshot is the end of the range and reflects the edit.
    let last = payload.entries.last().unwrap();
    assert_eq!(
        Some(&last.event_id),
        payload.final_snapshot.event_id.as_ref()
    );
    assert_eq!(last.event_id, checkpoint.end_event_id);
    assert_eq!(last.content["dirty"], true);
    assert_eq!(last.content["changedPaths"][0]["path"], "retry.ts");
    assert_eq!(payload.entries[0].event_id, checkpoint.start_event_id);

    // The failing command's output is part of the evidence.
    let finished = payload
        .entries
        .iter()
        .find(|e| e.kind == "command.finished")
        .unwrap();
    assert_eq!(finished.content["exitCode"], 1);
    assert_eq!(finished.content["stdout"]["text"], "2 provider calls\n");
}

#[test]
fn the_request_event_records_the_trigger_and_range() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::IdleSuggestion)
        .unwrap();

    let events = db.list_all_events(&id).unwrap();
    let EventPayload::CheckpointRequested(value) = &events.last().unwrap().payload else {
        panic!("the last event should be the request");
    };
    let requested: CheckpointRequestedPayload = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(requested.checkpoint_id, checkpoint.id);
    assert_eq!(requested.trigger, CheckpointTrigger::IdleSuggestion);
    assert_eq!(requested.start_event_id, checkpoint.start_event_id);
    assert_eq!(requested.end_event_id, checkpoint.end_event_id);
    // The request comes after the range it describes.
    assert!(
        events
            .iter()
            .position(|e| e.id == requested.end_event_id)
            .unwrap()
            < events.len() - 1
    );
}

#[test]
fn the_draft_survives_a_restart_and_stays_inspectable_without_a_model() {
    let repo = repository();
    let data = TempDir::new().unwrap();
    let path = data.path().join("threadback.db");
    let (id, checkpoint_id);
    {
        let db = Database::open(&path).unwrap();
        id = start(&db, &approve(&db, repo.path()));
        note(&db, &id, "check the lock owner", NoteLabel::NextStep);
        checkpoint_id = CheckpointService::new(&db)
            .save(&id, CheckpointTrigger::Manual)
            .unwrap()
            .id;
    }

    let db = Database::open(&path).unwrap();
    // Startup recovery leaves a checkpoint draft alone.
    assert!(
        threadback_core::recover_interrupted_threads(&db)
            .unwrap()
            .is_empty()
    );
    let service = CheckpointService::new(&db);
    let checkpoint = service.get(&checkpoint_id).unwrap();
    assert_eq!(checkpoint.status, STATUS_DRAFT);
    let payload = payload(&checkpoint);
    assert!(
        payload
            .entries
            .iter()
            .any(|e| e.content["text"] == "check the lock owner")
    );
    assert_eq!(service.list(&id).unwrap().len(), 1);
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );
}

#[test]
fn it_works_offline_and_within_two_seconds_on_a_demo_sized_thread() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    for index in 0..60 {
        note(&db, &id, &format!("observation {index}"), NoteLabel::Fact);
    }
    for index in 0..25 {
        run(
            &db,
            repo.path(),
            "sh",
            &[
                "-c",
                &format!("head -c 300000 /dev/zero | tr '\\0' x; echo run {index}; exit 1"),
            ],
        );
    }
    ThreadService::new(&db).capture_git(&id).unwrap();

    let started = Instant::now();
    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::Manual)
        .unwrap();
    let elapsed = started.elapsed();
    eprintln!("Save My Place took {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");

    let payload = payload(&checkpoint);
    assert!(payload.entries.len() <= 150);
    assert!(payload.total_chars <= 48_000, "{}", payload.total_chars);
    assert!(
        payload.omissions.over_budget_removed > 0,
        "the demo thread exceeds the budget"
    );
    // Every note survives the budget.
    assert_eq!(
        payload
            .entries
            .iter()
            .filter(|e| e.kind == "note.added")
            .count(),
        60
    );
}

#[test]
fn a_failed_final_snapshot_still_saves_the_checkpoint_and_says_why() {
    let plain_folder = TempDir::new().unwrap();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve(&db, plain_folder.path());
    let id = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id,
            title: "t".into(),
            goal: "g".into(),
            hypothesis: None,
        })
        .unwrap()
        .id;
    note(&db, &id, "work done outside Git", NoteLabel::Fact);

    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::Manual)
        .unwrap();
    let payload = payload(&checkpoint);
    assert!(!payload.final_snapshot.captured);
    assert!(
        payload
            .final_snapshot
            .reason
            .as_deref()
            .unwrap()
            .contains("Git repository")
    );
    assert!(payload.entries.iter().all(|e| e.kind != "git.snapshot"));
    assert!(payload.entries.iter().any(|e| e.kind == "note.added"));
}

#[test]
fn it_is_refused_when_the_thread_is_not_recording_and_changes_nothing() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    ThreadService::new(&db)
        .pause(&id, PauseReason::User)
        .unwrap();
    let events_before = db.list_all_events(&id).unwrap().len();

    let error = service.save(&id, CheckpointTrigger::Manual).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert!(service.list(&id).unwrap().is_empty());
    assert_eq!(db.list_all_events(&id).unwrap().len(), events_before);
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::Paused
    );

    let error = service
        .save(&ThreadId::new(), CheckpointTrigger::Manual)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn saving_twice_creates_one_checkpoint() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    service.save(&id, CheckpointTrigger::Manual).unwrap();
    let error = service.save(&id, CheckpointTrigger::Manual).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert_eq!(service.list(&id).unwrap().len(), 1);
}

#[test]
fn ending_a_thread_saves_a_checkpoint_and_closes_it() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    note(&db, &id, "wrapping up", NoteLabel::Fact);

    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::ThreadEnded)
        .unwrap();
    let thread = db.find_thread(&id).unwrap().unwrap();
    assert_eq!(thread.state, ThreadState::CheckpointDraft);
    assert!(thread.ended_at.is_some());
    assert_eq!(payload(&checkpoint).trigger, CheckpointTrigger::ThreadEnded);
}

#[test]
fn a_paused_thread_can_be_ended_but_takes_no_new_snapshot() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    note(&db, &id, "stepping away", NoteLabel::Fact);
    ThreadService::new(&db)
        .pause(&id, PauseReason::User)
        .unwrap();

    let checkpoint = CheckpointService::new(&db)
        .save(&id, CheckpointTrigger::ThreadEnded)
        .unwrap();
    let payload = payload(&checkpoint);
    assert!(!payload.final_snapshot.captured);
    assert!(
        payload
            .final_snapshot
            .reason
            .as_deref()
            .unwrap()
            .contains("paused")
    );
    // Pause events are kept: they explain gaps.
    assert!(payload.entries.iter().any(|e| e.kind == "thread.paused"));
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );
}

#[test]
fn a_second_checkpoint_covers_only_the_new_interval() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let checkpoints = CheckpointService::new(&db);
    let threads = ThreadService::new(&db);

    note(&db, &id, "first interval note", NoteLabel::Fact);
    let first = checkpoints.save(&id, CheckpointTrigger::Manual).unwrap();
    threads.mark_ready(&id).unwrap();
    threads.resume(&id).unwrap();
    note(&db, &id, "second interval note", NoteLabel::Hypothesis);
    let second = checkpoints.save(&id, CheckpointTrigger::Manual).unwrap();

    let first_payload = payload(&first);
    let second_payload = payload(&second);
    let texts = |p: &CheckpointPayload| -> Vec<String> {
        p.entries
            .iter()
            .filter(|e| e.kind == "note.added")
            .map(|e| e.content["text"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(texts(&first_payload), ["first interval note"]);
    assert_eq!(texts(&second_payload), ["second interval note"]);

    // The second interval begins at the resume that followed the first
    // checkpoint's request, so no evidence is shared between them.
    let events = db.list_all_events(&id).unwrap();
    let index = |event_id: &threadback_core::EventId| {
        events.iter().position(|e| &e.id == event_id).unwrap()
    };
    assert!(index(&second.start_event_id) > index(&first.end_event_id));
    assert_eq!(second_payload.entries[0].kind, "thread.resumed");
    assert_eq!(
        checkpoints.list(&id).unwrap()[0].id,
        second.id,
        "newest first"
    );
}

#[test]
fn a_failed_checkpoint_attempt_leaves_no_partial_state() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    ThreadService::new(&db).begin_checkpoint(&id).unwrap();
    ThreadService::new(&db).mark_failed(&id).unwrap();

    assert!(service.save(&id, CheckpointTrigger::Manual).is_err());
    assert!(service.list(&id).unwrap().is_empty());
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::Failed
    );
}

#[test]
fn unknown_checkpoint_is_not_found() {
    let db = Database::open_in_memory().unwrap();
    let error = CheckpointService::new(&db)
        .get(&threadback_core::CheckpointId::new())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

// ---- Discarding a draft ----

use threadback_core::{EventPayload as Payload, STATUS_DISCARDED};

#[test]
fn discarding_a_draft_returns_the_thread_to_recording() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    let checkpoint = service.save(&id, CheckpointTrigger::Manual).unwrap();
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );

    let thread = service.discard(&checkpoint.id).unwrap();
    assert_eq!(thread.state, ThreadState::Active);
    assert_eq!(
        db.find_thread(&id).unwrap().unwrap().state,
        ThreadState::Active
    );
    assert_eq!(
        service.get(&checkpoint.id).unwrap().status,
        STATUS_DISCARDED
    );

    // The discard is part of the thread's history.
    let events = db.list_all_events(&id).unwrap();
    let Payload::CheckpointDiscarded(value) = &events.last().unwrap().payload else {
        panic!("the last event should record the discard");
    };
    assert_eq!(value["checkpointId"], checkpoint.id.0);

    // Evidence is accepted again.
    note(&db, &id, "back at work", NoteLabel::Fact);
}

#[test]
fn discarding_a_draft_made_by_ending_reopens_the_thread() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    let checkpoint = service.save(&id, CheckpointTrigger::ThreadEnded).unwrap();
    assert!(db.find_thread(&id).unwrap().unwrap().ended_at.is_some());

    let thread = service.discard(&checkpoint.id).unwrap();
    assert_eq!(thread.state, ThreadState::Active);
    assert!(
        thread.ended_at.is_none(),
        "an ended thread that is reopened is not ended"
    );
    assert!(db.find_thread(&id).unwrap().unwrap().ended_at.is_none());
}

#[test]
fn a_discarded_draft_does_not_consume_its_evidence() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);

    note(&db, &id, "before the first draft", NoteLabel::Fact);
    let discarded = service.save(&id, CheckpointTrigger::Manual).unwrap();
    service.discard(&discarded.id).unwrap();
    note(&db, &id, "after the discard", NoteLabel::Hypothesis);
    let second = service.save(&id, CheckpointTrigger::Manual).unwrap();

    let texts: Vec<String> = payload(&second)
        .entries
        .iter()
        .filter(|e| e.kind == "note.added")
        .map(|e| e.content["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(texts, ["before the first draft", "after the discard"]);
    assert_eq!(
        second.start_event_id, discarded.start_event_id,
        "the interval starts where the first would have"
    );
    // Newest first, and the discarded one is still on record.
    let all = service.list(&id).unwrap();
    assert_eq!(
        (all[0].status.as_str(), all[1].status.as_str()),
        (STATUS_DRAFT, STATUS_DISCARDED)
    );
}

#[test]
fn only_an_unused_draft_can_be_discarded_and_only_once() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let id = start(&db, &approve(&db, repo.path()));
    let service = CheckpointService::new(&db);
    let checkpoint = service.save(&id, CheckpointTrigger::Manual).unwrap();
    service.discard(&checkpoint.id).unwrap();

    let error = service.discard(&checkpoint.id).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert_eq!(
        service
            .discard(&threadback_core::CheckpointId::new())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[test]
fn a_draft_cannot_be_discarded_while_another_thread_is_recording() {
    let repo = repository();
    let db = Database::open_in_memory().unwrap();
    let project_id = approve(&db, repo.path());
    let first = start(&db, &project_id);
    let service = CheckpointService::new(&db);
    let checkpoint = service.save(&first, CheckpointTrigger::Manual).unwrap();
    let second = start(&db, &project_id);

    let error = service.discard(&checkpoint.id).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert!(
        error
            .message
            .contains("Another thread is already recording")
    );
    // Nothing changed: the draft is still a draft and the first thread is still drafting.
    assert_eq!(service.get(&checkpoint.id).unwrap().status, STATUS_DRAFT);
    assert_eq!(
        db.find_thread(&first).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );
    assert_eq!(
        db.find_thread(&second).unwrap().unwrap().state,
        ThreadState::Active
    );
}
