use std::{io, path::Path, process::ExitCode};

use threadback_core::{CaptureLimits, CommandRequest, Database, RunIo, database_path, run_command};

const USAGE: &str = "ThreadBack command wrapper

Usage:
  tb run [options] -- <command> [args...]
  tb --version

Options (before the command; anything after it belongs to the command):
  --head-bytes <n>   keep the first n bytes of each output stream (default 4096)
  --tail-bytes <n>   keep the last n bytes of each output stream (default 8192)
                     Each is at most 65536. Environment: THREADBACK_HEAD_BYTES,
                     THREADBACK_TAIL_BYTES. Output is always passed through in
                     full; the limits only bound what is stored.

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

fn run(rest: Vec<String>) -> ExitCode {
    let parsed = match parse_run_args(rest, &|name| std::env::var(name).ok()) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("tb: {message}");
            return ExitCode::from(2);
        }
    };
    let ParsedRun {
        limits,
        program,
        args,
    } = parsed;

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
        args,
        cwd,
        limits,
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

struct ParsedRun {
    limits: CaptureLimits,
    program: String,
    args: Vec<String>,
}

/// Reads the options in front of the command, then treats everything else as
/// the command line. Options never apply after the command starts, so a
/// command's own `--head-bytes` is left alone. Flags override the environment.
fn parse_run_args(
    mut rest: Vec<String>,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Result<ParsedRun, String> {
    let mut head = environment_bytes(environment, "THREADBACK_HEAD_BYTES")?;
    let mut tail = environment_bytes(environment, "THREADBACK_TAIL_BYTES")?;

    while let Some(first) = rest.first().cloned() {
        if first == "--" {
            rest.remove(0);
            break;
        }
        if !first.starts_with('-') {
            break;
        }
        let (name, inline) = match first.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (first.clone(), None),
        };
        let slot = match name.as_str() {
            "--head-bytes" => &mut head,
            "--tail-bytes" => &mut tail,
            _ => {
                return Err(format!(
                    "unknown option '{first}'. Put the command after --, for example: tb run -- cargo test"
                ));
            }
        };
        rest.remove(0);
        let value = match inline {
            Some(value) => value,
            None if !rest.is_empty() => rest.remove(0),
            None => return Err(format!("{name} needs a number of bytes.")),
        };
        *slot = Some(parse_bytes(&name, &value)?);
    }

    if rest.is_empty() {
        return Err("run needs a command, for example: tb run -- cargo test".into());
    }
    let defaults = CaptureLimits::default();
    let limits = CaptureLimits::new(
        head.unwrap_or(defaults.head_bytes),
        tail.unwrap_or(defaults.tail_bytes),
    )
    .map_err(|error| error.message)?;
    let program = rest.remove(0);
    Ok(ParsedRun {
        limits,
        program,
        args: rest,
    })
}

fn environment_bytes(
    environment: &dyn Fn(&str) -> Option<String>,
    name: &str,
) -> Result<Option<usize>, String> {
    environment(name)
        .filter(|value| !value.is_empty())
        .map(|value| parse_bytes(name, &value))
        .transpose()
}

fn parse_bytes(name: &str, value: &str) -> Result<usize, String> {
    value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a whole number of bytes, not '{value}'."))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn no_environment(_: &str) -> Option<String> {
        None
    }

    fn parse(values: &[&str]) -> Result<ParsedRun, String> {
        parse_run_args(args(values), &no_environment)
    }

    #[test]
    fn defaults_apply_without_options() {
        let parsed = parse(&["--", "cargo", "test"]).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::default());
        assert_eq!(
            (parsed.program.as_str(), parsed.args),
            ("cargo", args(&["test"]))
        );
    }

    #[test]
    fn options_before_the_command_set_the_limits_in_both_forms() {
        let parsed = parse(&["--head-bytes", "10", "--tail-bytes=20", "--", "ls"]).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::new(10, 20).unwrap());
        assert_eq!(parsed.program, "ls");
    }

    #[test]
    fn options_are_also_read_without_the_double_dash() {
        let parsed = parse(&["--tail-bytes", "0", "ls", "-l"]).unwrap();
        assert_eq!(parsed.limits.tail_bytes, 0);
        assert_eq!(parsed.args, args(&["-l"]));
    }

    #[test]
    fn options_after_the_command_belong_to_the_command() {
        let parsed = parse(&["--", "mytool", "--head-bytes", "1"]).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::default());
        assert_eq!(parsed.args, args(&["--head-bytes", "1"]));
        let parsed = parse(&["mytool", "--tail-bytes=1"]).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::default());
        assert_eq!(parsed.args, args(&["--tail-bytes=1"]));
    }

    #[test]
    fn environment_sets_defaults_and_flags_override_it() {
        let environment = |name: &str| match name {
            "THREADBACK_HEAD_BYTES" => Some("100".to_owned()),
            "THREADBACK_TAIL_BYTES" => Some("200".to_owned()),
            _ => None,
        };
        let parsed = parse_run_args(args(&["--", "ls"]), &environment).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::new(100, 200).unwrap());
        let parsed =
            parse_run_args(args(&["--head-bytes", "5", "--", "ls"]), &environment).unwrap();
        assert_eq!(parsed.limits, CaptureLimits::new(5, 200).unwrap());
    }

    #[test]
    fn bad_values_are_explained() {
        let error = |values: &[&str]| parse(values).err().unwrap();
        assert!(error(&["--head-bytes", "lots", "--", "ls"]).contains("whole number"));
        assert!(error(&["--head-bytes", "-5", "--", "ls"]).contains("whole number"));
        assert!(error(&["--head-bytes", "65537", "--", "ls"]).contains("at most 65536"));
        assert!(error(&["--tail-bytes", "--", "ls"]).contains("whole number"));
        assert!(error(&["--head-bytes"]).contains("needs a number"));
        assert!(error(&["--bogus", "--", "ls"]).contains("unknown option"));
        assert!(error(&["-v", "ls"]).contains("unknown option"));
        assert!(error(&[]).contains("needs a command"));
        assert!(error(&["--"]).contains("needs a command"));
        let environment = |name: &str| (name == "THREADBACK_TAIL_BYTES").then(|| "x".to_owned());
        assert!(
            parse_run_args(args(&["--", "ls"]), &environment)
                .err()
                .unwrap()
                .contains("THREADBACK_TAIL_BYTES")
        );
    }
}
