use std::{fs, path::Path, process::Command};

use chrono::{Duration, Utc};
use serde_json::json;
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, ApprovalService, CapsuleModel, CaptureLimits, CheckpointPayload,
    CheckpointService, CheckpointTrigger, CommandRequest, CreateProjectInput, Database,
    DomainError, DriftKind, ErrorCode, EventId, EventPayload, ModelReply, NoteLabel,
    OutboundService, PromptRequest, Redactor, RelinkProjectInput, RunIo, Sensitivity,
    StartThreadInput, ThreadEvent, ThreadService, ThreadState, approve_project, generate_draft,
    relink_project, run_command,
};

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git must run");
    assert!(status.success(), "git {:?} failed", args);
}

fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/retry.rs"),
        "pub fn max_retries() -> usize { 3 }\n",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "Initial commit"]);
    dir
}

struct FixedModel(serde_json::Value);

impl CapsuleModel for FixedModel {
    fn complete(&self, _request: &PromptRequest) -> Result<ModelReply, DomainError> {
        Ok(ModelReply {
            text: self.0.to_string(),
        })
    }
}

#[test]
fn collector_policy_toggles_disable_git_and_command_capture_when_turned_off() {
    let db = Database::open_in_memory().unwrap();
    let repo = init_repo();
    let project = approve_project(
        &db,
        CreateProjectInput {
            display_name: "ToggleRepo".into(),
            root_path: repo.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    // Disable both git snapshots and command capture in project policy
    let mut policy = db.find_project_policy(&project.id).unwrap();
    policy.capture_git = false;
    policy.capture_commands = false;
    db.upsert_project_policy(&policy).unwrap();

    let thread = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id: project.id.clone(),
            title: "Policy toggle check".into(),
            goal: "Verify disabled collectors do not record events".into(),
            hypothesis: None,
        })
        .unwrap();

    // capture_git should be rejected when capture_git is false
    let snap_err = ThreadService::new(&db).capture_git(&thread.id).unwrap_err();
    assert_eq!(snap_err.code, ErrorCode::InvalidState);

    // run_command should still execute the child process and return its exit code,
    // but record no command events when capture_commands is false.
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut notices = Vec::new();
    let mut notify = |msg: &str| notices.push(msg.to_string());
    let req = CommandRequest {
        cwd: repo.path().to_path_buf(),
        program: "echo".into(),
        args: vec!["unrecorded".into()],
        limits: CaptureLimits::default(),
    };
    let outcome = run_command(
        Some(&db),
        &req,
        RunIo {
            stdout: &mut out,
            stderr: &mut err,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap();
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.started_event_id.is_none());
    assert!(outcome.finished_event_id.is_none());

    // Save checkpoint while capture_git is false: final_snapshot.captured must be false with reason
    let cp = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();
    let payload: CheckpointPayload = serde_json::from_value(cp.payload).unwrap();
    assert!(!payload.final_snapshot.captured);
    assert!(
        payload
            .final_snapshot
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("turned off")
    );
}

#[test]
fn discarding_draft_preserves_evidence_for_subsequent_checkpoint_and_multi_interval_resume_excludes_prior_interval()
 {
    let db = Database::open_in_memory().unwrap();
    let repo = init_repo();
    let project = approve_project(
        &db,
        CreateProjectInput {
            display_name: "Intervals".into(),
            root_path: repo.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    let thread = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id: project.id.clone(),
            title: "Multi-interval test".into(),
            goal: "Verify discard and multi-interval boundaries".into(),
            hypothesis: None,
        })
        .unwrap();

    let note_1 = ThreadService::new(&db)
        .add_note(
            &thread.id,
            AddNoteInput {
                text: "First observation before discard.".into(),
                label: NoteLabel::Fact,
            },
        )
        .unwrap();

    let draft_1 = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();
    let active_thread = CheckpointService::new(&db).discard(&draft_1.id).unwrap();
    assert_eq!(active_thread.state, ThreadState::Active);

    let note_2 = ThreadService::new(&db)
        .add_note(
            &thread.id,
            AddNoteInput {
                text: "Second observation after discard.".into(),
                label: NoteLabel::Fact,
            },
        )
        .unwrap();

    // Second save must include BOTH note_1 and note_2 because draft_1 was discarded
    let cp_1 = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();
    let payload_1: CheckpointPayload = serde_json::from_value(cp_1.payload.clone()).unwrap();
    assert!(payload_1.entries.iter().any(|e| e.event_id == note_1.id));
    assert!(payload_1.entries.iter().any(|e| e.event_id == note_2.id));

    let preview = OutboundService::new(&db).prepare(&cp_1.id, &[]).unwrap();
    let model = FixedModel(json!({
        "schema_version": "1.0",
        "goal": "Verify discard and multi-interval boundaries",
        "checkpoint_summary": "Combined first and second observations.",
        "verified_facts": [{
            "id": "fact_1",
            "claim": "Both observations were captured.",
            "source_event_ids": [note_1.id.0, note_2.id.0]
        }],
        "hypotheses": [],
        "failed_attempts": [],
        "unresolved_questions": [],
        "relevant_artifacts": [],
        "next_action": {
            "instruction": "Verify second interval after resume",
            "justification": "Ensure intervals do not overlap.",
            "success_check": "Second checkpoint contains only post-resume events.",
            "source_event_ids": [note_2.id.0]
        },
        "limitations": []
    }));
    let generated = generate_draft(
        &db,
        &cp_1.id,
        &[],
        &preview.payload_hash,
        Redactor::builtin(),
        &model,
        "nvidia/nemotron-3-super-120b-a12b",
    )
    .unwrap();
    let approved = ApprovalService::new(&db)
        .approve(&cp_1.id, generated.draft.unwrap())
        .unwrap();
    assert_eq!(approved.thread.state, ThreadState::Ready);

    // Resume thread -> add note_3 -> save cp_2 -> verify note_1 and note_2 are NOT in cp_2
    let resumed = ThreadService::new(&db).resume(&thread.id).unwrap();
    assert_eq!(resumed.state, ThreadState::Resumed);

    let note_3 = ThreadService::new(&db)
        .add_note(
            &thread.id,
            AddNoteInput {
                text: "Third observation in resumed interval.".into(),
                label: NoteLabel::Fact,
            },
        )
        .unwrap();
    let cp_2 = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();
    let payload_2: CheckpointPayload = serde_json::from_value(cp_2.payload).unwrap();
    assert!(payload_2.entries.iter().any(|e| e.event_id == note_3.id));
    assert!(!payload_2.entries.iter().any(|e| e.event_id == note_1.id));
    assert!(!payload_2.entries.iter().any(|e| e.event_id == note_2.id));
}

#[test]
fn raw_event_retention_expires_old_unreferenced_events_while_protecting_checkpoint_sources() {
    let db = Database::open_in_memory().unwrap();
    let repo = init_repo();
    let project = approve_project(
        &db,
        CreateProjectInput {
            display_name: "RetentionRepo".into(),
            root_path: repo.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    let mut policy = db.find_project_policy(&project.id).unwrap();
    policy.retention_days = 7;
    db.upsert_project_policy(&policy).unwrap();

    let thread = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id: project.id.clone(),
            title: "Retention thread".into(),
            goal: "Expire old unreferenced events".into(),
            hypothesis: None,
        })
        .unwrap();

    let old_time = Utc::now() - Duration::days(30);
    let old_cited = ThreadEvent {
        id: EventId("evt-old-cited".into()),
        thread_id: thread.id.clone(),
        timestamp: old_time,
        payload: EventPayload::NoteAdded {
            text: "Old note cited by a saved checkpoint".into(),
            label: NoteLabel::Fact,
        },
        sensitivity: Sensitivity::Normal,
        content_hash: "hash-old-2".into(),
    };
    let old_unreferenced = ThreadEvent {
        id: EventId("evt-old-unreferenced".into()),
        thread_id: thread.id.clone(),
        timestamp: old_time + Duration::seconds(1),
        payload: EventPayload::NoteAdded {
            text: "Unreferenced old note".into(),
            label: NoteLabel::Question,
        },
        sensitivity: Sensitivity::Normal,
        content_hash: "hash-old-1".into(),
    };
    db.insert_event(&old_cited).unwrap();
    db.insert_event(&old_unreferenced).unwrap();

    let cp = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();
    let preview = OutboundService::new(&db).prepare(&cp.id, &[]).unwrap();
    let model = FixedModel(json!({
        "schema_version": "1.0",
        "goal": "Expire old unreferenced events",
        "checkpoint_summary": "Cites evt-old-cited.",
        "verified_facts": [{
            "id": "fact_1",
            "claim": "Old note is cited.",
            "source_event_ids": ["evt-old-cited"]
        }],
        "hypotheses": [],
        "failed_attempts": [],
        "unresolved_questions": [],
        "relevant_artifacts": [],
        "next_action": {
            "instruction": "Verify retention",
            "justification": "Keep cited events.",
            "success_check": "Uncited old event removed.",
            "source_event_ids": ["evt-old-cited"]
        },
        "limitations": []
    }));
    let generated = generate_draft(
        &db,
        &cp.id,
        &[],
        &preview.payload_hash,
        Redactor::builtin(),
        &model,
        "nvidia/nemotron-3-super-120b-a12b",
    )
    .unwrap();
    ApprovalService::new(&db)
        .approve(&cp.id, generated.draft.unwrap())
        .unwrap();

    let report = db.expire_raw_events(Utc::now()).unwrap();
    assert_eq!(report.raw_events_expired, 1);

    let remaining = db.list_all_events(&thread.id).unwrap();
    assert!(!remaining.iter().any(|e| e.id.0 == "evt-old-unreferenced"));
    assert!(remaining.iter().any(|e| e.id.0 == "evt-old-cited"));
}

#[test]
fn open_artifact_truncates_large_files_redacts_secrets_and_handles_missing_files_on_moved_projects()
{
    let db = Database::open_in_memory().unwrap();
    let repo = init_repo();
    // Write a >32 KiB file with a fake secret inside
    let large_content = format!(
        "// header with secret sk_live_threadback_demo_99f18a4c2d8e4b60b5c7a1d2e3f40506\n{}",
        "pub const LINE: &str = \"padding\";\n".repeat(1_500)
    );
    fs::write(repo.path().join("src/retry.rs"), &large_content).unwrap();

    let project = approve_project(
        &db,
        CreateProjectInput {
            display_name: "ArtifactsRepo".into(),
            root_path: repo.path().to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    let thread = ThreadService::new(&db)
        .start(StartThreadInput {
            project_id: project.id.clone(),
            title: "Artifact preview test".into(),
            goal: "Verify artifact preview bounding, redaction, and moved-project handling".into(),
            hypothesis: None,
        })
        .unwrap();
    let note = ThreadService::new(&db)
        .add_note(
            &thread.id,
            AddNoteInput {
                text: "Inspected src/retry.rs.".into(),
                label: NoteLabel::Fact,
            },
        )
        .unwrap();
    let cp = CheckpointService::new(&db)
        .save(&thread.id, CheckpointTrigger::Manual)
        .unwrap();

    // Unapproved checkpoint cannot open artifacts
    let unapproved_err = CheckpointService::new(&db)
        .open_artifact(&cp.id, "src/retry.rs")
        .unwrap_err();
    assert_eq!(unapproved_err.code, ErrorCode::InvalidState);

    let preview = OutboundService::new(&db).prepare(&cp.id, &[]).unwrap();
    let model = FixedModel(json!({
        "schema_version": "1.0",
        "goal": "Verify artifact preview bounding, redaction, and moved-project handling",
        "checkpoint_summary": "Inspected src/retry.rs.",
        "verified_facts": [{
            "id": "fact_1",
            "claim": "Inspected src/retry.rs.",
            "source_event_ids": [note.id.0]
        }],
        "hypotheses": [],
        "failed_attempts": [],
        "unresolved_questions": [],
        "relevant_artifacts": [{
            "type": "file",
            "relative_path": "src/retry.rs",
            "reason": "Main retry module.",
            "source_event_ids": [note.id.0]
        }],
        "next_action": {
            "instruction": "Open src/retry.rs",
            "justification": "Review retry count.",
            "success_check": "File opens cleanly.",
            "source_event_ids": [note.id.0]
        },
        "limitations": []
    }));
    let generated = generate_draft(
        &db,
        &cp.id,
        &[],
        &preview.payload_hash,
        Redactor::builtin(),
        &model,
        "nvidia/nemotron-3-super-120b-a12b",
    )
    .unwrap();
    ApprovalService::new(&db)
        .approve(&cp.id, generated.draft.unwrap())
        .unwrap();

    // 1. Opening existing large file truncates at 32 KiB and redacts the secret
    let opened = CheckpointService::new(&db)
        .open_artifact(&cp.id, "src/retry.rs")
        .unwrap();
    assert!(opened.exists);
    assert!(opened.truncated);
    let preview_text = opened.content_preview.unwrap();
    assert!(!preview_text.contains("sk_live_threadback_demo_99f18a4c2d8e4b60b5c7a1d2e3f40506"));
    assert!(preview_text.contains("[REDACTED:"));

    // 2. Move the repository folder and verify graceful missing artifact + RepositoryUnavailable drift
    let moved_holder = TempDir::new().unwrap();
    let moved_path = moved_holder.path().join("moved-repo");
    fs::rename(repo.path(), &moved_path).unwrap();

    let missing = CheckpointService::new(&db)
        .open_artifact(&cp.id, "src/retry.rs")
        .unwrap();
    assert!(!missing.exists);
    assert_eq!(missing.relative_path, "src/retry.rs");

    let resume_moved = CheckpointService::new(&db).get_resume(&cp.id).unwrap();
    assert!(
        resume_moved
            .drift
            .signals
            .iter()
            .any(|s| s.kind == DriftKind::RepositoryUnavailable)
    );

    // 3. Relink project to moved_path and verify artifact opens again
    relink_project(
        &db,
        RelinkProjectInput {
            project_id: project.id,
            root_path: moved_path.to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    let reopened = CheckpointService::new(&db)
        .open_artifact(&cp.id, "src/retry.rs")
        .unwrap();
    assert!(reopened.exists);
}
