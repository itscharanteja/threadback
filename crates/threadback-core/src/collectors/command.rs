use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    Database, DomainError, EventId, EventPayload, ProjectBoundary, Sensitivity, ThreadId,
    ThreadService,
};

pub const DEFAULT_HEAD_BYTES: usize = 4 * 1024;
pub const DEFAULT_TAIL_BYTES: usize = 8 * 1024;
const READ_CHUNK_BYTES: usize = 16 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// How long to keep draining output after the child exits, in case a
/// background process it started still holds the pipe open.
const PIPE_GRACE: Duration = Duration::from_secs(1);

/// Payload of a `command.started` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandStartedPayload {
    /// Display form of the command line. Never includes the environment.
    pub command: String,
    /// Working directory relative to the project root.
    pub cwd: String,
}

/// Payload of a `command.finished` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandFinishedPayload {
    pub started_event_id: EventId,
    /// `None` when the process was ended by a signal.
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub stdout: OutputExcerpt,
    pub stderr: OutputExcerpt,
}

/// A bounded view of one output stream: the first and last bytes, plus how
/// much was left out so truncation is never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutputExcerpt {
    pub head: String,
    pub tail: String,
    #[ts(type = "number")]
    pub original_bytes: u64,
    #[ts(type = "number")]
    pub omitted_bytes: u64,
    pub truncated: bool,
    /// Binary output is counted but never stored.
    pub binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    pub head_bytes: usize,
    pub tail_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            head_bytes: DEFAULT_HEAD_BYTES,
            tail_bytes: DEFAULT_TAIL_BYTES,
        }
    }
}

/// Keeps the first `head_bytes` and the last `tail_bytes` of a stream in
/// constant memory, however much is written.
pub struct OutputCapture {
    limits: CaptureLimits,
    head: Vec<u8>,
    tail: Vec<u8>,
    total: u64,
    saw_nul: bool,
}

impl OutputCapture {
    pub fn new(limits: CaptureLimits) -> Self {
        Self {
            limits,
            head: Vec::new(),
            tail: Vec::new(),
            total: 0,
            saw_nul: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        self.saw_nul |= chunk.contains(&0);

        let room = self.limits.head_bytes.saturating_sub(self.head.len());
        let (to_head, rest) = chunk.split_at(room.min(chunk.len()));
        self.head.extend_from_slice(to_head);

        let limit = self.limits.tail_bytes;
        if rest.len() >= limit {
            self.tail.clear();
            self.tail.extend_from_slice(&rest[rest.len() - limit..]);
        } else {
            self.tail.extend_from_slice(rest);
            if self.tail.len() > limit {
                let excess = self.tail.len() - limit;
                self.tail.drain(..excess);
            }
        }
    }

    pub fn finish(self) -> OutputExcerpt {
        let kept = (self.head.len() + self.tail.len()) as u64;
        let omitted_bytes = self.total - kept;
        let truncated = omitted_bytes > 0;

        if self.saw_nul {
            return OutputExcerpt {
                head: String::new(),
                tail: String::new(),
                original_bytes: self.total,
                omitted_bytes: self.total,
                truncated: self.total > 0,
                binary: true,
            };
        }

        let mut head: &[u8] = &self.head;
        let mut tail: &[u8] = &self.tail;
        if truncated {
            // Do not show half of a multi-byte character at either cut.
            if let Err(error) = std::str::from_utf8(head) {
                if error.error_len().is_none() {
                    head = &head[..error.valid_up_to()];
                }
            }
            let continuation = tail.iter().take_while(|byte| *byte & 0xC0 == 0x80).count();
            tail = &tail[continuation.min(3)..];
        }
        let shown = (head.len() + tail.len()) as u64;
        OutputExcerpt {
            head: String::from_utf8_lossy(head).into_owned(),
            tail: String::from_utf8_lossy(tail).into_owned(),
            original_bytes: self.total,
            omitted_bytes: self.total - shown,
            truncated,
            binary: false,
        }
    }
}

/// Renders argv for display, quoting only what a shell would need quoted.
pub fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quote_for_display)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_for_display(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub limits: CaptureLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// What the wrapper should exit with: the child's code, or 128 plus the
    /// signal number when it was killed.
    pub exit_code: i32,
    pub thread_id: Option<ThreadId>,
    pub started_event_id: Option<EventId>,
    pub finished_event_id: Option<EventId>,
}

/// Where a command's output goes, and how the caller hears about problems.
pub struct RunIo<'a> {
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    /// Called for messages the user should see, such as "not recorded".
    pub notify: &'a mut dyn FnMut(&str),
    /// Called once, right after the child starts. The CLI uses it to ignore
    /// Ctrl-C so the finish is still recorded when the child is interrupted.
    pub after_spawn: Option<Box<dyn FnOnce() + 'a>>,
}

struct RecordingTarget {
    thread_id: ThreadId,
    relative_cwd: String,
}

/// Runs a command with literal arguments (no shell), streams its output
/// through unchanged, and records start and finish evidence when a thread is
/// recording and the working directory is inside its project.
///
/// A command that cannot be recorded still runs; the user is told why. Only an
/// unusable working directory or an empty command is refused.
pub fn run_command(
    database: Option<&Database>,
    request: &CommandRequest,
    io: RunIo<'_>,
) -> Result<RunOutcome, DomainError> {
    let RunIo {
        stdout,
        stderr,
        notify,
        after_spawn,
    } = io;

    if request.program.trim().is_empty() {
        return Err(DomainError::invalid_input(
            "A command is required.",
            "command",
        ));
    }
    let cwd = fs::canonicalize(&request.cwd).map_err(|_| {
        DomainError::invalid_input(
            "The working directory does not exist or cannot be opened.",
            "cwd",
        )
    })?;
    if !cwd.is_dir() {
        return Err(DomainError::invalid_input(
            "The working directory is not a folder.",
            "cwd",
        ));
    }

    let mut outcome = RunOutcome {
        exit_code: 0,
        thread_id: None,
        started_event_id: None,
        finished_event_id: None,
    };

    let service = database.map(ThreadService::new);
    let target = match database {
        Some(database) => recording_target(database, &cwd),
        None => Err("ThreadBack has no data yet. Open the app and start a thread first.".into()),
    };
    match (&service, &target) {
        (Some(service), Ok(target)) => {
            let started = CommandStartedPayload {
                command: display_command(&request.program, &request.args),
                cwd: target.relative_cwd.clone(),
            };
            match service.record(
                &target.thread_id,
                EventPayload::CommandStarted(to_value(&started)?),
                Sensitivity::Normal,
            ) {
                Ok(event) => {
                    outcome.thread_id = Some(target.thread_id.clone());
                    outcome.started_event_id = Some(event.id);
                }
                Err(error) => notify(&format!("not recorded: {}", error.message)),
            }
        }
        (_, Err(reason)) => notify(&format!("not recorded: {reason}")),
        (None, Ok(_)) => unreachable!("a target requires a database"),
    }

    let executed = execute(request, &cwd, stdout, stderr, after_spawn);
    outcome.exit_code = executed
        .exit_code
        .unwrap_or_else(|| 128 + executed.signal.unwrap_or(0));

    if let (Some(service), Some(thread_id), Some(started_id)) =
        (&service, &outcome.thread_id, &outcome.started_event_id)
    {
        let finished = CommandFinishedPayload {
            started_event_id: started_id.clone(),
            exit_code: executed.exit_code,
            signal: executed.signal,
            duration_ms: executed.duration.as_millis().min(u128::from(u64::MAX)) as u64,
            stdout: executed.stdout,
            stderr: executed.stderr,
        };
        match service.record(
            thread_id,
            EventPayload::CommandFinished(to_value(&finished)?),
            Sensitivity::Normal,
        ) {
            Ok(event) => outcome.finished_event_id = Some(event.id),
            Err(error) => notify(&format!("result not recorded: {}", error.message)),
        }
    }
    Ok(outcome)
}

fn recording_target(database: &Database, cwd: &Path) -> Result<RecordingTarget, String> {
    let thread = database
        .find_live_thread()
        .map_err(|error| error.message)?
        .ok_or("no thread is recording.")?;
    let project = database
        .find_project(&thread.project_id)
        .map_err(|error| error.message)?
        .ok_or("the thread's project is no longer approved.")?;
    let boundary =
        ProjectBoundary::new(&project.canonical_root, &[]).map_err(|error| error.message)?;
    let resolved = boundary.resolve(cwd).map_err(|error| {
        format!(
            "this folder is not inside the recording thread's project ({}).",
            error.message.trim_end_matches('.')
        )
    })?;
    Ok(RecordingTarget {
        thread_id: thread.id,
        relative_cwd: resolved.relative,
    })
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, DomainError> {
    serde_json::to_value(value)
        .map_err(|error| DomainError::internal(format!("Command serialization failed: {error}")))
}

struct Executed {
    exit_code: Option<i32>,
    signal: Option<i32>,
    duration: Duration,
    stdout: OutputExcerpt,
    stderr: OutputExcerpt,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stream {
    Stdout,
    Stderr,
}

fn execute(
    request: &CommandRequest,
    cwd: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    after_spawn: Option<Box<dyn FnOnce() + '_>>,
) -> Executed {
    let mut captures: HashMap<Stream, OutputCapture> = HashMap::from([
        (Stream::Stdout, OutputCapture::new(request.limits)),
        (Stream::Stderr, OutputCapture::new(request.limits)),
    ]);
    let started = Instant::now();

    let mut child = match Command::new(&request.program)
        .args(&request.args)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            // Mirror a shell: 127 when the program is missing, 126 when it
            // exists but cannot be run.
            let (code, message) = match error.kind() {
                io::ErrorKind::NotFound => {
                    (127, format!("tb: {}: command not found\n", request.program))
                }
                io::ErrorKind::PermissionDenied => {
                    (126, format!("tb: {}: permission denied\n", request.program))
                }
                _ => (
                    126,
                    format!("tb: {}: could not be started\n", request.program),
                ),
            };
            let _ = stderr.write_all(message.as_bytes());
            captures
                .get_mut(&Stream::Stderr)
                .expect("present")
                .push(message.as_bytes());
            return finish(Some(code), None, started, captures);
        }
    };
    if let Some(after_spawn) = after_spawn {
        after_spawn();
    }

    let (sender, receiver) = mpsc::channel::<(Stream, Vec<u8>)>();
    spawn_reader(child.stdout.take(), Stream::Stdout, sender.clone());
    spawn_reader(child.stderr.take(), Stream::Stderr, sender);

    let mut status = None;
    let mut drain_until: Option<Instant> = None;
    loop {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok((stream, chunk)) => {
                // A closed or full consumer must not stop the capture.
                let _ = match stream {
                    Stream::Stdout => stdout.write_all(&chunk).and_then(|_| stdout.flush()),
                    Stream::Stderr => stderr.write_all(&chunk).and_then(|_| stderr.flush()),
                };
                captures.get_mut(&stream).expect("present").push(&chunk);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if status.is_none() {
            if let Ok(Some(exited)) = child.try_wait() {
                status = Some(exited);
                drain_until = Some(Instant::now() + PIPE_GRACE);
            }
        }
        if drain_until.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
    }

    let status = match status {
        Some(status) => Some(status),
        None => child.wait().ok(),
    };
    let (code, signal) = status.map(split_status).unwrap_or((Some(1), None));
    finish(code, signal, started, captures)
}

fn finish(
    exit_code: Option<i32>,
    signal: Option<i32>,
    started: Instant,
    mut captures: HashMap<Stream, OutputCapture>,
) -> Executed {
    Executed {
        exit_code,
        signal,
        duration: started.elapsed(),
        stdout: captures.remove(&Stream::Stdout).expect("present").finish(),
        stderr: captures.remove(&Stream::Stderr).expect("present").finish(),
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    source: Option<R>,
    stream: Stream,
    sender: mpsc::Sender<(Stream, Vec<u8>)>,
) {
    let Some(mut source) = source else { return };
    thread::spawn(move || {
        let mut buffer = vec![0u8; READ_CHUNK_BYTES];
        loop {
            match source.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if sender.send((stream, buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

#[cfg(unix)]
fn split_status(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    use std::os::unix::process::ExitStatusExt;
    (status.code(), status.signal())
}

#[cfg(not(unix))]
fn split_status(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    (status.code(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(limits: (usize, usize), chunks: &[&[u8]]) -> OutputExcerpt {
        let mut capture = OutputCapture::new(CaptureLimits {
            head_bytes: limits.0,
            tail_bytes: limits.1,
        });
        for chunk in chunks {
            capture.push(chunk);
        }
        capture.finish()
    }

    #[test]
    fn small_output_is_kept_whole() {
        let excerpt = capture((8, 8), &[b"hello ", b"world"]);
        assert_eq!(format!("{}{}", excerpt.head, excerpt.tail), "hello world");
        assert!(!excerpt.truncated);
        assert_eq!(excerpt.omitted_bytes, 0);
        assert_eq!(excerpt.original_bytes, 11);
    }

    #[test]
    fn large_output_keeps_head_and_tail_with_exact_counts() {
        let data: Vec<u8> = (0..1000u32).map(|n| b'a' + (n % 26) as u8).collect();
        let excerpt = capture((10, 20), &[&data[..300], &data[300..]]);
        assert_eq!(excerpt.head.as_bytes(), &data[..10]);
        assert_eq!(excerpt.tail.as_bytes(), &data[980..]);
        assert!(excerpt.truncated);
        assert_eq!(excerpt.original_bytes, 1000);
        assert_eq!(excerpt.omitted_bytes, 970);
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_result() {
        let data: Vec<u8> = (0..500u32).map(|n| b'a' + (n % 26) as u8).collect();
        let whole = capture((16, 32), &[&data]);
        let single_bytes: Vec<&[u8]> = data.chunks(1).collect();
        let bytewise = capture((16, 32), &single_bytes);
        let uneven: Vec<&[u8]> = data.chunks(7).collect();
        assert_eq!(whole, bytewise);
        assert_eq!(whole, capture((16, 32), &uneven));
    }

    #[test]
    fn cuts_never_split_a_multibyte_character() {
        let text = "é".repeat(100);
        let excerpt = capture((5, 5), &[text.as_bytes()]);
        assert!(!excerpt.head.contains('\u{FFFD}'));
        assert!(!excerpt.tail.contains('\u{FFFD}'));
        assert!(excerpt.truncated);
        assert_eq!(
            excerpt.original_bytes,
            excerpt.omitted_bytes + (excerpt.head.len() + excerpt.tail.len()) as u64
        );
    }

    #[test]
    fn binary_output_is_counted_but_not_stored() {
        let excerpt = capture((8, 8), &[b"abc\0def"]);
        assert!(excerpt.binary);
        assert_eq!(excerpt.head, "");
        assert_eq!(excerpt.tail, "");
        assert_eq!(excerpt.original_bytes, 7);
    }

    #[test]
    fn empty_output_is_empty_and_not_truncated() {
        let excerpt = capture((8, 8), &[]);
        assert!(!excerpt.truncated && !excerpt.binary);
        assert_eq!(excerpt.original_bytes, 0);
    }

    #[test]
    fn zero_tail_keeps_only_the_head() {
        let excerpt = capture((4, 0), &[b"abcdefgh"]);
        assert_eq!(excerpt.head, "abcd");
        assert_eq!(excerpt.tail, "");
        assert_eq!(excerpt.omitted_bytes, 4);
    }

    #[test]
    fn display_command_quotes_only_when_needed() {
        let args = [
            "--flag=1".to_owned(),
            "two words".to_owned(),
            "it's".to_owned(),
            String::new(),
        ];
        assert_eq!(
            display_command("cargo", &args),
            "cargo --flag=1 'two words' 'it'\\''s' ''"
        );
        assert_eq!(display_command("ls", &[]), "ls");
    }
}
