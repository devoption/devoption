use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

#[cfg(unix)]
mod setup;

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]
       devoption serve [--port PORT] [--data-dir ABSOLUTE_PATH] [--repository-root ABSOLUTE_PATH]
       devoption setup --data-dir ABSOLUTE_PATH

  -h, --help     Show this help
  -V, --version  Show the development version
  serve         Start HTTP on 127.0.0.1:3000; Ctrl+C stops the process
  setup         Create the first owner using hidden terminal prompts (Unix/local/SSH)
  --port PORT   Override the port (0 chooses a free port)
  --data-dir    Validate and retain an existing private owner-data directory (Unix)
  --repository-root  Explicit trusted checkout root; requires an existing configured owner

The data path must be absolute, private, and owned by this OS user, with no symlinks or parent traversal. No directories or permissions are created or changed. Serve creates no accounts; setup never replaces an owner.
After CLI setup, run devoption serve --data-dir ABSOLUTE_PATH with the same directory, then sign in at /sign-in. Browser owner setup is not available.
The local server shows Overview, saved projects and empty-project creation for configured owners, and browser-local Appearance settings. Releases and Infrastructure show unavailable states. Factory and runtime services are not implemented.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let options = match args.as_slice() {
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => None,
        [flag] if flag == "--version" || flag == "-V" => {
            println!("devoption {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [command, flag, path] if command == "setup" && flag == "--data-dir" => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                eprintln!("error: data directory requires an absolute path; use --help");
                return ExitCode::from(2);
            }
            #[cfg(unix)]
            return match setup::run(&path) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            };
            #[cfg(not(unix))]
            {
                eprintln!("error: owner setup requires Unix");
                return ExitCode::FAILURE;
            }
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

    if let Some(ServeOptions {
        port,
        data_directory,
        repository_root,
    }) = options
    {
        if let Err(error) = devoption_server::run_with_repository_root(
            port,
            data_directory.as_deref(),
            repository_root.as_deref(),
        ) {
            eprintln!("error: could not serve on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("{HELP}");
    }
    ExitCode::SUCCESS
}

struct ServeOptions {
    port: u16,
    data_directory: Option<PathBuf>,
    repository_root: Option<PathBuf>,
}

fn serve_options(mut args: &[OsString]) -> Result<ServeOptions, &'static str> {
    let mut port = None;
    let mut data_directory = None;
    let mut repository_root = None;
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
        } else if flag == "--repository-root" && repository_root.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("repository root requires an absolute path");
            }
            repository_root = Some(path);
        } else {
            return Err("unsupported or duplicate serve option");
        }
        args = rest;
    }
    if repository_root.is_some() && data_directory.is_none() {
        return Err("repository root requires --data-dir with an existing configured owner");
    }
    Ok(ServeOptions {
        port: port.unwrap_or(3000),
        data_directory,
        repository_root,
    })
}
