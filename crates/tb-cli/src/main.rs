use std::{io, path::Path, process::ExitCode};

use threadback_core::{CaptureLimits, CommandRequest, Database, RunIo, database_path, run_command};

const USAGE: &str = "ThreadBack command wrapper

Usage:
  tb run -- <command> [args...]
  tb --version

`tb run` runs the command exactly as given (no shell) and, while a ThreadBack
thread is recording in this project, records the command, its exit code, and
a bounded excerpt of its output. The command's exit code is passed through.";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("tb {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("run") => run(args.collect()),
        _ => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
    }
}

fn run(mut rest: Vec<String>) -> ExitCode {
    if rest.first().map(String::as_str) == Some("--") {
        rest.remove(0);
    }
    if rest.is_empty() {
        eprintln!("tb: run needs a command, for example: tb run -- cargo test");
        return ExitCode::from(2);
    }
    let program = rest.remove(0);

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("tb: the working directory is not accessible: {error}");
            return ExitCode::from(2);
        }
    };

    let database = open_existing_database(&database_path());
    let request = CommandRequest {
        program,
        args: rest,
        cwd,
        limits: CaptureLimits::default(),
    };

    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut notify = |message: &str| eprintln!("tb: {message}");
    let io = RunIo {
        stdout: &mut stdout,
        stderr: &mut stderr,
        notify: &mut notify,
        after_spawn: Some(Box::new(ignore_interrupts)),
    };

    match run_command(database.as_ref(), &request, io) {
        Ok(outcome) => ExitCode::from((outcome.exit_code & 0xFF) as u8),
        Err(error) => {
            eprintln!("tb: {}", error.message);
            ExitCode::from(2)
        }
    }
}

/// The wrapper never creates the database: if the app has not run yet there is
/// nothing to record into, and the command still runs.
fn open_existing_database(path: &Path) -> Option<Database> {
    if !path.is_file() {
        return None;
    }
    Database::open(path).ok()
}

/// Ctrl-C reaches the whole foreground process group. The child handles it as
/// it normally would; the wrapper keeps running so it can record how the
/// command ended. Done after the child has started so the child keeps the
/// default behaviour.
#[cfg(unix)]
fn ignore_interrupts() {
    // SAFETY: setting a signal disposition to "ignore" has no memory-safety
    // requirements.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
    }
}

#[cfg(not(unix))]
fn ignore_interrupts() {}
