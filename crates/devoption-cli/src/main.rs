use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]
       devoption serve [--port PORT] [--data-dir ABSOLUTE_PATH]

  -h, --help     Show this help
  -V, --version  Show the development version
  serve         Start HTTP on 127.0.0.1:3000; Ctrl+C stops the process
  --port PORT   Override the port (0 chooses a free port)
  --data-dir    Validate and retain an existing private owner-data directory (Unix)

The data path must be absolute, with no symlinks or parent traversal. No directories or accounts are created; browser account setup and sign-in are not implemented.
The local server shows Home, Projects, and About placeholders plus browser-local Appearance settings. Factory and runtime services are not implemented.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let options = match args.as_slice() {
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => None,
        [flag] if flag == "--version" || flag == "-V" => {
            println!("devoption {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [command, options @ ..] if command == "serve" => match serve_options(options) {
            Ok(options) => Some(options),
            Err(error) => {
                eprintln!("error: {error}; use --help");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("error: unsupported command or options; use --help");
            return ExitCode::from(2);
        }
    };

    if let Some((port, data_directory)) = options {
        if let Err(error) = devoption_server::run(port, data_directory.as_deref()) {
            eprintln!("error: could not serve on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("{HELP}");
    }
    ExitCode::SUCCESS
}

fn serve_options(mut args: &[OsString]) -> Result<(u16, Option<PathBuf>), &'static str> {
    let mut port = None;
    let mut data_directory = None;
    while !args.is_empty() {
        let [flag, value, rest @ ..] = args else {
            return Err("serve options require a value");
        };
        if flag == "--port" && port.is_none() {
            port = Some(
                value
                    .to_str()
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or("port must be an integer from 0 to 65535")?,
            );
        } else if flag == "--data-dir" && data_directory.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("data directory requires an absolute path");
            }
            data_directory = Some(path);
        } else {
            return Err("unsupported or duplicate serve option");
        }
        args = rest;
    }
    Ok((port.unwrap_or(3000), data_directory))
}
