use std::{
    fs,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use tauri::{App, Manager, State};
use threadback_core::{
    AddNoteInput, AppStatus, ApprovalResult, ApprovalService, CancelToken, CapsuleModel,
    Checkpoint, CheckpointId, CheckpointService, CheckpointTrigger, ConfigurationService,
    ConfigurationStatus, ConnectionTestResult, Correction, CreateProjectInput, Database,
    DeletionReceipt, DomainError, EventId, EventPage, ExportService, FileResult, ModelConfigInput,
    OpenedArtifact, OutboundService, PauseReason, PayloadPreview, Project, ProjectId,
    ProjectPolicy, RecoveryReport, RedactionTestInput, RedactionTestResult, Redactor,
    RelinkProjectInput, ResumeCapsule, ResumeViewModel, RetentionReport, SecretStore,
    StartThreadInput, ThreadEvent, ThreadId, ThreadService, TokenFactoryClient, WorkThread,
    approve_project, database_path, default_exclusions, default_secret_store, finish_generation,
    plan_generation, recover_interrupted_threads, relink_project, run_generation, test_redaction,
};

const TIMELINE_PAGE_SIZE: u32 = 100;

pub struct AppState {
    database: Arc<Mutex<Database>>,
    /// The macOS Keychain. The API key lives here and nowhere else.
    secrets: Arc<dyn SecretStore>,
    /// The generation in progress, so the user can cancel it.
    generation: Arc<Mutex<Option<CancelToken>>>,
    recovery: Vec<RecoveryReport>,
}

pub fn initialize(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // The same path the `tb` command uses, so both write to one database.
    let path = database_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let database = Database::open(path)?;
    let _ = database.expire_raw_events(Utc::now());
    // Threads still recording when the app last stopped are paused and reported.
    let recovery = recover_interrupted_threads(&database)?;
    app.manage(AppState {
        database: Arc::new(Mutex::new(database)),
        secrets: Arc::from(default_secret_store()),
        generation: Arc::new(Mutex::new(None)),
        recovery,
    });
    Ok(())
}

#[tauri::command]
pub fn app_status() -> AppStatus {
    AppStatus {
        service: "threadback".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        ready: true,
    }
}

#[tauri::command]
pub fn recovery_reports(state: State<'_, AppState>) -> Vec<RecoveryReport> {
    state.recovery.clone()
}

#[tauri::command]
pub fn projects_create(
    input: CreateProjectInput,
    state: State<'_, AppState>,
) -> Result<Project, DomainError> {
    approve_project(&*database(&state)?, input)
}

#[tauri::command]
pub fn projects_relink(
    input: RelinkProjectInput,
    state: State<'_, AppState>,
) -> Result<Project, DomainError> {
    relink_project(&*database(&state)?, input)
}

/// The exclusions applied to every project, shown during onboarding.
#[tauri::command]
pub fn projects_default_exclusions() -> Vec<String> {
    default_exclusions()
}

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>) -> Result<Vec<Project>, DomainError> {
    database(&state)?.list_projects()
}

#[tauri::command]
pub fn projects_get_policy(
    project_id: ProjectId,
    state: State<'_, AppState>,
) -> Result<ProjectPolicy, DomainError> {
    database(&state)?.find_project_policy(&project_id)
}

#[tauri::command]
pub fn projects_update_policy(
    policy: ProjectPolicy,
    state: State<'_, AppState>,
) -> Result<ProjectPolicy, DomainError> {
    let db = database(&state)?;
    let updated = db.upsert_project_policy(&policy)?;
    let _ = db.expire_raw_events(Utc::now());
    Ok(updated)
}

#[tauri::command]
pub fn threads_list(state: State<'_, AppState>) -> Result<Vec<WorkThread>, DomainError> {
    ThreadService::new(&*database(&state)?).list_threads()
}

/// Starts a thread and records its first Git snapshot in one step.
#[tauri::command]
pub fn threads_start(
    input: StartThreadInput,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).start_with_git_snapshot(input)
}

#[tauri::command]
pub fn threads_pause(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).pause(&thread_id, PauseReason::User)
}

#[tauri::command]
pub fn threads_pause_all(state: State<'_, AppState>) -> Result<Vec<WorkThread>, DomainError> {
    ThreadService::new(&*database(&state)?).pause_all()
}

#[tauri::command]
pub fn threads_resume(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).resume(&thread_id)
}

/// Ends the thread and saves its checkpoint draft, so a closed thread always
/// has something to review.
#[tauri::command]
pub fn threads_end(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    let database = database(&state)?;
    CheckpointService::new(&database).save(&thread_id, CheckpointTrigger::ThreadEnded)?;
    database
        .find_thread(&thread_id)?
        .ok_or_else(|| DomainError::not_found("That thread no longer exists."))
}

/// Save My Place: works offline and stores a local checkpoint draft.
#[tauri::command]
pub fn checkpoints_save_place(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<Checkpoint, DomainError> {
    CheckpointService::new(&*database(&state)?).save(&thread_id, CheckpointTrigger::Manual)
}

#[tauri::command]
pub fn checkpoints_list(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<Vec<Checkpoint>, DomainError> {
    CheckpointService::new(&*database(&state)?).list(&thread_id)
}

/// Exactly what would be sent for this checkpoint, with the entries the user
/// excluded left out. Nothing is sent by previewing.
#[tauri::command]
pub fn checkpoints_preview(
    checkpoint_id: CheckpointId,
    excluded_event_ids: Vec<EventId>,
    state: State<'_, AppState>,
) -> Result<PayloadPreview, DomainError> {
    let db = database(&state)?;
    let redactor = checkpoint_redactor(&db, &checkpoint_id)?;
    OutboundService::with_redactor(&db, &redactor).prepare(&checkpoint_id, &excluded_event_ids)
}

/// Asks the model for a capsule from the payload the user reviewed.
///
/// The request can take a minute, so this runs on a worker thread (a plain
/// command would run on the main thread and freeze the window) and does not
/// hold the database while it waits. The payload is authorized again here: a
/// blocked, empty, or changed payload is refused before anything is sent.
#[tauri::command]
pub async fn checkpoints_generate(
    checkpoint_id: CheckpointId,
    excluded_event_ids: Vec<EventId>,
    reviewed_hash: String,
    state: State<'_, AppState>,
) -> Result<Checkpoint, DomainError> {
    let (database, secrets, slot) = (
        state.database.clone(),
        state.secrets.clone(),
        state.generation.clone(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = CancelToken::new();
        let busy = || DomainError::internal("The local database lock is unavailable.");

        // Phase 1, with the database: authorize, build the prompt, prepare the checks.
        let (plan, client, model_id) = {
            let guard = database.lock().map_err(|_| busy())?;
            let redactor = checkpoint_redactor(&guard, &checkpoint_id)?;
            let plan = plan_generation(
                &guard,
                &checkpoint_id,
                &excluded_event_ids,
                &reviewed_hash,
                &redactor,
            )?;
            let configuration = ConfigurationService::new(&guard, &*secrets);
            let model_id = configuration.settings()?.model_id;
            let client = configuration.build_client(cancel.clone())?;
            (plan, client, model_id)
        };

        // Phase 2, without it: the network request.
        *slot.lock().map_err(|_| busy())? = Some(cancel);
        let outcome = run_generation(&client, &plan);
        let calls = client.take_diagnostics();
        *slot.lock().map_err(|_| busy())? = None;

        // Phase 3, with the database again: record the calls and store the draft.
        let guard = database.lock().map_err(|_| busy())?;
        finish_generation(&guard, &plan, &model_id, outcome, &calls)
    })
    .await
    .map_err(|_| DomainError::internal("The generation stopped unexpectedly."))?
}

/// Stops the generation in progress, if any. Nothing more is sent.
#[tauri::command]
pub fn checkpoints_cancel_generation(state: State<'_, AppState>) {
    if let Ok(mut slot) = state.generation.lock() {
        if let Some(token) = slot.take() {
            token.cancel();
        }
    }
}

#[tauri::command]
pub fn configuration_status(
    state: State<'_, AppState>,
) -> Result<ConfigurationStatus, DomainError> {
    ConfigurationService::new(&*database(&state)?, &*state.secrets).status()
}

/// Saves the model connection. The key, if one is given, goes straight to the
/// Keychain; nothing returned from here contains it.
#[tauri::command]
pub fn configuration_store(
    input: ModelConfigInput,
    state: State<'_, AppState>,
) -> Result<ConfigurationStatus, DomainError> {
    ConfigurationService::new(&*database(&state)?, &*state.secrets).store(input)
}

#[tauri::command]
pub fn configuration_remove_key(
    state: State<'_, AppState>,
) -> Result<ConfigurationStatus, DomainError> {
    ConfigurationService::new(&*database(&state)?, &*state.secrets).remove_api_key()
}

/// Checks the endpoint, key, and model with one tiny request that contains
/// none of the user's evidence. It saves nothing.
#[tauri::command]
pub async fn configuration_test(
    input: ModelConfigInput,
    state: State<'_, AppState>,
) -> Result<ConnectionTestResult, DomainError> {
    let (database, secrets) = (state.database.clone(), state.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let (settings, key) = {
            let guard = database
                .lock()
                .map_err(|_| DomainError::internal("The local database lock is unavailable."))?;
            ConfigurationService::new(&guard, &*secrets).resolve_for_test(&input)?
        };
        let Some(key) = key else {
            return Ok(ConnectionTestResult {
                ok: false,
                message: "Enter an API key first. Nothing was sent.".into(),
                latency_ms: None,
                model_id: settings.model_id,
            });
        };
        Ok(TokenFactoryClient::new(settings, key, CancelToken::new()).test_connection())
    })
    .await
    .map_err(|_| DomainError::internal("The connection test stopped unexpectedly."))?
}

/// Throws a checkpoint draft away and returns the thread to recording.
#[tauri::command]
pub fn checkpoints_discard(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    CheckpointService::new(&*database(&state)?).discard(&checkpoint_id)
}

/// Approves a capsule: the model's draft with whatever the user changed. The
/// server checks it again, records each change by field path, and keeps the
/// model's draft untouched next to the approved version.
#[tauri::command]
pub fn checkpoints_approve(
    checkpoint_id: CheckpointId,
    capsule: ResumeCapsule,
    state: State<'_, AppState>,
) -> Result<ApprovalResult, DomainError> {
    ApprovalService::new(&*database(&state)?).approve(&checkpoint_id, capsule)
}

/// The changes made when this checkpoint was approved, oldest first.
#[tauri::command]
pub fn checkpoints_corrections(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<Vec<Correction>, DomainError> {
    ApprovalService::new(&*database(&state)?).corrections(&checkpoint_id)
}

#[tauri::command]
pub fn checkpoints_get(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<Checkpoint, DomainError> {
    CheckpointService::new(&*database(&state)?).get(&checkpoint_id)
}

#[tauri::command]
pub fn checkpoints_get_resume(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<ResumeViewModel, DomainError> {
    CheckpointService::new(&*database(&state)?).get_resume(&checkpoint_id)
}

#[tauri::command]
pub fn checkpoints_open_artifact(
    checkpoint_id: CheckpointId,
    relative_path: String,
    state: State<'_, AppState>,
) -> Result<OpenedArtifact, DomainError> {
    CheckpointService::new(&*database(&state)?).open_artifact(&checkpoint_id, &relative_path)
}

#[tauri::command]
pub fn threads_archive(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).archive(&thread_id)
}

#[tauri::command]
pub fn threads_delete(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<DeletionReceipt, DomainError> {
    ThreadService::new(&*database(&state)?).delete(&thread_id)
}

#[tauri::command]
pub fn exports_json(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<FileResult, DomainError> {
    ExportService::new(&*database(&state)?).export_json(&checkpoint_id)
}

#[tauri::command]
pub fn exports_markdown(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<FileResult, DomainError> {
    ExportService::new(&*database(&state)?).export_markdown(&checkpoint_id)
}

#[tauri::command]
pub fn privacy_test_redaction(
    input: RedactionTestInput,
    state: State<'_, AppState>,
) -> Result<RedactionTestResult, DomainError> {
    test_redaction(&*database(&state)?, &input)
}

#[tauri::command]
pub fn privacy_expire_events(state: State<'_, AppState>) -> Result<RetentionReport, DomainError> {
    database(&state)?.expire_raw_events(Utc::now())
}

#[tauri::command]
pub fn events_list(
    thread_id: ThreadId,
    cursor: Option<String>,
    state: State<'_, AppState>,
) -> Result<EventPage, DomainError> {
    ThreadService::new(&*database(&state)?).timeline(
        &thread_id,
        cursor.as_deref(),
        TIMELINE_PAGE_SIZE,
    )
}

#[tauri::command]
pub fn events_add_note(
    thread_id: ThreadId,
    note: AddNoteInput,
    state: State<'_, AppState>,
) -> Result<ThreadEvent, DomainError> {
    ThreadService::new(&*database(&state)?).add_note(&thread_id, note)
}

#[tauri::command]
pub fn events_capture_git(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<ThreadEvent, DomainError> {
    ThreadService::new(&*database(&state)?).capture_git(&thread_id)
}

#[tauri::command]
pub fn popover_close(app: tauri::AppHandle) -> Result<(), DomainError> {
    crate::mark_popover_closed(&app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(None::<String>);
    }
    Ok(())
}

#[tauri::command]
pub fn popover_set_status(status: String, app: tauri::AppHandle) -> Result<(), DomainError> {
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = match status.as_str() {
            "working" => "ThreadBack: Recording active thread",
            "saved" => "ThreadBack: Checkpoint saved locally",
            "attention" => "ThreadBack: Action needed / Project changed",
            _ => "ThreadBack",
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
    Ok(())
}

#[tauri::command]
pub fn app_quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn checkpoint_redactor(
    db: &Database,
    checkpoint_id: &CheckpointId,
) -> Result<Redactor, DomainError> {
    if let Some(checkpoint) = db.find_checkpoint(checkpoint_id)? {
        if let Some(thread) = db.find_thread(&checkpoint.thread_id)? {
            if let Ok(policy) = db.find_project_policy(&thread.project_id) {
                return Redactor::new(&policy.redaction_patterns);
            }
        }
    }
    Redactor::new(&[])
}

fn database<'a, 'r>(
    state: &'a State<'r, AppState>,
) -> Result<std::sync::MutexGuard<'a, Database>, DomainError> {
    state
        .database
        .lock()
        .map_err(|_| DomainError::internal("The local database lock is unavailable."))
}
