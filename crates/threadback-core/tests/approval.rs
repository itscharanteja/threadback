//! M-05: the model's draft and the user's approved capsule are separate, the
//! draft is never written to, and every change is recorded by field path.

use std::{collections::BTreeMap, fs, path::PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, ApprovalService, CapsuleModel, CaptureLimits, Checkpoint, CheckpointService,
    CheckpointTrigger, CommandRequest, Correction, Database, DomainError, ErrorCode, EventId,
    EventPayload, ModelReply, NoteLabel, OutboundService, Project, ProjectId, PromptRequest,
    Redactor, ResumeCapsule, RunIo, STATUS_APPROVED, STATUS_DISCARDED, STATUS_GENERATED,
    StartThreadInput, ThreadId, ThreadService, ThreadState, generate_draft, plan_generation,
};

struct Fixed(String);

impl CapsuleModel for Fixed {
    fn complete(&self, _: &PromptRequest) -> Result<ModelReply, DomainError> {
        Ok(ModelReply {
            text: self.0.clone(),
        })
    }
}

struct Flow {
    _dir: TempDir,
    path: PathBuf,
    db: Database,
    thread_id: ThreadId,
    checkpoint: Checkpoint,
    excluded: Vec<EventId>,
    hash: String,
    fact_note: EventId,
    hypothesis_note: EventId,
    command_event: EventId,
}

fn flow() -> Flow {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(project.join("tests")).unwrap();
    let path = dir.path().join("threadback.db");
    let db = Database::open(&path).unwrap();
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
    let add = |text: &str, label| {
        ThreadService::new(&db)
            .add_note(
                &thread_id,
                AddNoteInput {
                    text: text.into(),
                    label,
                },
            )
            .unwrap()
            .id
    };
    let fact_note = add(
        "The focused retry test made two provider calls.",
        NoteLabel::Fact,
    );
    let hypothesis_note = add(
        "A second worker bypasses the distributed lock.",
        NoteLabel::Hypothesis,
    );
    let kept_from_model = add("The lock backend is Redis in staging.", NoteLabel::Fact);

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut notify = |m: &str| panic!("{m}");
    threadback_core::run_command(
        Some(&db),
        &CommandRequest {
            program: "sh".into(),
            args: vec!["-c".into(), "echo '2 provider calls'; exit 1".into()],
            cwd: project.clone(),
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

    let checkpoint = CheckpointService::new(&db)
        .save(&thread_id, CheckpointTrigger::Manual)
        .unwrap();
    let excluded = vec![kept_from_model];
    let service = OutboundService::new(&db);
    let preview = service.prepare(&checkpoint.id, &excluded).unwrap();
    let approved = service
        .authorize(&checkpoint.id, &excluded, &preview.payload_hash)
        .unwrap();
    let command_event = approved
        .document()
        .entries
        .iter()
        .find(|e| e.kind == "command.finished")
        .unwrap()
        .event_id
        .clone();

    Flow {
        _dir: dir,
        path,
        db,
        thread_id,
        checkpoint,
        excluded,
        hash: preview.payload_hash,
        fact_note,
        hypothesis_note,
        command_event,
    }
}

impl Flow {
    fn model_reply(&self) -> String {
        let mut capsule: Value = serde_json::from_str(
            &fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/evaluation/capsules/valid/spec-example.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let command = &self.command_event.0;
        capsule["verified_facts"][0]["source_event_ids"] = json!([self.fact_note.0, command]);
        capsule["hypotheses"][0]["source_event_ids"] = json!([self.hypothesis_note.0]);
        capsule["failed_attempts"][0]["source_event_ids"] = json!([command]);
        capsule["relevant_artifacts"][0]["source_event_ids"] = json!([command]);
        capsule["next_action"]["source_event_ids"] = json!([command, self.hypothesis_note.0]);
        capsule.to_string()
    }

    /// The checkpoint after the model has produced its draft.
    fn generated(self) -> Self {
        generate_draft(
            &self.db,
            &self.checkpoint.id,
            &self.excluded,
            &self.hash,
            Redactor::builtin(),
            &Fixed(self.model_reply()),
            "nvidia/test-model",
        )
        .unwrap();
        self
    }

    fn stored(&self) -> Checkpoint {
        self.db
            .find_checkpoint(&self.checkpoint.id)
            .unwrap()
            .unwrap()
    }

    fn draft(&self) -> ResumeCapsule {
        self.stored().draft.expect("a generated draft")
    }

    fn approve(
        &self,
        capsule: ResumeCapsule,
    ) -> Result<threadback_core::ApprovalResult, DomainError> {
        ApprovalService::new(&self.db).approve(&self.checkpoint.id, capsule)
    }

    fn thread_state(&self) -> ThreadState {
        self.db.find_thread(&self.thread_id).unwrap().unwrap().state
    }

    fn raw(&self, column: &str) -> Option<String> {
        Connection::open(&self.path)
            .unwrap()
            .query_row(
                &format!("SELECT {column} FROM checkpoints WHERE id = ?1"),
                [&self.checkpoint.id.0],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn correction_rows(&self) -> i64 {
        Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM corrections", [], |row| row.get(0))
            .unwrap()
    }

    fn approved_events(&self) -> Vec<Value> {
        self.db
            .list_all_events(&self.thread_id)
            .unwrap()
            .into_iter()
            .filter_map(|event| match event.payload {
                EventPayload::CheckpointApproved(value) => Some(value),
                _ => None,
            })
            .collect()
    }
}

fn by_path(corrections: &[Correction]) -> BTreeMap<&str, (&Value, &Value)> {
    corrections
        .iter()
        .map(|c| (c.field_path.as_str(), (&c.before, &c.after)))
        .collect()
}

#[test]
fn approving_the_draft_as_it_stands_records_no_corrections() {
    let f = flow().generated();
    let draft = f.draft();
    let result = f.approve(draft.clone()).unwrap();

    assert!(result.corrections.is_empty());
    assert_eq!(result.checkpoint.status, STATUS_APPROVED);
    assert_eq!(result.checkpoint.approved.as_ref(), Some(&draft));
    assert_eq!(result.checkpoint.draft.as_ref(), Some(&draft));
    assert!(result.checkpoint.approved_at.is_some());
    assert_eq!(result.thread.state, ThreadState::Ready);
    assert_eq!(f.thread_state(), ThreadState::Ready);

    let events = f.approved_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["checkpointId"], json!(f.checkpoint.id));
    assert_eq!(events[0]["correctionCount"], 0);
    assert_eq!(events[0]["approvedHash"].as_str().unwrap().len(), 64);
}

#[test]
fn edits_are_recorded_by_field_path_and_the_draft_is_not_touched() {
    let f = flow().generated();
    let draft_before = f.raw("draft_json").unwrap();

    let mut edited = f.draft();
    edited.verified_facts[0].claim =
        "The focused retry test made two provider calls (twice).".into();
    edited.hypotheses.clear();
    edited
        .unresolved_questions
        .push("Does staging use the same Redis as production?".into());
    edited.next_action.instruction = "Print the lock owner in both workers.".into();
    let result = f.approve(edited.clone()).unwrap();

    // The model's draft is byte-for-byte what it was.
    assert_eq!(f.raw("draft_json").unwrap(), draft_before);
    assert_ne!(result.checkpoint.draft, result.checkpoint.approved);
    assert_eq!(result.checkpoint.approved.as_ref(), Some(&edited));
    assert_eq!(
        f.stored().draft.unwrap(),
        serde_json::from_str::<ResumeCapsule>(&draft_before).unwrap()
    );

    let changes = by_path(&result.corrections);
    assert_eq!(changes.len(), 4, "{changes:?}");
    assert_eq!(
        changes["/verified_facts/0/claim"],
        (
            &json!("The focused retry test made two provider calls."),
            &json!("The focused retry test made two provider calls (twice).")
        )
    );
    let (removed, after) = changes["/hypotheses/0"];
    assert_eq!(removed["id"], "hyp_1");
    assert!(after.is_null());
    let (before, added) = changes["/unresolved_questions/1"];
    assert!(before.is_null());
    assert_eq!(
        added,
        &json!("Does staging use the same Redis as production?")
    );
    assert_eq!(
        changes["/next_action/instruction"].1,
        &json!("Print the lock owner in both workers.")
    );

    assert_eq!(f.approved_events()[0]["correctionCount"], 4);
    assert_eq!(f.correction_rows(), 4);
}

#[test]
fn every_correction_can_be_listed_again_in_the_order_it_was_made() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.checkpoint_summary = "Retries still make two provider calls.".into();
    edited.limitations.push("Staging was not inspected.".into());
    let result = f.approve(edited).unwrap();

    let listed = ApprovalService::new(&f.db)
        .corrections(&f.checkpoint.id)
        .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed.iter().map(|c| &c.id).collect::<Vec<_>>(),
        result.corrections.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
    assert!(listed.iter().all(|c| c.checkpoint_id == f.checkpoint.id));
    assert!(listed.iter().all(|c| c.reason.is_none()));
}

#[test]
fn relabelling_a_fact_as_a_hypothesis_is_recorded_as_a_removal_and_an_addition() {
    let f = flow().generated();
    let mut edited = f.draft();
    let fact = edited.verified_facts.remove(0);
    edited.hypotheses.push(threadback_core::Hypothesis {
        id: "hyp_2".into(),
        claim: fact.claim.clone(),
        confidence: threadback_core::Confidence::Low,
        source_event_ids: vec![f.fact_note.clone()],
    });
    let result = f.approve(edited).unwrap();

    let changes = by_path(&result.corrections);
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes["/verified_facts/0"].0["id"], fact.id);
    assert!(changes["/verified_facts/0"].1.is_null());
    assert!(changes["/hypotheses/1"].0.is_null());
    assert_eq!(changes["/hypotheses/1"].1["claim"], fact.claim);
    assert!(
        result
            .checkpoint
            .approved
            .unwrap()
            .verified_facts
            .is_empty()
    );
}

#[test]
fn an_edit_cannot_make_a_verified_fact_rest_on_a_hypothesis() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.verified_facts[0].source_event_ids = vec![f.hypothesis_note.clone()];

    let error = f.approve(edited).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidCapsule);
    assert_eq!(
        error.field.as_deref(),
        Some("/verified_facts/0/source_event_ids")
    );
    assert!(
        error.message.contains("cannot be a verified fact"),
        "{error}"
    );

    // Nothing was approved, recorded, or moved.
    assert_eq!(f.stored().status, STATUS_GENERATED);
    assert!(f.stored().approved.is_none());
    assert_eq!(f.correction_rows(), 0);
    assert!(f.approved_events().is_empty());
    assert_eq!(f.thread_state(), ThreadState::CheckpointDraft);
}

#[test]
fn an_edit_cannot_cite_an_event_that_is_not_in_the_checkpoint() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.verified_facts[0]
        .source_event_ids
        .push(EventId("evt_made_up".into()));

    let error = f.approve(edited).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidCapsule);
    assert_eq!(
        error.field.as_deref(),
        Some("/verified_facts/0/source_event_ids/2")
    );
    assert!(f.stored().approved.is_none());
}

#[test]
fn an_edit_may_cite_evidence_that_was_kept_from_the_model() {
    let f = flow().generated();
    let kept = f.excluded[0].clone();
    let mut edited = f.draft();
    edited.verified_facts[0].source_event_ids.push(kept.clone());

    let result = f.approve(edited).unwrap();
    assert!(
        result.checkpoint.approved.unwrap().verified_facts[0]
            .source_event_ids
            .contains(&kept)
    );
}

#[test]
fn an_edit_with_a_credential_is_refused_and_the_credential_is_not_stored() {
    let f = flow().generated();
    let secret = "Bearer abcdefghijklmnopqrstuvwxyz0123456789ABCDEF";
    let mut edited = f.draft();
    edited.limitations.push(format!("Authorization: {secret}"));

    let error = f.approve(edited).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidCapsule);
    assert!(
        !error.message.contains("abcdefghijklmnopqrstuvwxyz"),
        "{error}"
    );

    let bytes = fs::read(&f.path).unwrap();
    let wal = fs::read(f.path.with_extension("db-wal")).unwrap_or_default();
    for file in [bytes, wal] {
        assert!(
            !String::from_utf8_lossy(&file).contains("abcdefghijklmnopqrstuvwxyz0123"),
            "the credential reached the database file"
        );
    }
    assert!(f.stored().approved.is_none());
}

#[test]
fn an_edit_that_breaks_the_schema_is_refused() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.verified_facts[0].claim = String::new();

    let error = f.approve(edited).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidCapsule);
    assert_eq!(error.field.as_deref(), Some("/verified_facts/0/claim"));
    assert_eq!(f.correction_rows(), 0);
}

#[test]
fn a_second_approval_is_refused_and_changes_nothing() {
    let f = flow().generated();
    let mut first = f.draft();
    first.checkpoint_summary = "First approved wording.".into();
    f.approve(first.clone()).unwrap();
    let approved_before = f.raw("approved_json").unwrap();

    let mut second = f.draft();
    second.checkpoint_summary = "Second wording.".into();
    let error = f.approve(second).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);

    assert_eq!(f.raw("approved_json").unwrap(), approved_before);
    assert_eq!(f.stored().approved, Some(first));
    assert_eq!(f.correction_rows(), 1);
    assert_eq!(f.approved_events().len(), 1);
}

#[test]
fn nothing_can_be_approved_before_the_model_has_drafted_it() {
    let f = flow();
    let draft = serde_json::from_str::<ResumeCapsule>(&f.model_reply()).unwrap();
    let error = f.approve(draft).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert_eq!(f.thread_state(), ThreadState::CheckpointDraft);
    assert!(f.stored().approved.is_none());
}

#[test]
fn an_approved_checkpoint_cannot_be_discarded_or_generated_again() {
    let f = flow().generated();
    f.approve(f.draft()).unwrap();

    let discard = CheckpointService::new(&f.db).discard(&f.checkpoint.id);
    assert_eq!(discard.unwrap_err().code, ErrorCode::InvalidState);

    let again = plan_generation(
        &f.db,
        &f.checkpoint.id,
        &f.excluded,
        &f.hash,
        Redactor::builtin(),
    );
    assert_eq!(again.err().unwrap().code, ErrorCode::InvalidState);

    let stored = f.stored();
    assert_eq!(stored.status, STATUS_APPROVED);
    assert_ne!(stored.status, STATUS_DISCARDED);
}

#[test]
fn a_discarded_draft_cannot_be_approved() {
    let f = flow().generated();
    CheckpointService::new(&f.db)
        .discard(&f.checkpoint.id)
        .unwrap();

    let error = f.approve(f.draft()).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert!(f.stored().approved.is_none());
}

#[test]
fn both_versions_and_the_corrections_survive_reopening_the_database() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.checkpoint_summary = "Reworded after review.".into();
    let draft = f.draft();
    f.approve(edited.clone()).unwrap();

    let Flow {
        path,
        checkpoint,
        _dir,
        db,
        ..
    } = f;
    drop(db);
    let reopened = Database::open(&path).unwrap();
    let stored = reopened.find_checkpoint(&checkpoint.id).unwrap().unwrap();
    assert_eq!(stored.status, STATUS_APPROVED);
    assert_eq!(stored.draft, Some(draft));
    assert_eq!(stored.approved, Some(edited));
    let corrections = reopened.list_corrections(&checkpoint.id).unwrap();
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].field_path, "/checkpoint_summary");
}

#[test]
fn the_approval_event_records_the_fact_of_approval_and_not_the_content() {
    let f = flow().generated();
    let mut edited = f.draft();
    edited.checkpoint_summary = "A distinctive reworded summary 7Qz.".into();
    f.approve(edited).unwrap();

    let event = serde_json::to_string(&f.approved_events()[0]).unwrap();
    assert!(!event.contains("7Qz"), "{event}");
}

#[test]
fn approval_is_all_or_nothing() {
    let f = flow().generated();
    // Make the last step of approval, recording the event, fail.
    Connection::open(&f.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_approval_event BEFORE INSERT ON events
             WHEN NEW.event_type = 'checkpoint.approved'
             BEGIN SELECT RAISE(ABORT, 'refused for the test'); END;",
        )
        .unwrap();

    let mut edited = f.draft();
    edited.checkpoint_summary = "Reworded.".into();
    assert!(f.approve(edited).is_err());

    // The approved capsule, the corrections, and the new status were undone together.
    assert!(f.raw("approved_json").is_none());
    assert_eq!(f.stored().status, STATUS_GENERATED);
    assert_eq!(f.correction_rows(), 0);
    assert_eq!(f.thread_state(), ThreadState::CheckpointDraft);
}
