//! M-02 and G3: from an approved payload, through the real client and a local
//! server, to a stored, validated draft.

mod common;

use std::{fs, path::PathBuf, thread, time::Duration};

use chrono::Utc;
use common::{FakeServer, Scripted};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, ApiKey, CancelToken, CaptureLimits, Checkpoint, CheckpointService,
    CheckpointTrigger, CommandRequest, ConfigurationService, Database, DomainError, ErrorCode,
    EventId, EventPayload, MemorySecretStore, ModelCallRecord, ModelConfigInput, ModelSettings,
    NoteLabel, OutboundService, PROMPT_VERSION, Project, ProjectId, Redactor, ResumeCapsule, RunIo,
    STATUS_DRAFT, STATUS_GENERATED, StartThreadInput, StructuredOutput, ThreadId, ThreadService,
    ThreadState, TokenFactoryClient, generate_draft, plan_generation,
};

const KEY: &str = "tfk-7F4A91C2-demo-secret-key-value-0001";
const EXCLUDED_TEXT: &str = "EXCLUDED-NOTE-the-coffee-is-cold";

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
    let excluded_note = add(EXCLUDED_TEXT, NoteLabel::Fact);

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
    let excluded = vec![excluded_note];
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
    /// A capsule that cites only what was approved.
    fn capsule(&self) -> Value {
        let spec: Value = serde_json::from_str(
            &fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/evaluation/capsules/valid/spec-example.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let mut capsule = spec;
        let command = &self.command_event.0;
        capsule["verified_facts"][0]["source_event_ids"] = json!([self.fact_note.0, command]);
        capsule["hypotheses"][0]["source_event_ids"] = json!([self.hypothesis_note.0]);
        capsule["failed_attempts"][0]["source_event_ids"] = json!([command]);
        capsule["relevant_artifacts"][0]["source_event_ids"] = json!([command]);
        capsule["next_action"]["source_event_ids"] = json!([command, self.hypothesis_note.0]);
        capsule
    }

    fn client(&self, server: &FakeServer) -> TokenFactoryClient {
        TokenFactoryClient::new(
            settings(&server.endpoint),
            ApiKey::new(KEY),
            CancelToken::new(),
        )
    }

    fn generate(&self, client: &TokenFactoryClient) -> Result<Checkpoint, DomainError> {
        generate_draft(
            &self.db,
            &self.checkpoint.id,
            &self.excluded,
            &self.hash,
            Redactor::builtin(),
            client,
            "nvidia/test-model",
        )
    }

    fn stored(&self) -> Checkpoint {
        self.db
            .find_checkpoint(&self.checkpoint.id)
            .unwrap()
            .unwrap()
    }

    fn calls(&self) -> Vec<(String, u8)> {
        let connection = Connection::open(&self.path).unwrap();
        let mut statement = connection
            .prepare("SELECT status, attempt FROM model_calls ORDER BY at, rowid")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

fn settings(endpoint: &str) -> ModelSettings {
    ModelSettings {
        endpoint: endpoint.into(),
        model_id: "nvidia/test-model".into(),
        timeout_secs: 5,
        structured_output: StructuredOutput::JsonSchema,
    }
}

#[test]
fn a_valid_reply_becomes_a_stored_draft_with_its_provenance() {
    let f = flow();
    let server = FakeServer::start(vec![Scripted::ok(&f.capsule().to_string())]);
    let generated = f.generate(&f.client(&server)).unwrap();

    assert_eq!(generated.status, STATUS_GENERATED);
    assert_eq!(generated.model_id.as_deref(), Some("nvidia/test-model"));
    assert_eq!(generated.prompt_version.as_deref(), Some(PROMPT_VERSION));
    assert!(generated.approved.is_none());
    let draft: ResumeCapsule = generated.draft.clone().unwrap();
    assert_eq!(
        serde_json::to_value(&draft).unwrap(),
        f.capsule(),
        "stored exactly as the model returned it"
    );
    assert_eq!(f.stored().draft, generated.draft);
    assert_eq!(
        f.db.find_thread(&f.thread_id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );

    // The thread's history says it was generated, with metadata and no content.
    let events = f.db.list_all_events(&f.thread_id).unwrap();
    let EventPayload::CheckpointGenerated(meta) = &events.last().unwrap().payload else {
        panic!("last event")
    };
    assert_eq!(meta["modelId"], "nvidia/test-model");
    assert_eq!(meta["promptVersion"], PROMPT_VERSION);
    assert_eq!(meta["payloadHash"], f.hash);
    assert_eq!(
        (meta["attempts"].as_u64(), meta["repaired"].as_bool()),
        (Some(1), Some(false))
    );
    assert_eq!(
        (
            meta["promptTokens"].as_u64(),
            meta["completionTokens"].as_u64()
        ),
        (Some(120), Some(45))
    );
    let metadata = meta.to_string();
    assert!(
        !metadata.contains("provider call") && !metadata.contains(KEY),
        "{metadata}"
    );

    // The request carried the reviewed payload and nothing that was excluded.
    let sent = &server.requests()[0];
    let body = sent.json();
    let prompt = body["messages"][1]["content"].as_str().unwrap();
    assert!(prompt.contains("The focused retry test made two provider calls."));
    assert!(
        !sent.body.contains(EXCLUDED_TEXT),
        "an excluded entry must not be sent"
    );
    assert!(
        !sent.body.contains(KEY),
        "the key travels only in the header"
    );
    assert_eq!(sent.headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(f.calls(), [("ok".to_owned(), 1)]);
}

#[test]
fn the_draft_is_written_once_and_a_second_generation_is_refused_before_any_request() {
    let f = flow();
    let server = FakeServer::start(vec![
        Scripted::ok(&f.capsule().to_string()),
        Scripted::ok(&f.capsule().to_string()),
    ]);
    let client = f.client(&server);
    let first = f.generate(&client).unwrap();

    let error = f.generate(&client).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert!(error.message.contains("already generated"));
    assert_eq!(server.requests().len(), 1, "no second request");
    assert_eq!(f.stored().draft, first.draft, "the original is untouched");

    // The storage layer refuses an overwrite too, whoever asks.
    let capsule: ResumeCapsule = serde_json::from_value(f.capsule()).unwrap();
    let error =
        f.db.set_checkpoint_draft(&f.checkpoint.id, &capsule, "other", "v2")
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidState);
    assert_eq!(f.stored().model_id.as_deref(), Some("nvidia/test-model"));
}

#[test]
fn an_invalid_first_reply_is_corrected_once_and_both_calls_are_recorded() {
    let f = flow();
    let mut bad = f.capsule();
    bad["verified_facts"][0]["source_event_ids"] = json!(["evt_made_up"]);
    let server = FakeServer::start(vec![
        Scripted::ok(&bad.to_string()),
        Scripted::ok(&f.capsule().to_string()),
    ]);
    let client = f.client(&server);
    let generated = f.generate(&client).unwrap();

    assert_eq!(generated.status, STATUS_GENERATED);
    assert_eq!(server.requests().len(), 2);
    let repair = server.requests()[1].json();
    let messages = repair["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[2]["role"], "assistant");
    assert!(
        messages[3]["content"]
            .as_str()
            .unwrap()
            .contains("/verified_facts/0/source_event_ids/0")
    );
    assert_eq!(f.calls(), [("ok".to_owned(), 1), ("ok".to_owned(), 2)]);

    let events = f.db.list_all_events(&f.thread_id).unwrap();
    let EventPayload::CheckpointGenerated(meta) = &events.last().unwrap().payload else {
        panic!()
    };
    assert_eq!(
        (meta["attempts"].as_u64(), meta["repaired"].as_bool()),
        (Some(2), Some(true))
    );
}

#[test]
fn two_invalid_replies_are_a_visible_error_with_no_draft_and_a_retry_still_works() {
    let f = flow();
    let mut bad = f.capsule();
    bad["verified_facts"][0]["source_event_ids"] = json!([f.hypothesis_note.0]);
    let server = FakeServer::start(vec![
        Scripted::ok(&bad.to_string()),
        Scripted::ok(&bad.to_string()),
        Scripted::ok(&f.capsule().to_string()),
    ]);
    let client = f.client(&server);

    let error = f.generate(&client).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidModelOutput);
    assert!(error.retryable && error.message.contains("even after one correction"));
    assert_eq!(
        server.requests().len(),
        2,
        "one reply and one correction, never more"
    );
    let stored = f.stored();
    assert_eq!(
        (stored.status.as_str(), stored.draft.is_none()),
        (STATUS_DRAFT, true),
        "no unsupported fact was stored"
    );
    assert!(
        f.db.list_all_events(&f.thread_id)
            .unwrap()
            .iter()
            .all(|e| !matches!(e.payload, EventPayload::CheckpointGenerated(_)))
    );
    assert_eq!(f.calls().len(), 2, "the failed calls are still recorded");

    // Trying again from the same draft works.
    assert_eq!(f.generate(&client).unwrap().status, STATUS_GENERATED);
}

#[test]
fn a_service_failure_keeps_the_draft_and_is_recorded() {
    let f = flow();
    let server = FakeServer::start(vec![Scripted::error(401, "bad key")]);
    let error = f.generate(&f.client(&server)).unwrap_err();
    assert_eq!(error.code, ErrorCode::ModelAuthentication);
    assert!(!error.message.contains(KEY));
    assert_eq!(f.stored().status, STATUS_DRAFT);
    assert_eq!(f.calls(), [("authentication".to_owned(), 1)]);
    assert_eq!(
        f.db.find_thread(&f.thread_id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );
}

#[test]
fn cancelling_a_generation_in_flight_leaves_the_draft_untouched() {
    let f = flow();
    let server = FakeServer::start(vec![
        Scripted::ok(&f.capsule().to_string()).delayed(Duration::from_secs(5)),
    ]);
    let cancel = CancelToken::new();
    let client =
        TokenFactoryClient::new(settings(&server.endpoint), ApiKey::new(KEY), cancel.clone());
    let trigger = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        trigger.cancel();
    });
    let error = f.generate(&client).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(f.stored().status, STATUS_DRAFT);
    assert_eq!(f.calls(), [("cancelled".to_owned(), 1)]);
}

#[test]
fn a_payload_that_is_not_approved_never_reaches_the_service() {
    let f = flow();
    let server = FakeServer::start(vec![Scripted::ok(&f.capsule().to_string())]);
    let client = f.client(&server);
    let stale = generate_draft(
        &f.db,
        &f.checkpoint.id,
        &f.excluded,
        "stale",
        Redactor::builtin(),
        &client,
        "m",
    )
    .unwrap_err();
    assert_eq!(stale.code, ErrorCode::PayloadChanged);
    let everything = generate_draft(
        &f.db,
        &f.checkpoint.id,
        &[],
        &f.hash,
        Redactor::builtin(),
        &client,
        "m",
    )
    .unwrap_err();
    assert_eq!(
        everything.code,
        ErrorCode::PayloadChanged,
        "a different selection than the one reviewed"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(server.requests().is_empty());
    assert!(f.calls().is_empty());
}

#[test]
fn nothing_the_database_holds_about_calls_contains_evidence_replies_or_the_key() {
    let f = flow();
    let server = FakeServer::start(vec![Scripted::ok(&f.capsule().to_string())]);
    f.generate(&f.client(&server)).unwrap();

    let connection = Connection::open(&f.path).unwrap();
    let mut statement = connection.prepare("SELECT * FROM model_calls").unwrap();
    let columns = statement.column_count();
    let rows: Vec<String> = statement
        .query_map([], |row| {
            let mut cells = Vec::new();
            for index in 0..columns {
                cells.push(match row.get::<_, rusqlite::types::Value>(index)? {
                    rusqlite::types::Value::Text(text) => text,
                    rusqlite::types::Value::Integer(number) => number.to_string(),
                    rusqlite::types::Value::Null => "NULL".to_owned(),
                    other => format!("{other:?}"),
                });
            }
            Ok(cells.join("|"))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows.len(), 1);
    for forbidden in [
        KEY,
        "7F4A91C2",
        "provider calls",
        "distributed lock",
        "Bearer",
        "SYS",
    ] {
        assert!(!rows[0].contains(forbidden), "{forbidden}: {}", rows[0]);
    }
    assert!(rows[0].contains("nvidia/test-model") && rows[0].contains("127.0.0.1"));
    drop(statement);
    drop(connection);

    let bytes: Vec<u8> = ["", "-wal"]
        .iter()
        .filter_map(|s| fs::read(format!("{}{s}", f.path.display())).ok())
        .flatten()
        .collect();
    assert!(
        !String::from_utf8_lossy(&bytes).contains(KEY),
        "the key is nowhere in the database files"
    );
}

#[test]
fn latency_percentiles_are_summarised_from_successful_calls() {
    let f = flow();
    assert_eq!(f.db.model_latency_summary().unwrap().count, 0);
    let record = |latency: u64, status: &str| ModelCallRecord {
        model_id: "m".into(),
        endpoint_host: "h".into(),
        status: status.into(),
        http_status: None,
        latency_ms: latency,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        finish_reason: None,
        structured_output: StructuredOutput::Off,
        attempt: 1,
    };
    for latency in (1..=20).map(|n| n * 10) {
        f.db.record_model_call(None, &record(latency, "ok"))
            .unwrap();
    }
    f.db.record_model_call(None, &record(99_999, "timeout"))
        .unwrap();
    let summary = f.db.model_latency_summary().unwrap();
    assert_eq!(
        (summary.count, summary.p50_ms, summary.p95_ms),
        (20, Some(100), Some(190)),
        "failures are not counted"
    );
}

#[test]
fn a_missing_key_is_reported_before_any_request_and_after_the_payload_checks() {
    let f = flow();
    let secrets = MemorySecretStore::new();
    let service = ConfigurationService::new(&f.db, &secrets);

    // A payload that fails review is refused first, key or no key.
    let planned = plan_generation(
        &f.db,
        &f.checkpoint.id,
        &f.excluded,
        "stale",
        Redactor::builtin(),
    );
    assert_eq!(planned.err().unwrap().code, ErrorCode::PayloadChanged);

    // A good payload with no key is "not configured".
    plan_generation(
        &f.db,
        &f.checkpoint.id,
        &f.excluded,
        &f.hash,
        Redactor::builtin(),
    )
    .unwrap();
    let error = service.build_client(CancelToken::new()).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotConfigured);
    assert!(error.message.contains("Nothing was sent"));

    // With a key stored, a client is built from the saved settings.
    service
        .store(ModelConfigInput {
            endpoint: "http://127.0.0.1:9/v1".into(),
            model_id: "nvidia/test-model".into(),
            timeout_secs: 20,
            structured_output: StructuredOutput::Off,
            api_key: Some(KEY.into()),
        })
        .unwrap();
    let client = service.build_client(CancelToken::new()).unwrap();
    assert_eq!(client.structured_output_in_use(), StructuredOutput::Off);
    assert!(!format!("{client:?}").contains("7F4A91C2"));
}
