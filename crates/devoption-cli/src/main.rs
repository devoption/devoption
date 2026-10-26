use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

#[cfg(unix)]
mod recovery;
#[cfg(unix)]
mod setup;
#[cfg(unix)]
mod source_check;

const HELP: &str = "DevOption — software factory foundation

Usage: devoption [--help | --version]
       devoption serve [--port PORT] [--public-origin HTTPS_ORIGIN] [--data-dir ABSOLUTE_PATH] [--project-secrets-key-file ABSOLUTE_PATH] [--build-history-dir ABSOLUTE_PATH] [--build-execution enabled] [--app-deployment enabled] [--repository-root ABSOLUTE_PATH | --managed-repository-root ABSOLUTE_PATH]
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

    if let Some(ServeOptions {
        port,
        data_directory,
        repository_root,
        managed_repository_root,
        build_history_directory,
        public_origin,
        execution_enabled,
        deployment_enabled,
        project_secrets_key_file,
    }) = options
    {
        let result = if let Some(key_file) = project_secrets_key_file {
            devoption_server::run_with_project_secrets(
                port,
                data_directory.as_deref().expect("validated owner"),
                managed_repository_root
                    .as_deref()
                    .or(repository_root.as_deref()),
                managed_repository_root.is_some(),
                build_history_directory.as_deref(),
                public_origin,
                devoption_server::ProjectSecretsOptions {
                    key_file: &key_file,
                    build_execution: execution_enabled,
                    app_deployment: deployment_enabled,
                },
            )
        } else if deployment_enabled {
            devoption_server::run_with_deployments(
                port,
                data_directory.as_deref().expect("validated owner"),
                managed_repository_root
                    .as_deref()
                    .or(repository_root.as_deref())
                    .expect("validated root"),
                managed_repository_root.is_some(),
                build_history_directory
                    .as_deref()
                    .expect("validated history"),
                public_origin,
            )
        } else if execution_enabled {
            devoption_server::run_with_execution(
                port,
                data_directory.as_deref().expect("validated owner"),
                managed_repository_root
                    .as_deref()
                    .or(repository_root.as_deref())
                    .expect("validated repository root"),
                managed_repository_root.is_some(),
                build_history_directory
                    .as_deref()
                    .expect("validated history"),
                public_origin,
            )
        } else if let Some(origin) = public_origin {
            devoption_server::run_with_public_origin(
                port,
                data_directory
                    .as_deref()
                    .expect("public origin requires data directory"),
                managed_repository_root
                    .as_deref()
                    .or(repository_root.as_deref()),
                managed_repository_root.is_some(),
                build_history_directory.as_deref(),
                origin,
            )
        } else if let Some(history) = build_history_directory {
            devoption_server::run_with_build_history(
                port,
                data_directory
                    .as_deref()
                    .expect("history requires data directory"),
                managed_repository_root
                    .as_deref()
                    .or(repository_root.as_deref()),
                managed_repository_root.is_some(),
                &history,
            )
        } else if let Some(root) = managed_repository_root {
            devoption_server::run_with_managed_repository_root(
                port,
                data_directory.as_deref(),
                &root,
            )
        } else {
            devoption_server::run_with_repository_root(
                port,
                data_directory.as_deref(),
                repository_root.as_deref(),
            )
        };
        if let Err(error) = result {
            eprintln!("error: could not serve on 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        println!("{HELP}");
    }
    ExitCode::SUCCESS
}

struct ServeOptions {
    project_secrets_key_file: Option<PathBuf>,
    port: u16,
    data_directory: Option<PathBuf>,
    repository_root: Option<PathBuf>,
    managed_repository_root: Option<PathBuf>,
    build_history_directory: Option<PathBuf>,
    public_origin: Option<devoption_server::PublicOrigin>,
    execution_enabled: bool,
    deployment_enabled: bool,
}

fn serve_options(mut args: &[OsString]) -> Result<ServeOptions, &'static str> {
    let mut project_secrets_key_file = None;
    let mut public_origin = None;
    let mut execution_enabled = false;
    let mut deployment_enabled = false;
    let mut port = None;
    let mut data_directory = None;
    let mut repository_root = None;
    let mut managed_repository_root = None;
    let mut build_history_directory = None;
    while !args.is_empty() {
        let [flag, value, rest @ ..] = args else {
            return Err("serve options require a value");
        };
        if flag == "--build-execution" && value == "enabled" && !execution_enabled {
            execution_enabled = true;
        } else if flag == "--app-deployment" && value == "enabled" && !deployment_enabled {
            deployment_enabled = true;
        } else if flag == "--port" && port.is_none() {
            port = Some(
                value
                    .to_str()
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or("port must be an integer from 0 to 65535")?,
            );
        } else if flag == "--public-origin" && public_origin.is_none() {
            public_origin = Some(devoption_server::PublicOrigin::parse(
                value.to_str().ok_or("public origin must be UTF-8")?,
            )?);
        } else if flag == "--data-dir" && data_directory.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("data directory requires an absolute path");
            }
            data_directory = Some(path);
        } else if flag == "--project-secrets-key-file" && project_secrets_key_file.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("project secrets key requires an absolute path");
            }
            project_secrets_key_file = Some(path);
        } else if flag == "--build-history-dir" && build_history_directory.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("build history requires an absolute path");
            }
            build_history_directory = Some(path);
        } else if (flag == "--repository-root" || flag == "--managed-repository-root")
            && repository_root.is_none()
            && managed_repository_root.is_none()
        {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("repository root requires an absolute path");
            }
            if flag == "--managed-repository-root" {
                managed_repository_root = Some(path);
            } else {
                repository_root = Some(path);
            }
        } else {
            return Err("unsupported or duplicate serve option");
        }
        args = rest;
    }
    if (repository_root.is_some() || managed_repository_root.is_some()) && data_directory.is_none()
    {
        return Err("repository root requires --data-dir with an existing configured owner");
    }
    if build_history_directory.is_some() && data_directory.is_none() {
        return Err("build history requires --data-dir with an existing configured owner");
    }
    if project_secrets_key_file.is_some() && data_directory.is_none() {
        return Err("project secrets require --data-dir with an existing configured owner");
    }
    if public_origin.is_some() && data_directory.is_none() {
        return Err("public origin requires --data-dir with an existing configured owner");
    }
    if deployment_enabled && !execution_enabled {
        return Err("app deployment requires --build-execution enabled");
    }
    if execution_enabled
        && (data_directory.is_none()
            || build_history_directory.is_none()
            || (repository_root.is_none() && managed_repository_root.is_none()))
    {
        return Err("build execution requires owner, repository and history directories");
    }
    Ok(ServeOptions {
        project_secrets_key_file,
        deployment_enabled,
        execution_enabled,
        public_origin,
        port: port.unwrap_or(3000),
        data_directory,
        repository_root,
        managed_repository_root,
        build_history_directory,
    })
}

#[cfg(test)]
mod deployment_options_tests {
    use super::*;
    #[test]
    fn deployment_is_explicit_and_requires_the_existing_build_storage_contract() {
        assert!(!serve_options(&[]).unwrap().deployment_enabled);
        let parse =
            |args: &[&str]| serve_options(&args.iter().map(OsString::from).collect::<Vec<_>>());
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
        assert!(parse(&selected).unwrap().deployment_enabled);
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
        let selected = serve_options(&options(&[
            "--data-dir",
            "/private/data",
            "--project-secrets-key-file",
            "/private/key",
        ]))
        .unwrap();
        assert_eq!(
            selected.project_secrets_key_file,
            Some(PathBuf::from("/private/key"))
        );
        assert!(!selected.execution_enabled);
        assert!(!selected.deployment_enabled);
    }
}
