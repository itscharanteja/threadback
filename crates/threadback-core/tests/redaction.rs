//! The C-01 guarantee: no credential character reaches the database, however
//! the data got there, while the terminal still receives the raw output.

use std::{fs, path::Path};

use chrono::Utc;
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, CaptureLimits, CheckpointService, CheckpointTrigger, CommandRequest, Database,
    EventId, EventPayload, NoteLabel, Project, ProjectId, RunIo, Sensitivity, StartThreadInput,
    ThreadEvent, ThreadId, ThreadService, run_command,
};

const SECRET: &str = "nbf_demo_7F4A91C2_DO_NOT_SEND";
const PASSWORD: &str = "c0rrect-h0rse-battery";
const KEY_BODY: &str = "MIIEowIBAAKCAQEAqZ3k0W1d7YtR2vXn";

struct Fixture {
    _dir: TempDir,
    database: std::path::PathBuf,
    project: std::path::PathBuf,
    thread_id: ThreadId,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let database = dir.path().join("threadback.db");
    let db = Database::open(&database).unwrap();
    let now = Utc::now();
    let record = Project {
        id: ProjectId::new(),
        display_name: "Payments".into(),
        canonical_root: fs::canonicalize(&project)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        created_at: now,
        updated_at: now,
    };
    db.insert_project(&record).unwrap();
    let thread_id = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id: record.id,
            title: "Investigation".into(),
            goal: "Find the second call".into(),
            hypothesis: None,
        })
        .unwrap()
        .id;
    Fixture {
        _dir: dir,
        database,
        project,
        thread_id,
    }
}

fn run(db: &Database, f: &Fixture, script: &str, limits: CaptureLimits) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut notify = |message: &str| panic!("unexpected notice: {message}");
    run_command(
        Some(db),
        &CommandRequest {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: f.project.clone(),
            limits,
        },
        RunIo {
            stdout: &mut out,
            stderr: &mut err,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap();
    (out, err)
}

/// Every byte the database wrote to disk, including the write-ahead log.
fn database_bytes(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let file = format!("{}{suffix}", path.display());
        if let Ok(content) = fs::read(&file) {
            bytes.extend(content);
        }
    }
    bytes
}

/// Fails if the secret, or any 8-character piece of it, is on disk.
fn assert_not_on_disk(path: &Path, secret: &str) {
    let bytes = database_bytes(path);
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains(secret), "{secret} is in the database files");
    let chars: Vec<char> = secret.chars().collect();
    for window in chars.windows(8.min(chars.len())) {
        let piece: String = window.iter().collect();
        assert!(
            !text.contains(&piece),
            "fragment {piece:?} of {secret:?} is in the database files"
        );
    }
}

fn events(db: &Database, id: &ThreadId) -> Vec<ThreadEvent> {
    db.list_all_events(id).unwrap()
}

#[test]
fn a_note_with_a_credential_is_stored_redacted_and_marked_sensitive() {
    let f = fixture();
    {
        let db = Database::open(&f.database).unwrap();
        let service = ThreadService::new(&db);
        service
            .add_note(
                &f.thread_id,
                AddNoteInput {
                    text: format!(
                        "curl got 401 with Authorization: Bearer {SECRET} and password={PASSWORD}"
                    ),
                    label: NoteLabel::Fact,
                },
            )
            .unwrap();
    }
    let db = Database::open(&f.database).unwrap();
    let stored = events(&db, &f.thread_id);
    let note = stored.last().unwrap();
    let EventPayload::NoteAdded { text, label } = &note.payload else {
        panic!()
    };
    assert_eq!(*label, NoteLabel::Fact, "the user's label is untouched");
    assert_eq!(
        text,
        "curl got 401 with Authorization: Bearer [REDACTED:bearer-token] and password=[REDACTED:secret-value]"
    );
    assert_eq!(note.sensitivity, Sensitivity::Sensitive);
    drop(db);
    assert_not_on_disk(&f.database, SECRET);
    assert_not_on_disk(&f.database, PASSWORD);
}

#[test]
fn ordinary_notes_are_untouched_and_stay_normal() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    let text = "  The focused test made two provider calls.\n";
    ThreadService::new(&db)
        .add_note(
            &f.thread_id,
            AddNoteInput {
                text: text.into(),
                label: NoteLabel::Hypothesis,
            },
        )
        .unwrap();
    let note = events(&db, &f.thread_id).pop().unwrap();
    assert!(
        matches!(&note.payload, EventPayload::NoteAdded { text: stored, .. } if stored == text)
    );
    assert_eq!(note.sensitivity, Sensitivity::Normal);
}

#[test]
fn the_content_hash_does_not_reveal_the_secret() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    let service = ThreadService::new(&db);
    let hash_of = |secret: &str| {
        service
            .add_note(
                &f.thread_id,
                AddNoteInput {
                    text: format!("token {secret}"),
                    label: NoteLabel::Fact,
                },
            )
            .unwrap()
            .content_hash
    };
    // Two different secrets redact to identical text, so their hashes match:
    // the hash cannot be used to guess a secret offline.
    assert_eq!(
        hash_of("sk_live_AAAA1111BBBB"),
        hash_of("sk_live_ZZZZ9999YYYY")
    );
}

#[test]
fn a_goal_or_title_with_a_credential_is_redacted_in_the_thread_record() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    // Approve is not needed: start an extra thread after pausing the first.
    ThreadService::new(&db)
        .pause(&f.thread_id, threadback_core::PauseReason::User)
        .unwrap();
    let project_id = db.list_projects().unwrap().remove(0).id;
    let thread = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id,
            title: format!("Debug {SECRET}"),
            goal: format!("Why does password={PASSWORD} fail?"),
            hypothesis: Some(format!("the key {SECRET} expired")),
        })
        .unwrap();
    assert_eq!(thread.title, "Debug [REDACTED:api-token]");
    assert_eq!(
        thread.goal,
        "Why does password=[REDACTED:secret-value] fail?"
    );
    let stored = db.find_thread(&thread.id).unwrap().unwrap();
    assert_eq!(stored.goal, thread.goal);
    drop(db);
    assert_not_on_disk(&f.database, SECRET);
    assert_not_on_disk(&f.database, PASSWORD);
}

#[test]
fn command_output_reaches_the_terminal_raw_but_is_stored_redacted() {
    let f = fixture();
    let (out, err) = {
        let db = Database::open(&f.database).unwrap();
        run(
            &db,
            &f,
            &format!(
                "echo 'provider request: Authorization: Bearer {SECRET}'; \
                 echo 'DB_PASSWORD={PASSWORD}' >&2; echo ordinary line; exit 1"
            ),
            CaptureLimits::default(),
        )
    };
    assert!(
        String::from_utf8_lossy(&out).contains(SECRET),
        "the terminal gets the raw output"
    );
    assert!(String::from_utf8_lossy(&err).contains(PASSWORD));

    let db = Database::open(&f.database).unwrap();
    let finished = events(&db, &f.thread_id)
        .into_iter()
        .find(|e| matches!(e.payload, EventPayload::CommandFinished(_)))
        .unwrap();
    let EventPayload::CommandFinished(value) = &finished.payload else {
        panic!()
    };
    assert_eq!(
        value["stdout"]["head"],
        "provider request: Authorization: Bearer [REDACTED:bearer-token]\nordinary line\n"
    );
    assert_eq!(
        value["stderr"]["head"],
        "DB_PASSWORD=[REDACTED:secret-value]\n"
    );
    assert_eq!(value["stdout"]["redactions"], 1);
    assert_eq!(value["stderr"]["redactions"], 1);
    assert_eq!(value["exitCode"], 1, "the exit status is unaffected");
    assert_eq!(finished.sensitivity, Sensitivity::Sensitive);
    drop(db);
    assert_not_on_disk(&f.database, SECRET);
    assert_not_on_disk(&f.database, PASSWORD);
}

#[test]
fn original_size_still_describes_what_the_command_wrote() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    let line = format!("Authorization: Bearer {SECRET}\n");
    run(
        &db,
        &f,
        &format!("printf '%s' '{line}'"),
        CaptureLimits::default(),
    );
    let finished = events(&db, &f.thread_id)
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::CommandFinished(v) => Some(v),
            _ => None,
        })
        .unwrap();
    assert_eq!(finished["stdout"]["originalBytes"], line.len() as u64);
    assert_eq!(finished["stdout"]["truncated"], false);
}

#[test]
fn a_credential_cannot_leak_through_a_head_or_tail_cut() {
    // Whatever the cut positions, no part of the secret may appear, even when
    // the cut falls in the middle of it.
    for head in 0..60 {
        let f = fixture();
        let db = Database::open(&f.database).unwrap();
        let script = format!(
            "echo 'aaaa bbbb Authorization: Bearer {SECRET} cccc dddd'; \
             echo 'eeee ffff {SECRET} gggg hhhh'; echo done"
        );
        run(
            &db,
            &f,
            &script,
            CaptureLimits {
                head_bytes: head,
                tail_bytes: 70 - head,
            },
        );
        drop(db);
        assert_not_on_disk(&f.database, SECRET);
    }
}

#[test]
fn a_private_key_in_output_is_removed_even_when_the_excerpt_cuts_through_it() {
    let f = fixture();
    {
        let db = Database::open(&f.database).unwrap();
        let script = format!(
            "echo before; echo '-----BEGIN RSA PRIVATE KEY-----'; \
             for i in 1 2 3 4 5 6 7 8; do echo '{KEY_BODY}'$i; done; \
             echo '-----END RSA PRIVATE KEY-----'; echo after"
        );
        run(
            &db,
            &f,
            &script,
            CaptureLimits {
                head_bytes: 40,
                tail_bytes: 40,
            },
        );
    }
    assert_not_on_disk(&f.database, KEY_BODY);

    let f2 = fixture();
    {
        let db = Database::open(&f2.database).unwrap();
        let script = format!(
            "echo before; echo '-----BEGIN RSA PRIVATE KEY-----'; \
             for i in 1 2 3 4 5 6 7 8; do echo '{KEY_BODY}'$i; done; \
             echo '-----END RSA PRIVATE KEY-----'; echo after"
        );
        run(&db, &f2, &script, CaptureLimits::default());
        let finished = events(&db, &f2.thread_id)
            .into_iter()
            .find_map(|e| match e.payload {
                EventPayload::CommandFinished(v) => Some(v),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            finished["stdout"]["head"],
            "before\n[REDACTED:private-key]\nafter\n"
        );
    }
    assert_not_on_disk(&f2.database, KEY_BODY);
}

#[test]
fn a_secret_in_the_command_line_is_redacted_in_the_command_text() {
    let f = fixture();
    {
        let db = Database::open(&f.database).unwrap();
        run(
            &db,
            &f,
            &format!("true # {SECRET}"),
            CaptureLimits::default(),
        );
    }
    assert_not_on_disk(&f.database, SECRET);
}

#[test]
fn the_storage_layer_redacts_events_that_bypass_the_normal_constructor() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    let event = ThreadEvent {
        id: EventId::new(),
        thread_id: f.thread_id.clone(),
        timestamp: Utc::now(),
        payload: EventPayload::NoteAdded {
            text: format!("pasted {SECRET}"),
            label: NoteLabel::Fact,
        },
        sensitivity: Sensitivity::Normal,
        content_hash: "made-up".into(),
    };
    db.insert_event(&event).unwrap();

    let stored = events(&db, &f.thread_id).pop().unwrap();
    let EventPayload::NoteAdded { text, .. } = &stored.payload else {
        panic!()
    };
    assert_eq!(text, "pasted [REDACTED:api-token]");
    assert_eq!(stored.sensitivity, Sensitivity::Sensitive);
    assert_ne!(
        stored.content_hash, "made-up",
        "the hash is recomputed from the redacted text"
    );
    drop(db);
    assert_not_on_disk(&f.database, SECRET);
}

#[test]
fn checkpoints_built_from_redacted_evidence_contain_no_secret() {
    let f = fixture();
    {
        let db = Database::open(&f.database).unwrap();
        run(
            &db,
            &f,
            &format!("echo 'Authorization: Bearer {SECRET}'; exit 1"),
            CaptureLimits::default(),
        );
        ThreadService::new(&db)
            .add_note(
                &f.thread_id,
                AddNoteInput {
                    text: format!("saw {SECRET}"),
                    label: NoteLabel::Fact,
                },
            )
            .unwrap();
        let checkpoint = CheckpointService::new(&db)
            .save(&f.thread_id, CheckpointTrigger::Manual)
            .unwrap();
        assert!(!checkpoint.payload.to_string().contains(SECRET));
        assert!(checkpoint.payload.to_string().contains("[REDACTED:"));
    }
    assert_not_on_disk(&f.database, SECRET);
}

#[test]
fn home_directories_in_output_are_rewritten() {
    let f = fixture();
    let db = Database::open(&f.database).unwrap();
    run(
        &db,
        &f,
        "echo 'error in /Users/charansriteja/Documents/app/src/a.rs:4'",
        CaptureLimits::default(),
    );
    let finished = events(&db, &f.thread_id)
        .into_iter()
        .find_map(|e| match e.payload {
            EventPayload::CommandFinished(v) => Some(v),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        finished["stdout"]["head"],
        "error in ~/Documents/app/src/a.rs:4\n"
    );
    assert_eq!(
        finished["stdout"]["redactions"], 0,
        "a path is rewritten, not counted as a secret"
    );
}
