use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use chrono::Utc;
use tempfile::TempDir;
use threadback_core::{
    CaptureLimits, CommandFinishedPayload, CommandRequest, CommandStartedPayload, Database,
    ErrorCode, EventPayload, PauseReason, Project, ProjectId, RunIo, RunOutcome, StartThreadInput,
    ThreadEvent, ThreadId, ThreadService, run_command,
};

struct Fixture {
    db: Database,
    project: TempDir,
    thread_id: ThreadId,
}

fn fixture() -> Fixture {
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join("services/payments")).unwrap();
    fs::create_dir_all(project.path().join("node_modules/pkg")).unwrap();
    let db = Database::open_in_memory().unwrap();
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
            title: "Duplicate capture".into(),
            goal: "Find the second provider call".into(),
            hypothesis: None,
        })
        .unwrap()
        .id;
    Fixture {
        db,
        project,
        thread_id,
    }
}

struct Run {
    outcome: RunOutcome,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    notices: Vec<String>,
}

fn run_in(db: Option<&Database>, cwd: &Path, program: &str, args: &[&str]) -> Run {
    run_with(db, cwd, program, args, CaptureLimits::default(), None)
}

fn run_with<'a>(
    db: Option<&'a Database>,
    cwd: &Path,
    program: &str,
    args: &[&str],
    limits: CaptureLimits,
    after_spawn: Option<Box<dyn FnOnce() + 'a>>,
) -> Run {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut notices = Vec::new();
    let outcome = {
        let mut notify = |message: &str| notices.push(message.to_owned());
        run_command(
            db,
            &CommandRequest {
                program: program.into(),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                cwd: cwd.to_path_buf(),
                limits,
            },
            RunIo {
                stdout: &mut stdout,
                stderr: &mut stderr,
                notify: &mut notify,
                after_spawn,
            },
        )
        .unwrap()
    };
    Run {
        outcome,
        stdout,
        stderr,
        notices,
    }
}

fn events(db: &Database, thread_id: &ThreadId) -> Vec<ThreadEvent> {
    db.list_events(thread_id, None, 200).unwrap().events
}

fn command_events(
    db: &Database,
    thread_id: &ThreadId,
) -> (Vec<CommandStartedPayload>, Vec<CommandFinishedPayload>) {
    let mut started = Vec::new();
    let mut finished = Vec::new();
    for event in events(db, thread_id) {
        match event.payload {
            EventPayload::CommandStarted(value) => {
                started.push(serde_json::from_value(value).unwrap())
            }
            EventPayload::CommandFinished(value) => {
                finished.push(serde_json::from_value(value).unwrap())
            }
            _ => {}
        }
    }
    (started, finished)
}

#[test]
fn records_start_and_finish_with_output_exit_code_and_relative_cwd() {
    let f = fixture();
    let cwd = f.project.path().join("services/payments");
    let run = run_in(
        Some(&f.db),
        &cwd,
        "sh",
        &["-c", "echo two provider calls; echo oops >&2; exit 1"],
    );

    assert_eq!(run.outcome.exit_code, 1);
    assert!(run.notices.is_empty());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "two provider calls\n");
    assert_eq!(String::from_utf8_lossy(&run.stderr), "oops\n");

    let (started, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].cwd, "services/payments");
    assert!(started[0].command.starts_with("sh -c "));
    assert_eq!(finished.len(), 1);
    let finished = &finished[0];
    assert_eq!(finished.exit_code, Some(1));
    assert_eq!(finished.signal, None);
    assert_eq!(finished.stdout.head, "two provider calls\n");
    assert_eq!(finished.stderr.head, "oops\n");
    assert!(!finished.stdout.truncated);

    // The finish event points back at the start event, and they are ordered.
    assert_eq!(
        Some(&finished.started_event_id),
        run.outcome.started_event_id.as_ref()
    );
    let all = events(&f.db, &f.thread_id);
    let position = |id| all.iter().position(|event| Some(&event.id) == id).unwrap();
    assert!(
        position(run.outcome.started_event_id.as_ref())
            < position(run.outcome.finished_event_id.as_ref())
    );
}

#[test]
fn project_root_is_recorded_as_a_dot() {
    let f = fixture();
    run_in(Some(&f.db), f.project.path(), "true", &[]);
    let (started, _) = command_events(&f.db, &f.thread_id);
    assert_eq!(started[0].cwd, ".");
}

#[test]
fn arguments_reach_the_child_literally() {
    let f = fixture();
    let nasty = "$(touch pwned); `touch pwned2` * | > out.txt";
    let run = run_in(
        Some(&f.db),
        f.project.path(),
        "echo",
        &[nasty, "a b", "--flag"],
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        format!("{nasty} a b --flag\n")
    );
    for leftover in ["pwned", "pwned2", "out.txt"] {
        assert!(
            !f.project.path().join(leftover).exists(),
            "{leftover} was created"
        );
    }
    let (started, _) = command_events(&f.db, &f.thread_id);
    assert!(
        started[0]
            .command
            .contains("'$(touch pwned); `touch pwned2` * | > out.txt'")
    );
}

#[test]
fn the_childs_exit_code_is_forwarded() {
    let f = fixture();
    for code in [0, 1, 3, 42, 255] {
        let run = run_in(
            Some(&f.db),
            f.project.path(),
            "sh",
            &["-c", &format!("exit {code}")],
        );
        assert_eq!(run.outcome.exit_code, code);
    }
}

#[cfg(unix)]
#[test]
fn a_killed_child_exits_128_plus_the_signal() {
    let f = fixture();
    let run = run_in(Some(&f.db), f.project.path(), "sh", &["-c", "kill -9 $$"]);
    assert_eq!(run.outcome.exit_code, 137);
    let (_, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(finished[0].signal, Some(9));
    assert_eq!(finished[0].exit_code, None);
}

#[test]
fn a_missing_program_exits_127_and_is_still_evidence() {
    let f = fixture();
    let run = run_in(
        Some(&f.db),
        f.project.path(),
        "definitely-not-a-real-program",
        &[],
    );
    assert_eq!(run.outcome.exit_code, 127);
    assert!(String::from_utf8_lossy(&run.stderr).contains("command not found"));
    let (started, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(started.len(), 1);
    assert_eq!(finished[0].exit_code, Some(127));
    assert!(finished[0].stderr.head.contains("command not found"));
}

#[test]
fn nothing_is_recorded_without_a_recording_thread() {
    let f = fixture();
    ThreadService::new(&f.db)
        .pause(&f.thread_id, PauseReason::User)
        .unwrap();
    let before = events(&f.db, &f.thread_id).len();

    let run = run_in(
        Some(&f.db),
        f.project.path(),
        "sh",
        &["-c", "echo still runs; exit 4"],
    );
    assert_eq!(run.outcome.exit_code, 4);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "still runs\n");
    assert!(run.outcome.started_event_id.is_none());
    assert_eq!(run.notices.len(), 1);
    assert!(run.notices[0].contains("not recorded"), "{:?}", run.notices);
    assert_eq!(events(&f.db, &f.thread_id).len(), before);
}

#[test]
fn nothing_is_recorded_without_a_database() {
    let dir = TempDir::new().unwrap();
    let run = run_in(None, dir.path(), "sh", &["-c", "echo hi; exit 5"]);
    assert_eq!(run.outcome.exit_code, 5);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi\n");
    assert!(run.notices[0].contains("not recorded"));
}

#[test]
fn directories_outside_the_project_are_not_recorded() {
    let f = fixture();
    let elsewhere = TempDir::new().unwrap();
    let before = events(&f.db, &f.thread_id).len();
    let run = run_in(Some(&f.db), elsewhere.path(), "sh", &["-c", "echo hi"]);
    assert_eq!(run.outcome.exit_code, 0);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi\n");
    assert!(run.notices[0].contains("not inside"), "{:?}", run.notices);
    assert_eq!(events(&f.db, &f.thread_id).len(), before);
}

#[test]
fn excluded_directories_inside_the_project_are_not_recorded() {
    let f = fixture();
    let before = events(&f.db, &f.thread_id).len();
    let run = run_in(
        Some(&f.db),
        &f.project.path().join("node_modules/pkg"),
        "true",
        &[],
    );
    assert!(run.outcome.started_event_id.is_none());
    assert!(run.notices[0].contains("not recorded"));
    assert_eq!(events(&f.db, &f.thread_id).len(), before);
}

#[cfg(unix)]
#[test]
fn a_symlink_cwd_that_escapes_the_project_is_not_recorded() {
    let f = fixture();
    let outside = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), f.project.path().join("escape")).unwrap();
    let before = events(&f.db, &f.thread_id).len();
    let run = run_in(Some(&f.db), &f.project.path().join("escape"), "pwd", &[]);
    assert!(run.outcome.started_event_id.is_none());
    assert_eq!(events(&f.db, &f.thread_id).len(), before);
}

#[test]
fn an_unusable_working_directory_is_rejected_before_anything_runs() {
    let f = fixture();
    let marker = f.project.path().join("ran");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut notify = |_: &str| {};
    let before = events(&f.db, &f.thread_id).len();

    for cwd in [
        f.project.path().join("missing"),
        f.project.path().join("services/../nope"),
    ] {
        let error = run_command(
            Some(&f.db),
            &CommandRequest {
                program: "touch".into(),
                args: vec![marker.to_string_lossy().into_owned()],
                cwd,
                limits: CaptureLimits::default(),
            },
            RunIo {
                stdout: &mut stdout,
                stderr: &mut stderr,
                notify: &mut notify,
                after_spawn: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(error.field.as_deref(), Some("cwd"));
    }

    let file = f.project.path().join("a-file");
    fs::write(&file, "x").unwrap();
    let error = run_command(
        Some(&f.db),
        &CommandRequest {
            program: "true".into(),
            args: vec![],
            cwd: file,
            limits: CaptureLimits::default(),
        },
        RunIo {
            stdout: &mut stdout,
            stderr: &mut stderr,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("cwd"));

    assert!(!marker.exists());
    assert_eq!(events(&f.db, &f.thread_id).len(), before);
}

#[test]
fn an_empty_command_is_rejected() {
    let f = fixture();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut notify = |_: &str| {};
    let error = run_command(
        Some(&f.db),
        &CommandRequest {
            program: "  ".into(),
            args: vec![],
            cwd: f.project.path().to_path_buf(),
            limits: CaptureLimits::default(),
        },
        RunIo {
            stdout: &mut stdout,
            stderr: &mut stderr,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("command"));
}

#[test]
fn large_output_is_bounded_in_storage_but_passes_through_unchanged() {
    let f = fixture();
    let run = run_in(
        Some(&f.db),
        f.project.path(),
        "sh",
        &[
            "-c",
            "head -c 2000000 /dev/zero | tr '\\0' x; echo ERROR-AT-THE-END >&2",
        ],
    );
    assert_eq!(
        run.stdout.len(),
        2_000_000,
        "the terminal still gets everything"
    );

    let (_, finished) = command_events(&f.db, &f.thread_id);
    let out = &finished[0].stdout;
    assert!(out.truncated);
    assert_eq!(out.original_bytes, 2_000_000);
    assert_eq!(
        out.head.len() + out.tail.len() + out.omitted_bytes as usize,
        2_000_000
    );
    assert_eq!(out.head.len(), 4 * 1024);
    assert_eq!(out.tail.len(), 8 * 1024);
    // Short output sits entirely in the head; nothing is lost or marked truncated.
    assert!(finished[0].stderr.head.contains("ERROR-AT-THE-END"));
    assert_eq!(finished[0].stderr.tail, "");
    assert!(!finished[0].stderr.truncated);

    // What reaches the database stays small whatever the output size.
    let stored: usize = events(&f.db, &f.thread_id)
        .iter()
        .map(|event| serde_json::to_string(event).unwrap().len())
        .sum();
    assert!(stored < 40 * 1024, "stored {stored} bytes");
}

#[test]
fn custom_limits_are_honoured() {
    let f = fixture();
    run_with(
        Some(&f.db),
        f.project.path(),
        "sh",
        &["-c", "printf 0123456789abcdefghij"],
        CaptureLimits {
            head_bytes: 3,
            tail_bytes: 4,
        },
        None,
    );
    let (_, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(finished[0].stdout.head, "012");
    assert_eq!(finished[0].stdout.tail, "ghij");
    assert_eq!(finished[0].stdout.omitted_bytes, 13);
}

#[test]
fn a_thread_paused_mid_command_records_the_start_but_not_the_finish() {
    let f = fixture();
    let service_db = &f.db;
    let thread_id = f.thread_id.clone();
    let run = run_with(
        Some(&f.db),
        f.project.path(),
        "sh",
        &["-c", "sleep 0.2; echo late"],
        CaptureLimits::default(),
        Some(Box::new(move || {
            ThreadService::new(service_db)
                .pause(&thread_id, PauseReason::User)
                .unwrap();
        })),
    );
    assert_eq!(run.outcome.exit_code, 0);
    assert!(run.outcome.started_event_id.is_some());
    assert!(run.outcome.finished_event_id.is_none());
    assert!(
        run.notices
            .iter()
            .any(|notice| notice.contains("result not recorded"))
    );
    let (started, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!((started.len(), finished.len()), (1, 0));
}

#[cfg(unix)]
#[test]
fn a_background_process_holding_the_pipe_does_not_hang_the_wrapper() {
    let f = fixture();
    let started = Instant::now();
    let run = run_in(
        Some(&f.db),
        f.project.path(),
        "sh",
        &["-c", "sleep 6 & echo launched"],
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "launched\n");
    assert!(run.outcome.finished_event_id.is_some());
}

#[test]
fn binary_output_is_not_stored() {
    let f = fixture();
    run_in(
        Some(&f.db),
        f.project.path(),
        "sh",
        &["-c", "printf 'ab\\0cd'"],
    );
    let (_, finished) = command_events(&f.db, &f.thread_id);
    assert!(finished[0].stdout.binary);
    assert_eq!(finished[0].stdout.head, "");
    assert_eq!(finished[0].stdout.original_bytes, 5);
}

#[test]
fn the_environment_is_never_recorded() {
    let f = fixture();
    // SAFETY: this is the only test that touches this variable.
    unsafe { std::env::set_var("THREADBACK_TEST_SECRET_ENV", "super-secret-value") };
    run_in(Some(&f.db), f.project.path(), "true", &[]);
    unsafe { std::env::remove_var("THREADBACK_TEST_SECRET_ENV") };
    let everything: String = events(&f.db, &f.thread_id)
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect();
    assert!(!everything.contains("super-secret-value"));
}

/// Discards what it is given but counts it, so a test can push gigabytes
/// through without holding them.
#[derive(Default)]
struct CountingSink {
    bytes: u64,
    delay_each_write: Option<Duration>,
}

impl std::io::Write for CountingSink {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if let Some(delay) = self.delay_each_write {
            std::thread::sleep(delay);
        }
        self.bytes += buffer.len() as u64;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn run_counting(
    f: &Fixture,
    script: &str,
    limits: CaptureLimits,
    stdout: &mut CountingSink,
    stderr: &mut CountingSink,
) -> RunOutcome {
    let mut notify = |message: &str| panic!("unexpected notice: {message}");
    run_command(
        Some(&f.db),
        &CommandRequest {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: f.project.path().to_path_buf(),
            limits,
        },
        RunIo {
            stdout,
            stderr,
            notify: &mut notify,
            after_spawn: None,
        },
    )
    .unwrap()
}

#[test]
fn huge_output_on_both_streams_keeps_storage_bounded_and_the_exit_status() {
    let f = fixture();
    let (mut out, mut err) = (CountingSink::default(), CountingSink::default());
    let started = Instant::now();
    let outcome = run_counting(
        &f,
        "head -c 100000000 /dev/zero | tr '\\0' o & \
         head -c 20000000 /dev/zero | tr '\\0' e >&2 & \
         wait; echo FINAL-STDOUT-LINE; echo FINAL-STDERR-LINE >&2; exit 3",
        CaptureLimits::default(),
        &mut out,
        &mut err,
    );
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "took {:?}",
        started.elapsed()
    );

    // The terminal received everything; only storage is bounded.
    assert_eq!(out.bytes, 100_000_000 + "FINAL-STDOUT-LINE\n".len() as u64);
    assert_eq!(err.bytes, 20_000_000 + "FINAL-STDERR-LINE\n".len() as u64);

    // The exit status is not lost.
    assert_eq!(outcome.exit_code, 3);
    let (_, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(finished[0].exit_code, Some(3));

    // Truncation is explicit, exact, and keeps the end of the output.
    for (excerpt, expected_total, last_line) in [
        (&finished[0].stdout, 100_000_000 + 18, "FINAL-STDOUT-LINE\n"),
        (&finished[0].stderr, 20_000_000 + 18, "FINAL-STDERR-LINE\n"),
    ] {
        assert!(excerpt.truncated);
        assert_eq!(excerpt.original_bytes, expected_total);
        assert_eq!(
            excerpt.head.len() as u64 + excerpt.tail.len() as u64 + excerpt.omitted_bytes,
            expected_total
        );
        assert!(excerpt.tail.ends_with(last_line));
    }

    // Stored evidence stays small however much was written.
    let stored: usize = events(&f.db, &f.thread_id)
        .iter()
        .map(|event| serde_json::to_string(event).unwrap().len())
        .sum();
    assert!(stored < 40 * 1024, "stored {stored} bytes");
}

#[test]
fn a_slow_consumer_loses_nothing_and_does_not_change_the_capture() {
    let f = fixture();
    let mut out = CountingSink {
        delay_each_write: Some(Duration::from_millis(1)),
        ..CountingSink::default()
    };
    let mut err = CountingSink::default();
    let outcome = run_counting(
        &f,
        "head -c 3000000 /dev/zero | tr '\\0' s; echo done >&2",
        CaptureLimits::default(),
        &mut out,
        &mut err,
    );
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(out.bytes, 3_000_000);
    let (_, finished) = command_events(&f.db, &f.thread_id);
    assert_eq!(finished[0].stdout.original_bytes, 3_000_000);
    assert_eq!(finished[0].stderr.head, "done\n");
}

#[test]
fn limits_above_the_ceiling_are_lowered_not_trusted() {
    let f = fixture();
    let (mut out, mut err) = (CountingSink::default(), CountingSink::default());
    run_counting(
        &f,
        "head -c 1000000 /dev/zero | tr '\\0' x",
        CaptureLimits {
            head_bytes: usize::MAX,
            tail_bytes: usize::MAX,
        },
        &mut out,
        &mut err,
    );
    let (_, finished) = command_events(&f.db, &f.thread_id);
    let stdout = &finished[0].stdout;
    assert_eq!(stdout.head.len(), threadback_core::MAX_CAPTURE_BYTES);
    assert_eq!(stdout.tail.len(), threadback_core::MAX_CAPTURE_BYTES);
    assert!(stdout.truncated);
}

#[test]
fn output_that_fits_is_stored_whole_and_not_marked_truncated() {
    let f = fixture();
    let (mut out, mut err) = (CountingSink::default(), CountingSink::default());
    // Exactly head + tail bytes: nothing has to be dropped.
    run_counting(
        &f,
        "head -c 12 /dev/zero | tr '\\0' z",
        CaptureLimits::new(4, 8).unwrap(),
        &mut out,
        &mut err,
    );
    let (_, finished) = command_events(&f.db, &f.thread_id);
    let stdout = &finished[0].stdout;
    assert_eq!(format!("{}{}", stdout.head, stdout.tail), "zzzzzzzzzzzz");
    assert!(!stdout.truncated);
    assert_eq!(stdout.omitted_bytes, 0);
}
