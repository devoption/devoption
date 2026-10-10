use std::{env, process::ExitCode};

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]

  -h, --help     Show this help
  -V, --version  Show the development version

This is a repository scaffold. Factory and runtime services are not implemented.";

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let first = args.next();
    if args.next().is_some() {
        eprintln!("error: expected at most one option; use --help");
        return ExitCode::from(2);
    }

    match first.as_deref().map(|arg| arg.to_str()) {
        None | Some(Some("--help" | "-h")) => println!("{HELP}"),
        Some(Some("--version" | "-V")) => println!("devoption {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("error: unsupported option; use --help");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
