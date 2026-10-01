use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DomainError, ProjectBoundary};

const MAX_LISTED_PATHS: usize = 200;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPath {
    pub path: String,
    /// Porcelain index column, e.g. `M`, `A`, `D`, `?`.
    pub index_status: String,
    /// Porcelain worktree column.
    pub worktree_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffStat {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiffStats {
    pub files: Vec<FileDiffStat>,
    pub file_count: u32,
    pub additions: u32,
    pub deletions: u32,
}

/// Payload of a `git.snapshot` event. Holds paths and counts only, never file
/// contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    /// `None` when the repository has no commits yet.
    pub head: Option<String>,
    /// True when Git reports any change, including excluded paths.
    pub dirty: bool,
    pub changed_paths: Vec<ChangedPath>,
    pub staged: DiffStats,
    pub unstaged: DiffStats,
    /// Changed paths withheld because they are excluded or resolve outside the
    /// project.
    pub excluded_changes: u32,
    /// True when a path list was cut at the listing limit.
    pub truncated: bool,
}

/// The only Git invocations this module can run. Each is read-only, and none
/// takes caller-supplied arguments, so file names and branch names are never
/// interpreted as options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitCommand {
    ShowPrefix,
    Branch,
    Head,
    Status,
    DiffUnstaged,
    DiffStaged,
}

impl GitCommand {
    #[cfg(test)]
    const ALL: [GitCommand; 6] = [
        Self::ShowPrefix,
        Self::Branch,
        Self::Head,
        Self::Status,
        Self::DiffUnstaged,
        Self::DiffStaged,
    ];

    /// Arguments after the fixed global options.
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::ShowPrefix => &["rev-parse", "--show-prefix"],
            Self::Branch => &["symbolic-ref", "--short", "-q", "HEAD"],
            Self::Head => &["rev-parse", "--verify", "-q", "HEAD"],
            Self::Status => &[
                "status",
                "--porcelain=v1",
                "-z",
                "--no-renames",
                "--untracked-files=normal",
                "--",
                ".",
            ],
            Self::DiffUnstaged => &[
                "diff",
                "--numstat",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--relative",
                "--",
                ".",
            ],
            Self::DiffStaged => &[
                "diff",
                "--cached",
                "--numstat",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--relative",
                "--",
                ".",
            ],
        }
    }
}

/// Options placed before every subcommand. `--no-optional-locks` stops
/// `status` from refreshing the index, and `core.fsmonitor=false` stops a
/// repository's own config from launching a program.
const GLOBAL_ARGS: &[&str] = &[
    "--no-pager",
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
];

struct GitOutput {
    code: Option<i32>,
    stdout: Vec<u8>,
}

/// Captures the repository state for the project behind `boundary`.
pub fn capture_snapshot(boundary: &ProjectBoundary) -> Result<GitSnapshot, DomainError> {
    let root = boundary.root();

    let prefix = run(root, GitCommand::ShowPrefix)?;
    if prefix.code != Some(0) {
        return Err(DomainError::invalid_state(
            "The project folder is not inside a Git repository.",
        ));
    }
    let prefix = text(&prefix.stdout).trim_end().to_owned();

    let branch = optional_line(run(root, GitCommand::Branch)?)?;
    let head = optional_line(run(root, GitCommand::Head)?)?;

    let status = expect_success(run(root, GitCommand::Status)?)?;
    let entries = parse_status(&status.stdout, &prefix);
    let dirty = !entries.is_empty();

    let mut excluded = 0u32;
    let mut truncated = false;
    let mut changed_paths = Vec::new();
    for entry in entries {
        if boundary.resolve(&entry.path).is_err() {
            excluded += 1;
        } else if changed_paths.len() < MAX_LISTED_PATHS {
            changed_paths.push(entry);
        } else {
            truncated = true;
        }
    }

    let unstaged = diff_stats(
        boundary,
        GitCommand::DiffUnstaged,
        &mut excluded,
        &mut truncated,
    )?;
    let staged = diff_stats(
        boundary,
        GitCommand::DiffStaged,
        &mut excluded,
        &mut truncated,
    )?;

    Ok(GitSnapshot {
        branch,
        head,
        dirty,
        changed_paths,
        staged,
        unstaged,
        excluded_changes: excluded,
        truncated,
    })
}

fn diff_stats(
    boundary: &ProjectBoundary,
    command: GitCommand,
    excluded: &mut u32,
    truncated: &mut bool,
) -> Result<DiffStats, DomainError> {
    let output = expect_success(run(boundary.root(), command)?)?;
    let mut stats = DiffStats::default();
    for file in parse_numstat(&output.stdout) {
        if boundary.resolve(&file.path).is_err() {
            *excluded += 1;
            continue;
        }
        stats.file_count += 1;
        stats.additions = stats.additions.saturating_add(file.additions);
        stats.deletions = stats.deletions.saturating_add(file.deletions);
        if stats.files.len() < MAX_LISTED_PATHS {
            stats.files.push(file);
        } else {
            *truncated = true;
        }
    }
    Ok(stats)
}

/// Parses `status --porcelain=v1 -z --no-renames`: `XY <path>\0` records.
/// Paths are relative to the repository root, so `prefix` (the project's
/// location inside it) is stripped.
fn parse_status(raw: &[u8], prefix: &str) -> Vec<ChangedPath> {
    raw.split(|byte| *byte == 0)
        .filter(|record| record.len() > 3 && record[2] == b' ')
        .filter_map(|record| {
            let path = text(&record[3..]);
            let path = path.strip_prefix(prefix)?;
            Some(ChangedPath {
                path: path.to_owned(),
                index_status: char::from(record[0]).to_string(),
                worktree_status: char::from(record[1]).to_string(),
            })
        })
        .collect()
}

/// Parses `diff --numstat -z --no-renames`: `<added>\t<deleted>\t<path>\0`,
/// with `-` counts for binary files.
fn parse_numstat(raw: &[u8]) -> Vec<FileDiffStat> {
    raw.split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let record = text(record);
            let mut parts = record.splitn(3, '\t');
            let added = parts.next()?;
            let deleted = parts.next()?;
            let path = parts.next()?;
            let binary = added == "-" || deleted == "-";
            Some(FileDiffStat {
                path: path.to_owned(),
                additions: added.parse().unwrap_or(0),
                deletions: deleted.parse().unwrap_or(0),
                binary,
            })
        })
        .collect()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Exit 0 yields the first line; exit 1 means "not present" (detached HEAD,
/// unborn branch); anything else is a failure.
fn optional_line(output: GitOutput) -> Result<Option<String>, DomainError> {
    match output.code {
        Some(0) => Ok(Some(text(&output.stdout).trim().to_owned()).filter(|line| !line.is_empty())),
        Some(1) => Ok(None),
        _ => Err(git_failure()),
    }
}

fn expect_success(output: GitOutput) -> Result<GitOutput, DomainError> {
    if output.code == Some(0) {
        Ok(output)
    } else {
        Err(git_failure())
    }
}

fn git_failure() -> DomainError {
    DomainError::invalid_state("Git could not read the repository state.")
}

fn run(root: &Path, command: GitCommand) -> Result<GitOutput, DomainError> {
    // The environment is rebuilt from scratch so inherited GIT_DIR,
    // GIT_WORK_TREE, or GIT_INDEX_FILE cannot point Git outside the project.
    let mut process = Command::new("git");
    process
        .args(GLOBAL_ARGS)
        .args(command.args())
        .current_dir(root)
        .env_clear()
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for key in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(key) {
            process.env(key, value);
        }
    }

    let mut child = process.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DomainError::invalid_state("Git is not installed or is not on the PATH.")
        } else {
            DomainError::internal("Git could not be started.")
        }
    })?;

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = thread::spawn(move || read_capped(&mut stdout));

    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(DomainError::invalid_state("Git took too long to respond."));
            }
            Err(_) => return Err(DomainError::internal("Git could not be awaited.")),
        }
    };

    let (stdout, overflowed) = reader
        .join()
        .map_err(|_| DomainError::internal("Git output could not be read."))?
        .map_err(|_| DomainError::internal("Git output could not be read."))?;
    if overflowed {
        return Err(DomainError::invalid_state(
            "The repository has too many changes to summarize.",
        ));
    }
    Ok(GitOutput {
        code: status.code(),
        stdout,
    })
}

/// Reads to the end but keeps only `MAX_OUTPUT_BYTES`, so a runaway process
/// can neither exhaust memory nor block on a full pipe.
fn read_capped(source: &mut impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut overflowed = false;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            return Ok((kept, overflowed));
        }
        let room = MAX_OUTPUT_BYTES.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..read.min(room)]);
        overflowed |= read > room;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;
    use std::{fs, path::PathBuf, process::Command as Std};
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let status = Std::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    fn snapshot(dir: &Path) -> GitSnapshot {
        capture_snapshot(&ProjectBoundary::new(dir, &[]).unwrap()).unwrap()
    }

    fn change_for<'a>(snapshot: &'a GitSnapshot, path: &str) -> &'a ChangedPath {
        snapshot
            .changed_paths
            .iter()
            .find(|change| change.path == path)
            .unwrap_or_else(|| panic!("{path} not listed in {:?}", snapshot.changed_paths))
    }

    #[test]
    fn clean_repository_reports_branch_and_head() {
        let dir = repo();
        let snapshot = snapshot(dir.path());
        assert_eq!(snapshot.branch.as_deref(), Some("main"));
        assert_eq!(snapshot.head.as_deref().map(str::len), Some(40));
        assert!(!snapshot.dirty);
        assert!(snapshot.changed_paths.is_empty());
        assert_eq!(snapshot.staged, DiffStats::default());
        assert_eq!(snapshot.unstaged, DiffStats::default());
    }

    #[test]
    fn reports_staged_unstaged_and_untracked_changes() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        fs::write(dir.path().join("b.txt"), "changed\n").unwrap();
        git(dir.path(), &["add", "b.txt"]);
        fs::write(dir.path().join("new.txt"), "new\n").unwrap();

        let snapshot = snapshot(dir.path());
        assert!(snapshot.dirty);
        assert_eq!(change_for(&snapshot, "a.txt").worktree_status, "M");
        assert_eq!(change_for(&snapshot, "a.txt").index_status, " ");
        assert_eq!(change_for(&snapshot, "b.txt").index_status, "M");
        assert_eq!(change_for(&snapshot, "new.txt").index_status, "?");

        assert_eq!(snapshot.unstaged.file_count, 1);
        assert_eq!(snapshot.unstaged.files[0].path, "a.txt");
        assert_eq!(snapshot.unstaged.additions, 2);
        assert_eq!(snapshot.unstaged.deletions, 0);
        assert_eq!(snapshot.staged.files[0].path, "b.txt");
        assert_eq!(snapshot.staged.additions, 1);
        assert_eq!(snapshot.staged.deletions, 1);
    }

    #[test]
    fn detached_head_has_no_branch() {
        let dir = repo();
        git(dir.path(), &["checkout", "-q", "--detach"]);
        let snapshot = snapshot(dir.path());
        assert_eq!(snapshot.branch, None);
        assert!(snapshot.head.is_some());
    }

    #[test]
    fn repository_without_commits_has_branch_but_no_head() {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        fs::write(dir.path().join("a.txt"), "x\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        let snapshot = snapshot(dir.path());
        assert_eq!(snapshot.branch.as_deref(), Some("main"));
        assert_eq!(snapshot.head, None);
        assert_eq!(snapshot.staged.additions, 1);
    }

    #[test]
    fn folder_outside_any_repository_is_rejected() {
        let dir = TempDir::new().unwrap();
        let boundary = ProjectBoundary::new(dir.path(), &[]).unwrap();
        let error = capture_snapshot(&boundary).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(error.message.contains("not inside a Git repository"));
    }

    #[test]
    fn excluded_changes_are_counted_but_not_listed() {
        let dir = repo();
        fs::write(dir.path().join(".env"), "TOKEN=abc\n").unwrap();
        fs::write(dir.path().join("key.pem"), "x\n").unwrap();
        fs::write(dir.path().join("logo.png"), [0u8, 1, 2]).unwrap();
        git(dir.path(), &["add", "-f", "logo.png"]);

        let snapshot = snapshot(dir.path());
        assert!(snapshot.dirty);
        assert_eq!(snapshot.excluded_changes, 4);
        assert!(snapshot.changed_paths.is_empty());
        assert_eq!(snapshot.staged.file_count, 0);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains(".env"));
        assert!(!serialized.contains("key.pem"));
    }

    #[test]
    fn binary_changes_are_flagged() {
        let dir = repo();
        fs::write(dir.path().join("data.dat"), [0u8, 159, 146, 150]).unwrap();
        git(dir.path(), &["add", "data.dat"]);
        let snapshot = snapshot(dir.path());
        assert!(snapshot.staged.files[0].binary);
        assert_eq!(snapshot.staged.additions, 0);
    }

    #[cfg(unix)]
    #[test]
    fn tracked_symlink_pointing_outside_is_withheld() {
        let dir = repo();
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        let snapshot = snapshot(dir.path());
        assert!(snapshot.dirty);
        assert!(snapshot.changed_paths.is_empty());
        assert_eq!(snapshot.excluded_changes, 1);
    }

    #[test]
    fn project_in_repository_subfolder_sees_only_its_own_paths() {
        let dir = repo();
        fs::create_dir_all(dir.path().join("services/payments")).unwrap();
        fs::write(dir.path().join("services/payments/lib.rs"), "x\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "add service"]);
        fs::write(dir.path().join("services/payments/lib.rs"), "x\ny\n").unwrap();
        fs::write(dir.path().join("a.txt"), "outside the project\n").unwrap();

        let snapshot = snapshot(&dir.path().join("services/payments"));
        assert_eq!(snapshot.changed_paths.len(), 1);
        assert_eq!(snapshot.changed_paths[0].path, "lib.rs");
        assert_eq!(snapshot.unstaged.files[0].path, "lib.rs");
        assert_eq!(snapshot.unstaged.file_count, 1);
    }

    #[test]
    fn shell_metacharacters_in_names_stay_literal() {
        let dir = repo();
        let name = "$(touch pwned);`touch pwned2`.txt";
        fs::write(dir.path().join(name), "x\n").unwrap();
        let snapshot = snapshot(dir.path());
        assert_eq!(change_for(&snapshot, name).index_status, "?");
        assert!(!dir.path().join("pwned").exists());
        assert!(!dir.path().join("pwned2").exists());
    }

    #[test]
    fn untracked_directories_are_listed_once() {
        let dir = repo();
        fs::create_dir_all(dir.path().join("scratch/nested")).unwrap();
        fs::write(dir.path().join("scratch/nested/a.txt"), "x").unwrap();
        let snapshot = snapshot(dir.path());
        assert_eq!(change_for(&snapshot, "scratch/").index_status, "?");
    }

    #[test]
    fn listing_is_bounded() {
        let dir = repo();
        for index in 0..(MAX_LISTED_PATHS + 5) {
            fs::write(dir.path().join(format!("f{index:03}.txt")), "x").unwrap();
        }
        let snapshot = snapshot(dir.path());
        assert_eq!(snapshot.changed_paths.len(), MAX_LISTED_PATHS);
        assert!(snapshot.truncated);
    }

    #[test]
    fn capture_does_not_modify_the_repository() {
        let dir = repo();
        fs::write(dir.path().join("a.txt"), "edited\n").unwrap();
        fs::write(dir.path().join("untracked.txt"), "x\n").unwrap();
        // Make the index stat data stale, which an ordinary `git status`
        // would refresh by rewriting `.git/index`.
        let before = fingerprint(&dir.path().join(".git"));
        let _ = snapshot(dir.path());
        let _ = snapshot(dir.path());
        assert_eq!(before, fingerprint(&dir.path().join(".git")));
    }

    fn fingerprint(git_dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        let mut pending = vec![git_dir.to_path_buf()];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(&path).unwrap() {
                let entry = entry.unwrap().path();
                if entry.is_dir() {
                    pending.push(entry);
                } else {
                    files.push((entry.clone(), fs::read(&entry).unwrap()));
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn only_read_only_subcommands_are_reachable() {
        let allowed = ["rev-parse", "symbolic-ref", "status", "diff"];
        for command in GitCommand::ALL {
            let args = command.args();
            assert!(allowed.contains(&args[0]), "{command:?} runs {}", args[0]);
            for forbidden in ["-d", "--delete", "--set", "--unset", "-m", "--stat-write"] {
                assert!(
                    !args.contains(&forbidden),
                    "{command:?} contains {forbidden}"
                );
            }
            if args[0] == "symbolic-ref" {
                // Two positional arguments would write a ref.
                let positionals = args[1..].iter().filter(|arg| !arg.starts_with('-')).count();
                assert_eq!(positionals, 1);
            }
        }
        assert!(GLOBAL_ARGS.contains(&"--no-optional-locks"));
        assert!(GLOBAL_ARGS.contains(&"core.fsmonitor=false"));
    }

    #[test]
    fn repository_config_cannot_launch_programs() {
        let dir = repo();
        let marker = dir.path().join("../fsmonitor-ran");
        let hook = dir.path().join("hook.sh");
        fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        git(
            dir.path(),
            &["config", "core.fsmonitor", hook.to_str().unwrap()],
        );
        let _ = snapshot(dir.path());
        assert!(!marker.exists());
    }

    #[test]
    fn status_parser_handles_all_record_shapes() {
        let raw = b" M src/a.rs\0A  b.rs\0?? c d.txt\0UU conflict.rs\0";
        let parsed = parse_status(raw, "");
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[2].path, "c d.txt");
        assert_eq!(parsed[3].index_status, "U");
        assert_eq!(
            parse_status(b" M sub/x.rs\0 M other/y.rs\0", "sub/")[0].path,
            "x.rs"
        );
        assert_eq!(
            parse_status(b" M sub/x.rs\0 M other/y.rs\0", "sub/").len(),
            1
        );
    }

    #[test]
    fn numstat_parser_handles_binary_and_tabs_in_names() {
        let parsed = parse_numstat(b"3\t1\ta.rs\0-\t-\timg.dat\00\t2\twith\ttab.txt\0");
        assert_eq!(parsed.len(), 3);
        assert_eq!((parsed[0].additions, parsed[0].deletions), (3, 1));
        assert!(parsed[1].binary);
        assert_eq!(parsed[2].path, "with\ttab.txt");
    }
}
