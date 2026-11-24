use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
};

use devoption_server::{PublicOrigin, ServerOptions};

#[cfg(unix)]
mod recovery;
#[cfg(unix)]
mod setup;
#[cfg(unix)]
mod source_check;

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]
       devoption serve [--port PORT] [--public-origin HTTPS_ORIGIN] [--data-dir ABSOLUTE_PATH] [--project-secrets-key-file ABSOLUTE_PATH] [--package-registry-dir ABSOLUTE_PATH] [--build-history-dir ABSOLUTE_PATH] [--build-execution enabled] [--app-deployment enabled] [--repository-root ABSOLUTE_PATH | --managed-repository-root ABSOLUTE_PATH]
       devoption setup --data-dir ABSOLUTE_PATH
       devoption check-source --data-dir ABSOLUTE_PATH --repository-root ABSOLUTE_PATH --project 32HEX --repository 32HEX
       devoption recover-repositories --data-dir ABSOLUTE_PATH --repository-root ABSOLUTE_PATH [--approve-device-rebind 64HEX]

  -h, --help     Show this help
  -V, --version  Show the development version
  serve         Start HTTP on 127.0.0.1:3000; Ctrl+C stops the process
  setup         Create the first owner using hidden terminal prompts (Unix/local/SSH)
  check-source  Read saved repository source checks (Unix/local/SSH); no build runs
  recover-repositories  Review changed device bindings; explicit approval required to apply (Unix)
  --port PORT   Override the port (0 chooses a free port)
  --public-origin  Serve one canonical https://DNS_NAME through a local TLS proxy; requires an existing owner
  --project-secrets-key-file  Enable encrypted project secret management using an existing protected 32-byte key (Unix)
  --package-registry-dir  Enable owner-only registry controls over an existing private registry root (Unix)
  --data-dir    Validate and retain an existing private owner-data directory (Unix)
  --app-deployment     Explicitly enable installed Linux app runtime (value: enabled)
  --build-execution    Explicitly enable installed Linux build runner (value: enabled)
  --build-history-dir  Recover selected private history before serving
  --repository-root  Explicit trusted checkout root for inspection/attachment
  --managed-repository-root  Explicit private root allowing creation of new repositories

The data path must be absolute, private, and owned by this OS user, with no symlinks or parent traversal. Startup creates no directories or permissions. Managed mode permits authenticated creation of new repository directories only. Serve creates no accounts; setup never replaces an owner.
After CLI setup, run devoption serve --data-dir ABSOLUTE_PATH with the same directory, then sign in at /sign-in. Browser owner setup is not available.
The local server shows Overview, saved projects and empty-project creation for configured owners, and browser-local Appearance settings. Global Releases and Infrastructure show unavailable states. Installed Linux app deployment is opt-in through project Releases; see docs/web-app-deployments.md. Full factory automation is not implemented.
Source checks require all four options and saved 32-character hexadecimal IDs from the project and repository URLs. They use this OS user's filesystem access without reading owner credentials or starting HTTP. Exit status: 0 checks passed, 1 changes needed or unavailable, 2 invalid arguments. Passing checks do not approve execution. See docs/build-preparation.md.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    #[cfg(unix)]
    if let [command, path] = args.as_slice()
        && command == "build-input-digest"
    {
        return match devoption_builds::execution::input_digest(&PathBuf::from(path)) {
            Ok(digest) => {
                println!("{digest}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                eprintln!("error: prepared input digest unavailable");
                ExitCode::FAILURE
            }
        };
    }
    let options = match args.as_slice() {
        [command, options @ ..] if command == "recover-repositories" => {
            #[cfg(unix)]
            return recovery::run(options);
            #[cfg(not(unix))]
            {
                let _ = options;
                eprintln!("error: repository recovery requires Unix");
                return ExitCode::from(2);
            }
        }
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => None,
        [flag] if flag == "--version" || flag == "-V" => {
            println!("devoption {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        [command, options @ ..] if command == "check-source" => {
            #[cfg(unix)]
            return source_check::run(options);
            #[cfg(not(unix))]
            {
                let _ = options;
                eprintln!("error: source checks require Unix");
                return ExitCode::FAILURE;
            }
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

    if let Some(options) = options {
        let port = options.port;
        if let Err(error) = devoption_server::run_with_options(options) {
            eprintln!("error: could not serve on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("{HELP}");
    }
    ExitCode::SUCCESS
}

fn serve_options(mut args: &[OsString]) -> Result<ServerOptions<'_>, &'static str> {
    let mut options = ServerOptions::default();
    let mut port = None;
    while !args.is_empty() {
        let [flag, value, rest @ ..] = args else {
            return Err("serve options require a value");
        };
        if flag == "--build-execution" && value == "enabled" && !options.execution_enabled {
            options.execution_enabled = true;
        } else if flag == "--app-deployment" && value == "enabled" && !options.deployment_enabled {
            options.deployment_enabled = true;
        } else if flag == "--port" && port.is_none() {
            port = Some(
                value
                    .to_str()
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or("port must be an integer from 0 to 65535")?,
            );
        } else if flag == "--public-origin" && options.public_origin.is_none() {
            options.public_origin = Some(PublicOrigin::parse(
                value.to_str().ok_or("public origin must be UTF-8")?,
            )?);
        } else if flag == "--package-registry-dir" && options.package_registry_directory.is_none() {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err("package registry requires an absolute path");
            }
            options.package_registry_directory = Some(path);
        } else if flag == "--data-dir" && options.data_directory.is_none() {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err("data directory requires an absolute path");
            }
            options.data_directory = Some(path);
        } else if flag == "--project-secrets-key-file" && options.project_secrets_key_file.is_none()
        {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err("project secrets key requires an absolute path");
            }
            options.project_secrets_key_file = Some(path);
        } else if flag == "--build-history-dir" && options.build_history_directory.is_none() {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err("build history requires an absolute path");
            }
            options.build_history_directory = Some(path);
        } else if (flag == "--repository-root" || flag == "--managed-repository-root")
            && options.repository_root.is_none()
        {
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err("repository root requires an absolute path");
            }
            options.repository_root = Some(path);
            options.managed_repository = flag == "--managed-repository-root";
        } else {
            return Err("unsupported or duplicate serve option");
        }
        args = rest;
    }
    options.port = port.unwrap_or(3000);
    options.validate()?;
    Ok(options)
}

#[cfg(test)]
mod deployment_options_tests {
    use super::*;
    #[test]
    fn deployment_is_explicit_and_requires_the_existing_build_storage_contract() {
        assert!(!serve_options(&[]).unwrap().deployment_enabled);
        let parse = |args: &[&str]| {
            serve_options(&args.iter().map(OsString::from).collect::<Vec<_>>())
                .map(|options| options.deployment_enabled)
        };
        assert!(parse(&["--app-deployment", "enabled"]).is_err());
        assert!(
            parse(&[
                "--app-deployment",
                "enabled",
                "--build-execution",
                "enabled"
            ])
            .is_err()
        );
        let selected = [
            "--app-deployment",
            "enabled",
            "--build-execution",
            "enabled",
            "--data-dir",
            "/owner",
            "--repository-root",
            "/repositories",
            "--build-history-dir",
            "/history",
        ];
        assert!(parse(&selected).unwrap());
        let mut duplicate = selected.to_vec();
        duplicate.extend(["--app-deployment", "enabled"]);
        assert!(parse(&duplicate).is_err());
    }
}

#[cfg(test)]
mod project_secrets_options_tests {
    use super::*;
    fn options(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn secret_key_is_explicit_absolute_and_requires_owner_storage() {
        assert!(
            serve_options(&[])
                .unwrap()
                .project_secrets_key_file
                .is_none()
        );
        for values in [
            vec!["--project-secrets-key-file", "relative"],
            vec!["--project-secrets-key-file", "/private/key"],
            vec![
                "--data-dir",
                "/private/data",
                "--project-secrets-key-file",
                "/private/key",
                "--project-secrets-key-file",
                "/other/key",
            ],
        ] {
            assert!(serve_options(&options(&values)).is_err());
        }
        let args = options(&[
            "--data-dir",
            "/private/data",
            "--project-secrets-key-file",
            "/private/key",
        ]);
        let selected = serve_options(&args).unwrap();
        assert_eq!(
            selected.project_secrets_key_file,
            Some(Path::new("/private/key"))
        );
        assert!(!selected.execution_enabled);
        assert!(!selected.deployment_enabled);
    }
}

#[cfg(test)]
mod package_registry_options_tests {
    use super::*;
    #[test]
    fn registry_requires_one_absolute_root_and_owner_storage() {
        let parse = |args: Vec<&str>| {
            serve_options(&args.into_iter().map(OsString::from).collect::<Vec<_>>())
                .map(|options| options.package_registry_directory.map(Path::to_path_buf))
        };
        for args in [
            vec!["--package-registry-dir", "relative", "--data-dir", "/owner"],
            vec!["--package-registry-dir", "/registry"],
            vec![
                "--package-registry-dir",
                "/one",
                "--package-registry-dir",
                "/two",
                "--data-dir",
                "/owner",
            ],
        ] {
            assert!(parse(args).is_err());
        }
        let options = parse(vec![
            "--data-dir",
            "/owner",
            "--package-registry-dir",
            "/registry",
        ])
        .unwrap();
        assert_eq!(options, Some(PathBuf::from("/registry")));
        assert!(parse(vec!["--data-dir", "/owner"]).unwrap().is_none());
    }
}
