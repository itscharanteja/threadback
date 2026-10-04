//! M-03: prompt-injection fixtures must not alter the output contract.
//!
//! Hostile text is recorded the way a real thread would record it (as notes
//! and command output), reviewed, authorized, and turned into a prompt. The
//! evidence must stay data, and a response that obeys it must be rejected.

use std::{fs, path::PathBuf};

use chrono::Utc;
use serde_json::Value;
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, CapsuleSchema, CaptureLimits, CheckpointService, CheckpointTrigger,
    CommandRequest, Database, NoteLabel, OutboundPayload, OutboundService, PROMPT_VERSION, Project,
    ProjectId, ResumeCapsule, RunIo, StartThreadInput, ThreadService, build_prompt, capsule_schema,
    run_command,
};

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/evaluation/injection")
        .join(name)
}

fn injections() -> Vec<String> {
    serde_json::from_str(&fs::read_to_string(fixture_dir("evidence-notes.json")).unwrap()).unwrap()
}

/// Everything a response must survive: it is JSON, it matches the schema, and
/// it deserializes into the strict contract type.
fn accepted(response: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(response) else {
        return false;
    };
    CapsuleSchema::new().unwrap().is_valid(&value)
        && serde_json::from_value::<ResumeCapsule>(value).is_ok()
}

struct Setup {
    _dir: TempDir,
    db: Database,
    project: PathBuf,
    thread_id: threadback_core::ThreadId,
}

fn setup() -> Setup {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let db = Database::open(dir.path().join("threadback.db")).unwrap();
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
            title: "Duplicate capture investigation".into(),
            goal: "Find why retries produce a second provider capture".into(),
            hypothesis: None,
        })
        .unwrap()
        .id;
    Setup {
        _dir: dir,
        db,
        project,
        thread_id,
    }
}

fn print_through_a_command(setup: &Setup, text: &str) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut notify = |message: &str| panic!("unexpected notice: {message}");
    run_command(
        Some(&setup.db),
        &CommandRequest {
            program: "printf".into(),
            args: vec!["%s\n".into(), text.into()],
            cwd: setup.project.clone(),
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

#[test]
fn hostile_notes_and_output_reach_the_model_only_as_data() {
    let setup = setup();
    let hostile = injections();
    for text in &hostile {
        ThreadService::new(&setup.db)
            .add_note(
                &setup.thread_id,
                AddNoteInput {
                    text: text.clone(),
                    label: NoteLabel::Fact,
                },
            )
            .unwrap();
    }
    print_through_a_command(&setup, &hostile[0]);
    print_through_a_command(&setup, &hostile[1]);
    let checkpoint = CheckpointService::new(&setup.db)
        .save(&setup.thread_id, CheckpointTrigger::Manual)
        .unwrap();

    let outbound = OutboundService::new(&setup.db);
    let preview = outbound.prepare(&checkpoint.id, &[]).unwrap();
    assert!(
        preview.can_generate,
        "nothing here is a credential, so nothing is blocked"
    );
    let approved = outbound
        .authorize(&checkpoint.id, &[], &preview.payload_hash)
        .unwrap();
    let request = build_prompt(&approved).unwrap();

    // The system message is the same constant it is for any checkpoint.
    let quiet = {
        let quiet = self::setup();
        let checkpoint = CheckpointService::new(&quiet.db)
            .save(&quiet.thread_id, CheckpointTrigger::Manual)
            .unwrap();
        let service = OutboundService::new(&quiet.db);
        let hash = service.prepare(&checkpoint.id, &[]).unwrap().payload_hash;
        build_prompt(&service.authorize(&checkpoint.id, &[], &hash).unwrap()).unwrap()
    };
    assert_eq!(request.messages[0], quiet.messages[0]);
    assert_eq!(request.prompt_version, PROMPT_VERSION);
    assert_eq!(request.response_schema, capsule_schema());

    // The evidence is a single line that parses back to exactly what was reviewed.
    let evidence = request.evidence_line().unwrap();
    assert_eq!(evidence, approved.json());
    let document: OutboundPayload = serde_json::from_str(evidence).unwrap();
    let notes: Vec<&str> = document
        .entries
        .iter()
        .filter(|entry| entry.kind == "note.added")
        .map(|entry| entry.content["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        notes,
        hostile.iter().map(String::as_str).collect::<Vec<_>>(),
        "every note, verbatim, as a string value"
    );

    // Only the real marker lines are on their own lines, and the system
    // message never contains any hostile text.
    let user = &request.messages[1].content;
    assert_eq!(
        user.lines()
            .filter(|l| l.starts_with("<<<EVIDENCE:"))
            .count(),
        1
    );
    assert_eq!(user.lines().filter(|l| l.starts_with("<<<END:")).count(), 1);
    for text in &hostile {
        assert!(!request.messages[0].content.contains(text.trim()));
    }
}

#[test]
fn a_response_that_obeys_an_injection_is_rejected() {
    let mut checked = 0;
    for entry in fs::read_dir(fixture_dir("obeyed")).unwrap() {
        let path = entry.unwrap().path();
        let response = fs::read_to_string(&path).unwrap();
        assert!(!accepted(&response), "{} was accepted", path.display());
        checked += 1;
    }
    assert!(
        checked >= 9,
        "expected the full set of obeyed-injection responses, found {checked}"
    );
}

#[test]
fn a_genuine_capsule_is_still_accepted() {
    // Rejecting hostile responses must not mean rejecting everything.
    for entry in fs::read_dir(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/evaluation/capsules/valid"),
    )
    .unwrap()
    {
        let path = entry.unwrap().path();
        assert!(
            accepted(&fs::read_to_string(&path).unwrap()),
            "{}",
            path.display()
        );
    }
}

#[test]
fn wrapping_a_valid_capsule_in_prose_or_fences_is_not_accepted_as_is() {
    // Whether to tolerate these is a validation-and-repair decision (M-04);
    // here it is enough that the strict contract does not accept them silently.
    let valid = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/evaluation/capsules/valid/spec-example.json"),
    )
    .unwrap();
    assert!(accepted(&valid));
    assert!(!accepted(&format!("Here you go:\n{valid}")));
    assert!(!accepted(&format!("```json\n{valid}\n```")));
    assert!(!accepted(&format!(
        "{valid}\nLet me know if you need anything else."
    )));
}
