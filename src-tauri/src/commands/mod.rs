use std::{fs, sync::Mutex};

use tauri::{App, Manager, State};
use threadback_core::{
    AddNoteInput, AppStatus, CreateProjectInput, Database, DomainError, EventPage, PauseReason,
    Project, RecoveryReport, StartThreadInput, ThreadEvent, ThreadId, ThreadService, WorkThread,
    approve_project, default_exclusions, recover_interrupted_threads,
};

const TIMELINE_PAGE_SIZE: u32 = 100;

pub struct AppState {
    database: Mutex<Database>,
    recovery: Vec<RecoveryReport>,
}

pub fn initialize(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let database = Database::open(data_dir.join("threadback.db"))?;
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

#[tauri::command]
pub fn threads_end(
    thread_id: ThreadId,
    state: State<'_, AppState>,
) -> Result<WorkThread, DomainError> {
    ThreadService::new(&*database(&state)?).end(&thread_id)
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
