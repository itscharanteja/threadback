use std::{fs, sync::Mutex};

use tauri::{App, Manager, State};
use threadback_core::{
    AddNoteInput, AppStatus, Checkpoint, CheckpointId, CheckpointService, CheckpointTrigger,
    CreateProjectInput, Database, DomainError, EventId, EventPage, OutboundService, PauseReason,
    PayloadPreview, Project, RecoveryReport, StartThreadInput, ThreadEvent, ThreadId,
    ThreadService, WorkThread, approve_project, database_path, default_exclusions,
    recover_interrupted_threads,
};

const TIMELINE_PAGE_SIZE: u32 = 100;

pub struct AppState {
    database: Mutex<Database>,
    recovery: Vec<RecoveryReport>,
}

pub fn initialize(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // The same path the `tb` command uses, so both write to one database.
    let path = database_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let database = Database::open(path)?;
    // Threads still recording when the app last stopped are paused and reported.
    let recovery = recover_interrupted_threads(&database)?;
    app.manage(AppState {
        database: Mutex::new(database),
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
    OutboundService::new(&*database(&state)?).prepare(&checkpoint_id, &excluded_event_ids)
}

/// Asks for a capsule from the payload the user reviewed. The payload is
/// authorized again here, so a blocked or changed payload is refused.
#[tauri::command]
pub fn checkpoints_generate(
    checkpoint_id: CheckpointId,
    excluded_event_ids: Vec<EventId>,
    reviewed_hash: String,
    state: State<'_, AppState>,
) -> Result<(), DomainError> {
    OutboundService::new(&*database(&state)?).generate(
        &checkpoint_id,
        &excluded_event_ids,
        &reviewed_hash,
    )
}

/// Throws a checkpoint draft away and returns the thread to recording.
#[tauri::command]
pub fn checkpoints_discard(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    CheckpointService::new(&*database(&state)?).discard(&checkpoint_id)
}

#[tauri::command]
pub fn checkpoints_get(
    checkpoint_id: CheckpointId,
    state: State<'_, AppState>,
) -> Result<Checkpoint, DomainError> {
    CheckpointService::new(&*database(&state)?).get(&checkpoint_id)
}

#[tauri::command]
pub fn threads_archive(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).archive(&thread_id)
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

fn database<'a, 'r>(
    state: &'a State<'r, AppState>,
) -> Result<std::sync::MutexGuard<'a, Database>, DomainError> {
    state
        .database
        .lock()
        .map_err(|_| DomainError::internal("The local database lock is unavailable."))
}
