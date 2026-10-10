use std::{env, process::ExitCode};

mod pages;
mod server;

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]
       devoption serve [--port PORT]

  -h, --help     Show this help
  -V, --version  Show the development version
  serve         Start HTTP on 127.0.0.1:3000; Ctrl+C stops the process
  --port PORT   Override the port (0 chooses a free port)

The local server shows Home and About placeholders. Factory and runtime services are not implemented.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let port = match args.as_slice() {
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => None,
        [flag] if flag == "--version" || flag == "-V" => {
            println!("devoption {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [command] if command == "serve" => Some(3000),
        [command, flag, port] if command == "serve" && flag == "--port" => {
            match port.to_str().and_then(|value| value.parse::<u16>().ok()) {
                Some(port) => Some(port),
                None => {
                    eprintln!("error: port must be an integer from 0 to 65535; use --help");
                    return ExitCode::from(2);
                }
            }
        }
        _ => {
            eprintln!("error: unsupported command or options; use --help");
            return ExitCode::from(2);
        }
    };

    if let Some(port) = port {
        if let Err(error) = server::run(port) {
            eprintln!("error: could not serve on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("{HELP}");
    }
    ExitCode::SUCCESS
}
