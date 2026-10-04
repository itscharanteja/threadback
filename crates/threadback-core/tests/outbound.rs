//! C-02: what may leave the computer. The only way to obtain content for a
//! model request is `OutboundService::authorize`, which refuses while anything
//! is blocked and when the payload is not the one the user reviewed.

use std::fs;

use chrono::Utc;
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, ApprovedPayload, CaptureLimits, Checkpoint, CheckpointId, CheckpointService,
    CheckpointTrigger, CommandRequest, Database, ErrorCode, EventId, NoteLabel, OutboundService,
    PayloadPreview, PreviewDisposition, Project, ProjectId, Redactor, RunIo, StartThreadInput,
    ThreadId, ThreadService, run_command,
};

const SECRET: &str = "nbf_demo_7F4A91C2_DO_NOT_SEND";

struct Fixture {
    _dir: TempDir,
    db: Database,
    project: std::path::PathBuf,
    thread_id: ThreadId,
}

fn fixture() -> Fixture {
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
    Fixture {
        _dir: dir,
        db,
        project,
        thread_id,
    }
}

fn note(f: &Fixture, text: &str, label: NoteLabel) -> EventId {
    ThreadService::new(&f.db)
        .add_note(
            &f.thread_id,
            AddNoteInput {
                text: text.into(),
                label,
            },
        )
        .unwrap()
        .id
}

fn run(f: &Fixture, script: &str) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut notify = |message: &str| panic!("unexpected notice: {message}");
    run_command(
        Some(&f.db),
        &CommandRequest {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: f.project.clone(),
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

fn save(f: &Fixture) -> Checkpoint {
    CheckpointService::new(&f.db)
        .save(&f.thread_id, CheckpointTrigger::Manual)
        .unwrap()
}

fn custom() -> Redactor {
    Redactor::new(&["ACME-[0-9]{6}".into()]).unwrap()
}

fn find<'a>(preview: &'a PayloadPreview, id: &EventId) -> &'a threadback_core::PayloadPreviewEntry {
    preview
        .entries
        .iter()
        .find(|entry| &entry.event_id == id)
        .unwrap()
}

fn denied(
    result: Result<ApprovedPayload, threadback_core::DomainError>,
) -> threadback_core::DomainError {
    match result {
        Ok(_) => panic!("authorization should have been refused"),
        Err(error) => error,
    }
}

#[test]
fn a_clean_draft_is_previewed_in_full_and_can_be_authorized() {
    let f = fixture();
    note(
        &f,
        "The focused test made two provider calls.",
        NoteLabel::Fact,
    );
    run(&f, "echo '2 provider calls'; exit 1");
    let checkpoint = save(&f);

    let service = OutboundService::new(&f.db);
    let preview = service.prepare(&checkpoint.id, &[]).unwrap();
    assert!(preview.can_generate && !preview.has_blocking_findings);
    assert_eq!(
        preview.goal,
        "Find why retries produce a second provider capture"
    );
    assert_eq!(
        preview.counts.blocked + preview.counts.local_only + preview.counts.excluded,
        0
    );
    assert!(preview.counts.included >= 4);

    let approved = service
        .authorize(&checkpoint.id, &[], &preview.payload_hash)
        .unwrap();
    assert_eq!(approved.hash(), preview.payload_hash);
    assert_eq!(approved.size_bytes(), preview.total_size_bytes as usize);
    assert_eq!(approved.checkpoint_id(), &checkpoint.id);
    assert_eq!(
        approved.document().entries.len() as u32,
        preview.counts.included
    );
    // The bytes that would be sent are exactly the document that was shown.
    assert_eq!(
        approved.json(),
        serde_json::to_string(approved.document()).unwrap()
    );
}

#[test]
fn the_seeded_credential_never_reaches_the_preview_or_the_approved_payload() {
    let f = fixture();
    note(
        &f,
        &format!("401 using Authorization: Bearer {SECRET}"),
        NoteLabel::Fact,
    );
    run(
        &f,
        &format!(
            "echo 'provider request: Authorization: Bearer {SECRET}'; echo 'token: {SECRET}' >&2; exit 1"
        ),
    );
    let checkpoint = save(&f);

    let service = OutboundService::new(&f.db);
    let preview = service.prepare(&checkpoint.id, &[]).unwrap();
    let shown = serde_json::to_string(&preview).unwrap();
    assert!(!shown.contains(SECRET) && !shown.contains("7F4A91C2"));
    assert!(
        shown.contains("[REDACTED:"),
        "the preview shows the replacement"
    );

    let approved = service
        .authorize(&checkpoint.id, &[], &preview.payload_hash)
        .unwrap();
    assert!(!approved.json().contains("7F4A91C2"));
    assert!(
        Redactor::builtin().find_secrets(approved.json()).is_empty(),
        "a second scan finds nothing"
    );
    assert!(approved.json().contains("[REDACTED:bearer-token]"));
}

#[test]
fn a_blocking_match_prevents_authorization_until_the_entry_is_excluded() {
    let f = fixture();
    note(&f, "fine observation", NoteLabel::Fact);
    let culprit = note(&f, "the culprit is ACME-123456", NoteLabel::Hypothesis);
    let checkpoint = save(&f);

    let redactor = custom();
    let service = OutboundService::with_redactor(&f.db, &redactor);
    let preview = service.prepare(&checkpoint.id, &[]).unwrap();
    assert!(preview.has_blocking_findings && !preview.can_generate);
    let blocked = find(&preview, &culprit);
    assert_eq!(blocked.disposition, PreviewDisposition::Blocked);
    assert_eq!(blocked.value["text"], "the culprit is [REDACTED:custom]");

    // Neither the right hash nor a guessed one gets past a block.
    let error = denied(service.authorize(&checkpoint.id, &[], &preview.payload_hash));
    assert_eq!(error.code, ErrorCode::PayloadBlocked);
    assert_eq!(
        denied(service.authorize(&checkpoint.id, &[], "anything")).code,
        ErrorCode::PayloadBlocked
    );

    // Excluding the entry clears the block and removes it from what is sent.
    let after = service.prepare(&checkpoint.id, &[culprit.clone()]).unwrap();
    assert_eq!(
        find(&after, &culprit).disposition,
        PreviewDisposition::Excluded
    );
    assert!(after.can_generate);
    let approved = service
        .authorize(&checkpoint.id, &[culprit.clone()], &after.payload_hash)
        .unwrap();
    assert!(!approved.json().contains("ACME-") && !approved.json().contains("culprit"));
    assert!(
        approved
            .document()
            .entries
            .iter()
            .all(|entry| entry.event_id != culprit)
    );
}

#[test]
fn authorization_requires_the_hash_of_what_was_reviewed() {
    let f = fixture();
    let first = note(&f, "first", NoteLabel::Fact);
    note(&f, "second", NoteLabel::Fact);
    let checkpoint = save(&f);
    let service = OutboundService::new(&f.db);

    let reviewed = service.prepare(&checkpoint.id, &[]).unwrap();
    // The user reviewed everything, but a different selection is being sent.
    let error = denied(service.authorize(&checkpoint.id, &[first.clone()], &reviewed.payload_hash));
    assert_eq!(error.code, ErrorCode::PayloadChanged);
    assert!(error.retryable);
    // An unrelated or empty hash is refused.
    assert_eq!(
        denied(service.authorize(&checkpoint.id, &[], "")).code,
        ErrorCode::PayloadChanged
    );
    assert_eq!(
        denied(service.authorize(&checkpoint.id, &[], &"0".repeat(64))).code,
        ErrorCode::PayloadChanged
    );
    // The same selection passes.
    assert!(
        service
            .authorize(&checkpoint.id, &[], &reviewed.payload_hash)
            .is_ok()
    );
}

#[test]
fn nothing_selected_is_refused() {
    let f = fixture();
    let checkpoint = save(&f);
    let service = OutboundService::new(&f.db);
    let everything: Vec<EventId> = service
        .prepare(&checkpoint.id, &[])
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.event_id)
        .collect();
    let preview = service.prepare(&checkpoint.id, &everything).unwrap();
    assert!(!preview.can_generate);
    assert_eq!(preview.counts.included, 0);
    let error = denied(service.authorize(&checkpoint.id, &everything, &preview.payload_hash));
    assert_eq!(error.code, ErrorCode::PayloadBlocked);
    assert!(error.message.contains("Nothing is selected"));
}

#[test]
fn excluding_something_that_is_not_in_the_checkpoint_is_an_error() {
    let f = fixture();
    let checkpoint = save(&f);
    let service = OutboundService::new(&f.db);
    let error = service
        .prepare(&checkpoint.id, &[EventId("not-an-event".into())])
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(error.field.as_deref(), Some("excludedEventIds"));
    let error = service.prepare(&CheckpointId::new(), &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn preparing_changes_nothing_and_is_repeatable() {
    let f = fixture();
    note(&f, "stable", NoteLabel::Fact);
    let checkpoint = save(&f);
    let before = f.db.find_checkpoint(&checkpoint.id).unwrap().unwrap();
    let service = OutboundService::new(&f.db);
    let first = service.prepare(&checkpoint.id, &[]).unwrap();
    let second = service.prepare(&checkpoint.id, &[]).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        f.db.find_checkpoint(&checkpoint.id)
            .unwrap()
            .unwrap()
            .payload,
        before.payload
    );
}

#[test]
fn a_credential_in_a_checkpoint_written_directly_is_still_redacted_in_storage() {
    let f = fixture();
    let real = save(&f);
    let mut forged = real.clone();
    forged.id = CheckpointId::new();
    forged.payload["goal"] = serde_json::json!(format!("goal with {SECRET}"));
    forged.payload["entries"][0]["content"]["goal"] = serde_json::json!(format!("Bearer {SECRET}"));
    f.db.insert_checkpoint(&forged).unwrap();

    let stored = f.db.find_checkpoint(&forged.id).unwrap().unwrap();
    assert!(!stored.payload.to_string().contains(SECRET));
    let service = OutboundService::new(&f.db);
    let preview = service.prepare(&forged.id, &[]).unwrap();
    assert!(!serde_json::to_string(&preview).unwrap().contains(SECRET));
}
