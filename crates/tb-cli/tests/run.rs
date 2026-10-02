use std::{fs, path::Path, process::Command};

use chrono::Utc;
use tempfile::TempDir;
use threadback_core::{
    Database, EventPayload, Project, ProjectId, StartThreadInput, ThreadId, ThreadService,
};

fn tb(database: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tb"))
        .args(args)
        .current_dir(cwd)
        .env("THREADBACK_DB", database)
        .output()
        .unwrap()
}

/// A project folder plus a database with one recording thread in it.
fn recording_project() -> (TempDir, TempDir, ThreadId) {
    let project = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let db = Database::open(data.path().join("threadback.db")).unwrap();
    let now = Utc::now();
    let record = Project {
        id: ProjectId::new(),
        display_name: "Payments".into(),
        canonical_root: fs::canonicalize(project.path())
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
            title: "t".into(),
            goal: "g".into(),
            hypothesis: None,
        })
        .unwrap()
        .id;
    (project, data, thread_id)
}

fn finished_events(database: &Path, thread_id: &ThreadId) -> Vec<serde_json::Value> {
    Database::open(database)
        .unwrap()
        .list_events(thread_id, None, 100)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event.payload {
            EventPayload::CommandFinished(value) => Some(value),
            _ => None,
        })
        .collect()
}

#[test]
fn records_a_command_and_forwards_its_exit_code_and_output() {
    let (project, data, thread_id) = recording_project();
    let database = data.path().join("threadback.db");

    let output = tb(
        &database,
        project.path(),
        &["run", "--", "sh", "-c", "echo out; echo err >&2; exit 7"],
    );
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "out\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "err\n");

    let finished = finished_events(&database, &thread_id);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["exitCode"], 7);
    assert_eq!(finished[0]["stdout"]["head"], "out\n");
}

#[test]
fn the_double_dash_is_optional_and_arguments_stay_literal() {
    let (project, data, _) = recording_project();
    let database = data.path().join("threadback.db");
    let output = tb(
        &database,
        project.path(),
        &["run", "echo", "--", "$HOME", "*"],
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "-- $HOME *\n");
}

#[test]
fn runs_but_does_not_record_when_no_database_exists_and_never_creates_one() {
    let project = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let database = data.path().join("threadback.db");

    let output = tb(
        &database,
        project.path(),
        &["run", "--", "sh", "-c", "echo hi; exit 3"],
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hi\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not recorded"));
    assert!(!database.exists(), "tb must not create the database");
}

#[test]
fn a_command_outside_the_project_runs_unrecorded() {
    let (_project, data, thread_id) = recording_project();
    let database = data.path().join("threadback.db");
    let elsewhere = TempDir::new().unwrap();

    let output = tb(
        &database,
        elsewhere.path(),
        &["run", "--", "sh", "-c", "exit 9"],
    );
    assert_eq!(output.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not recorded"));
    assert!(finished_events(&database, &thread_id).is_empty());
}

#[test]
fn a_missing_program_exits_127() {
    let (project, data, thread_id) = recording_project();
    let database = data.path().join("threadback.db");
    let output = tb(
        &database,
        project.path(),
        &["run", "--", "no-such-program-here"],
    );
    assert_eq!(output.status.code(), Some(127));
    assert_eq!(finished_events(&database, &thread_id)[0]["exitCode"], 127);
}

#[test]
fn run_without_a_command_is_a_usage_error() {
    let (project, data, _) = recording_project();
    let database = data.path().join("threadback.db");
    for args in [&["run"][..], &["run", "--"][..]] {
        let output = tb(&database, project.path(), args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("needs a command"));
    }
}

#[test]
fn version_and_help_work() {
    let (project, data, _) = recording_project();
    let database = data.path().join("threadback.db");
    let version = tb(&database, project.path(), &["--version"]);
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("tb "));
    let help = tb(&database, project.path(), &[]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("tb run -- <command>"));
}

/// Ctrl-C goes to the whole foreground process group. The child dies, but the
/// wrapper must stay alive long enough to record how the command ended.
#[cfg(unix)]
#[test]
fn interrupting_the_group_still_records_the_finish() {
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    let (project, data, thread_id) = recording_project();
    let database = data.path().join("threadback.db");

    let mut child = Command::new(env!("CARGO_BIN_EXE_tb"))
        .args(["run", "--", "sh", "-c", "sleep 30"])
        .current_dir(project.path())
        .env("THREADBACK_DB", &database)
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id() as i32;

    // Wait until the start event is stored, which happens before the child runs.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events = Database::open(&database)
            .unwrap()
            .list_events(&thread_id, None, 100)
            .unwrap()
            .events;
        if events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::CommandStarted(_)))
        {
            break;
        }
        assert!(Instant::now() < deadline, "command never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(300));

    // SAFETY: sending a signal to a process group we created.
    unsafe { libc::kill(-group, libc::SIGINT) };
    let status = child.wait().unwrap();

    assert_eq!(status.code(), Some(130), "the wrapper reports 128 + SIGINT");
    let finished = finished_events(&database, &thread_id);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["signal"], 2);
    assert!(finished[0]["exitCode"].is_null());
}
