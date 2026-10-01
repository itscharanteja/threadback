use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("tb {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("run") => {
            eprintln!("tb run is not available until evidence collection is configured");
            ExitCode::from(2)
        }
        _ => {
            println!(
                "ThreadBack command wrapper\n\nUsage:\n  tb run -- <command> [args...]\n  tb --version"
            );
            ExitCode::SUCCESS
        }
    }
}
