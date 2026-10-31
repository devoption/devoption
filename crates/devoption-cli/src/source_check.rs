use devoption_builds::preparation::{self, Outcome, Preparation};
use devoption_projects::{
    CatalogDirectory, Error, ProjectId, RepositoryId, worktree::PermittedRoot,
};
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

struct Options {
    data: PathBuf,
    root: PathBuf,
    project: ProjectId,
    repository: RepositoryId,
}

pub fn run(args: &[OsString]) -> ExitCode {
    let options = match options(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}; use --help");
            return ExitCode::from(2);
        }
    };
    match inspect(options) {
        Ok(report) => {
            println!(
                "{}",
                match report.outcome() {
                    Outcome::Passed => "Source checks passed",
                    Outcome::ChangesNeeded => "Changes needed",
                    Outcome::Unavailable => "Source checks unavailable",
                }
            );
            println!(
                "Observed HEAD: {}",
                report.status.commit.as_deref().unwrap_or("no commit")
            );
            if let Some(cargo) = &report.cargo {
                println!("Package: {} {}", cargo.package(), cargo.version());
                println!("Locked registry dependencies: {}", cargo.dependency_count());
            }
            for check in &report.checks {
                let outcome = match check.outcome {
                    Outcome::Passed => "passed",
                    Outcome::ChangesNeeded => "changes needed",
                    Outcome::Unavailable => "unavailable",
                };
                let detail = check
                    .detail
                    .strip_suffix(" See the supported source format below.")
                    .unwrap_or(&check.detail);
                println!("{}: {outcome} — {detail}", check.label);
            }
            println!(
                "Current working-tree observations only. No build ran or execution was approved."
            );
            println!("Supported source format and limits: docs/build-preparation.md");
            if report.outcome() == Outcome::Passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            // Project errors deliberately format fixed diagnostics, never source,
            // names, filesystem paths or underlying OS error strings.
            eprintln!("Source checks unavailable: {error}");
            ExitCode::FAILURE
        }
    }
}

fn inspect(options: Options) -> Result<Preparation, Error> {
    let directory = CatalogDirectory::open_absolute(&options.data)?;
    let root = PermittedRoot::open_absolute(&options.root)?;
    if !directory.is_disjoint(&root)? {
        return Err(Error::RootMismatch);
    }
    let catalog = directory.load()?.ok_or(Error::UnknownProject)?;
    preparation::inspect(&catalog, options.project, options.repository, &root)
}

fn options(mut args: &[OsString]) -> Result<Options, &'static str> {
    let (mut data, mut root, mut project, mut repository) = (None, None, None, None);
    while !args.is_empty() {
        let [flag, value, rest @ ..] = args else {
            return Err("source-check options require a value");
        };
        if flag == "--data-dir" && data.is_none() {
            data = Some(absolute_path(value)?);
        } else if flag == "--repository-root" && root.is_none() {
            root = Some(absolute_path(value)?);
        } else if flag == "--project" && project.is_none() {
            project = Some(identity(value, ProjectId::from_hex)?);
        } else if flag == "--repository" && repository.is_none() {
            repository = Some(identity(value, RepositoryId::from_hex)?);
        } else {
            return Err("unsupported or duplicate source-check option");
        }
        args = rest;
    }
    Ok(Options {
        data: data.ok_or("source checks require --data-dir")?,
        root: root.ok_or("source checks require --repository-root")?,
        project: project.ok_or("source checks require --project")?,
        repository: repository.ok_or("source checks require --repository")?,
    })
}

fn absolute_path(value: &OsString) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("source-check directories require absolute paths");
    }
    Ok(path)
}

fn identity<T>(value: &OsString, parse: impl FnOnce(&str) -> Option<T>) -> Result<T, &'static str> {
    let invalid = "project and repository IDs require exactly 32 hexadecimal characters";
    value.to_str().and_then(parse).ok_or(invalid)
}
