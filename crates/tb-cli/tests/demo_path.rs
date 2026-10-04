//! The seeded demonstration, end to end: reset the fixture, run commands
//! through the real `tb` binary, add notes, save a checkpoint, and let the
//! repository drift. Needs `node` and `npm`, as the fixture does.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Instant,
};

use tempfile::TempDir;
use threadback_core::{
    AddNoteInput, CheckpointPayload, CheckpointService, CheckpointTrigger, CreateProjectInput,
    Database, EventPayload, GitSnapshot, NoteLabel, ProjectBoundary, StartThreadInput, ThreadId,
    ThreadService, ThreadState, approve_project, capture_snapshot,
};

const FAKE_SECRET: &str = "nbf_demo_7F4A91C2_DO_NOT_SEND";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn node(args: &[&str]) -> Output {
    let output = Command::new("node")
        .args(args)
        .current_dir(repository_root())
        .output()
        .expect("`node` is required to run the demo fixture");
    assert!(
        output.status.success(),
        "node {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn tb(database: &Path, workspace: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tb"));
    command
        .args(args)
        .current_dir(workspace)
        .env("THREADBACK_DB", database);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

struct Demo {
    _dir: TempDir,
    workspace: PathBuf,
    database: PathBuf,
    thread_id: ThreadId,
}

impl Demo {
    fn start() -> Self {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path().join("payments");
        node(&["scripts/reset-demo.mjs", workspace.to_str().unwrap()]);
        let database = dir.path().join("threadback.db");

        let db = Database::open(&database).unwrap();
        let project = approve_project(
            &db,
            CreateProjectInput {
                display_name: "Payments".into(),
                root_path: workspace.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let thread_id = ThreadService::new(&db)
            .start_with_git_snapshot(StartThreadInput {
                project_id: project.id,
                title: "Duplicate capture investigation".into(),
                goal: "Find why retries produce a second provider capture".into(),
                hypothesis: None,
            })
            .unwrap()
            .id;
        Self {
            _dir: dir,
            workspace,
            database,
            thread_id,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.database).unwrap()
    }

    fn step(&self, name: &str) {
        node(&[
            "scripts/demo-step.mjs",
            name,
            self.workspace.to_str().unwrap(),
        ]);
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut full = vec!["run", "--"];
        full.extend_from_slice(args);
        tb(&self.database, &self.workspace, &full, env)
    }

    fn finished(&self) -> Vec<serde_json::Value> {
        self.db()
            .list_all_events(&self.thread_id)
            .unwrap()
            .into_iter()
            .filter_map(|event| match event.payload {
                EventPayload::CommandFinished(value) => Some(value),
                _ => None,
            })
            .collect()
    }
}

fn head(workspace: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn the_reset_command_restores_the_same_repository_every_time() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("w");
    node(&["scripts/reset-demo.mjs", workspace.to_str().unwrap()]);
    let first = head(&workspace);
    std::fs::write(workspace.join("scratch.txt"), "dirty").unwrap();
    node(&[
        "scripts/demo-step.mjs",
        "drift",
        workspace.to_str().unwrap(),
    ]);
    node(&["scripts/reset-demo.mjs", workspace.to_str().unwrap()]);
    assert_eq!(
        head(&workspace),
        first,
        "the seeded commit is deterministic"
    );
    assert!(!workspace.join("scratch.txt").exists());
    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(status.stdout.is_empty(), "the reset leaves a clean tree");
}

#[test]
fn the_demo_path_from_failing_test_to_drift() {
    let demo = Demo::start();

    // 3. Run the focused test: it fails with two provider calls, and the
    //    terminal output contains the planted fake credential.
    let first = demo.run(&["npm", "run", "--silent", "test:retry"], &[]);
    assert_eq!(first.status.code(), Some(1));
    let terminal = String::from_utf8_lossy(&first.stdout);
    assert!(terminal.contains("2 !== 1"), "{terminal}");
    assert!(
        terminal.contains(FAKE_SECRET),
        "the fixture must emit its fake secret"
    );

    // 4. Change the retry middleware and rerun: the same failure.
    demo.step("attempt-fix");
    let second = demo.run(&["npm", "run", "--silent", "test:retry"], &[]);
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stdout).contains("2 !== 1"));

    // 5. Record the hypothesis and a fact.
    let db = demo.db();
    let service = ThreadService::new(&db);
    for (text, label) in [
        (
            "The focused retry test made two provider calls.",
            NoteLabel::Fact,
        ),
        (
            "A second worker bypasses the distributed lock.",
            NoteLabel::Hypothesis,
        ),
        (
            "Do both workers share the same lock backend?",
            NoteLabel::Question,
        ),
    ] {
        service
            .add_note(
                &demo.thread_id,
                AddNoteInput {
                    text: text.into(),
                    label,
                },
            )
            .unwrap();
    }

    // 6. Save My Place, within two seconds, on the real demo data.
    let started = Instant::now();
    let checkpoint = CheckpointService::new(&db)
        .save(&demo.thread_id, CheckpointTrigger::Manual)
        .unwrap();
    let elapsed = started.elapsed();
    eprintln!("Save My Place on the demo fixture took {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
    assert_eq!(
        db.find_thread(&demo.thread_id).unwrap().unwrap().state,
        ThreadState::CheckpointDraft
    );

    let payload: CheckpointPayload = serde_json::from_value(checkpoint.payload.clone()).unwrap();
    assert!(payload.final_snapshot.captured);
    let failures = payload
        .entries
        .iter()
        .filter(|e| e.kind == "command.finished" && e.content["exitCode"] == 1)
        .count();
    assert_eq!(failures, 2, "both failed runs are kept");
    assert!(
        payload
            .entries
            .iter()
            .any(|e| e.kind == "note.added" && e.content["label"] == "hypothesis")
    );
    let last = payload.entries.last().unwrap();
    assert_eq!(last.kind, "git.snapshot");
    assert_eq!(last.content["branch"], "main");
    assert_eq!(
        last.content["changedPaths"][0]["path"],
        "src/retry-middleware.mjs"
    );
    assert_eq!(last.content["unstaged"]["additions"], 1);
    let final_snapshot: GitSnapshot = serde_json::from_value(last.content.clone()).unwrap();

    // 10. While the user is away the repository changes: branch, HEAD, status.
    demo.step("drift");
    let boundary = ProjectBoundary::new(&demo.workspace, &[]).unwrap();
    let now = capture_snapshot(&boundary).unwrap();
    assert_ne!(now.branch, final_snapshot.branch, "branch drift");
    assert_ne!(now.head, final_snapshot.head, "HEAD drift");
    assert_ne!(
        now.changed_paths, final_snapshot.changed_paths,
        "status drift"
    );
    assert_eq!(now.branch.as_deref(), Some("hotfix/review-feedback"));
    let paths: Vec<_> = now
        .changed_paths
        .iter()
        .map(|change| change.path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["src/lock.mjs", "src/retry-middleware.mjs"],
        "a new path appears"
    );
}

#[test]
fn the_lock_owner_check_shows_the_workers_use_different_backends() {
    let demo = Demo::start();
    let output = demo.run(
        &["npm", "run", "--silent", "test:retry"],
        &[("LOCK_DEBUG", "1")],
    );
    let terminal = String::from_utf8_lossy(&output.stdout);
    assert!(
        terminal.contains("worker=worker-a backend=lock-backend-1"),
        "{terminal}"
    );
    assert!(
        terminal.contains("worker=worker-b backend=lock-backend-2"),
        "{terminal}"
    );

    let lock = demo.run(&["npm", "run", "--silent", "test:lock"], &[]);
    assert_eq!(lock.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&lock.stdout).contains("worker-b sees owner worker-b"));
}

#[test]
fn a_reverted_attempt_leaves_a_clean_tree_but_keeps_its_evidence() {
    let demo = Demo::start();
    demo.step("attempt-fix");
    demo.run(&["npm", "run", "--silent", "test:retry"], &[]);
    demo.step("revert-fix");

    let boundary = ProjectBoundary::new(&demo.workspace, &[]).unwrap();
    assert!(
        !capture_snapshot(&boundary).unwrap().dirty,
        "the code is back to the commit"
    );
    assert_eq!(
        demo.finished().len(),
        1,
        "the failed attempt is still recorded"
    );
}

#[test]
fn large_output_is_truncated_but_the_failure_at_the_end_survives() {
    let demo = Demo::start();
    let output = demo.run(&["npm", "run", "--silent", "test:noisy"], &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.len() > 1_000_000,
        "the terminal gets everything"
    );

    let finished = &demo.finished()[0];
    assert_eq!(finished["exitCode"], 1);
    let stdout = &finished["stdout"];
    assert_eq!(stdout["truncated"], true);
    assert!(stdout["originalBytes"].as_u64().unwrap() > 1_000_000);
    assert!(stdout["omittedBytes"].as_u64().unwrap() > 900_000);
    assert!(
        stdout["tail"]
            .as_str()
            .unwrap()
            .contains("retry trace 20000")
    );
    assert!(
        finished["stderr"]["head"]
            .as_str()
            .unwrap()
            .contains("FAIL noisy run")
    );
}
