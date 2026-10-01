use std::{fs, sync::Mutex};

use chrono::Utc;
use tauri::{App, Manager, State};
use threadback_core::{AppStatus, CreateProjectInput, Database, DomainError, Project, ProjectId};

pub struct AppState {
    database: Mutex<Database>,
}

pub fn initialize(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let database = Database::open(data_dir.join("threadback.db"))?;
    app.manage(AppState {
        database: Mutex::new(database),
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
pub fn projects_create(
    input: CreateProjectInput,
    state: State<'_, AppState>,
) -> Result<Project, DomainError> {
    input.validate()?;
    let canonical_root = std::fs::canonicalize(&input.root_path).map_err(|_| {
        DomainError::invalid_input(
            "The selected project folder does not exist or cannot be opened.",
            "rootPath",
        )
    })?;
    if !canonical_root.is_dir() {
        return Err(DomainError::invalid_input(
            "The selected project root must be a folder.",
            "rootPath",
        ));
    }

    let now = Utc::now();
    let project = Project {
        id: ProjectId::new(),
        display_name: input.display_name.trim().into(),
        canonical_root: canonical_root.to_string_lossy().into_owned(),
        created_at: now,
        updated_at: now,
    };
    database(&state)?.insert_project(&project)?;
    Ok(project)
}

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>) -> Result<Vec<Project>, DomainError> {
    database(&state)?.list_projects()
}

fn database<'a, 'r>(
    state: &'a State<'r, AppState>,
) -> Result<std::sync::MutexGuard<'a, Database>, DomainError> {
    state
        .database
        .lock()
        .map_err(|_| DomainError::internal("The local database lock is unavailable."))
}
