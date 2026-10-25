use super::*;
#[test]
fn preparation_escapes_dynamic_values_and_keeps_results_separate_from_execution() {
    let project = ProjectSummary {
        id: [1; 16],
        name: "<project>",
        repositories: vec![],
    };
    let view = PreparationView {
        repository_id: [2; 16],
        repository_name: "<repo>",
        headline: "Source checks passed",
        commit: Some("<commit>"),
        branch: Some("<branch>"),
        execution_available: false,
        package: Some(("<package>", "<version>", 2)),
        checks: vec![PreparationCheck {
            label: "<label>",
            outcome: "Passed",
            detail: "<script>fixture</script>",
        }],
        unavailable: None,
    };
    let html = crate::render_build_preparation(&project, &view);
    for escaped in [
        "&lt;repo&gt;",
        "&lt;branch&gt;",
        "&lt;commit&gt;",
        "&lt;package&gt;",
        "&lt;label&gt;",
        "&lt;script&gt;fixture&lt;/script&gt;",
    ] {
        assert!(html.contains(escaped));
    }
    assert!(!html.contains("<script>fixture"));
    assert!(html.contains("/builds\" aria-current=\"page\""));
    assert!(html.contains("not approve or run a build"));
    assert!(html.contains("Refresh source</a>"));
    assert!(!html.contains("<button") || !html.contains(">Run build</button>"));
    assert!(list(&project, true).contains("No repositories yet"));
}

fn sample_view() -> PreparationView<'static> {
    PreparationView {
        repository_id: [2; 16],
        repository_name: "sample-site",
        headline: "Source checks passed",
        branch: Some("main"),
        commit: Some("8c42e1a000000000000000000000000000000000"),
        execution_available: true,
        package: Some(("sample-site", "0.1.0", 0)),
        unavailable: None,
        checks: vec![
            PreparationCheck {
                label: "Commit",
                outcome: "Passed",
                detail: "HEAD identifies a commit.",
            },
            PreparationCheck {
                label: "Working tree",
                outcome: "Passed",
                detail: "Git reports no staged, modified, untracked or conflicted entries.",
            },
            PreparationCheck {
                label: "Cargo.toml",
                outcome: "Passed",
                detail: "Tracked text file is readable.",
            },
            PreparationCheck {
                label: "Cargo.lock",
                outcome: "Passed",
                detail: "Tracked text file is readable.",
            },
            PreparationCheck {
                label: "src/main.rs",
                outcome: "Passed",
                detail: "Tracked text file is readable.",
            },
            PreparationCheck {
                label: "Cargo inputs",
                outcome: "Passed",
                detail: "The standalone manifest and version-4 lockfile are internally consistent. Dependency archives have not been acquired or verified.",
            },
        ],
    }
}
fn sample_project() -> ProjectSummary<'static> {
    ProjectSummary {
        id: [1; 16],
        name: "Sample site",
        repositories: vec![],
    }
}

#[test]
fn source_navigation_respects_git_observations_and_runner_configuration() {
    let project = sample_project();
    let mut view = sample_view();
    let page = content(&project, &view);
    assert!(page.contains("href=\"/projects/01010101010101010101010101010101/builds/02020202020202020202020202020202/run\""));
    assert!(!page.contains("data-build-action") && !page.contains("Approve and build"));
    assert!(page.contains("Opening the page does not prepare, approve or run a build."));
    assert!(page.contains("8c42e1a000000000000000000000000000000000"));
    assert!(!page.contains("source-technical-details\" open"));
    assert!(page.contains("repositories/02020202020202020202020202020202/files/Cargo.toml"));
    view.branch = None;
    assert!(content(&project, &view).contains("Detached HEAD"));
    view.checks[4].outcome = "Needs changes";
    let legacy = content(&project, &view);
    assert!(
        legacy.contains("Continue to build review")
            && legacy
                .contains("web-app build review validates its supported source layout separately")
    );
    view.checks[1].outcome = "Needs changes";
    let dirty = content(&project, &view);
    assert!(
        dirty.contains("Resolve source changes") && !dirty.contains("Continue to build review")
    );
    view = sample_view();
    view.commit = None;
    assert!(content(&project, &view).contains("No commits yet"));
    assert!(!content(&project, &view).contains("Continue to build review"));
    view = sample_view();
    view.execution_available = false;
    let unconfigured = content(&project, &view);
    assert!(
        unconfigured.contains("Build runner not enabled")
            && !unconfigured.contains("Continue to build review")
    );
    view.unavailable = Some("<read failed>");
    let unavailable = content(&project, &view);
    assert!(
        unavailable.contains("&lt;read failed&gt;") && unavailable.contains("Retry source checks")
    );
    assert!(!unavailable.contains("Detached HEAD"));
}

#[test]
#[ignore = "explicit synthetic source-readiness capture; no inspection or build execution"]
fn manual_source_readiness_fixture() {
    let directory = std::env::var_os("DEVOPTION_SOURCE_CAPTURE_DIR")
        .expect("explicit existing output directory");
    let directory = std::path::Path::new(&directory);
    let project = sample_project();
    let mut view = sample_view();
    let write = |name: &str, view: &PreparationView<'_>| {
        std::fs::write(
            directory.join(name),
            crate::render_build_preparation(&project, view),
        )
        .unwrap();
    };
    write("index.html", &view);
    let source = directory
        .join("projects/01010101010101010101010101010101/builds/02020202020202020202020202020202");
    std::fs::create_dir_all(source.join("run")).unwrap();
    std::fs::write(
        source.join("index.html"),
        crate::render_build_preparation(&project, &view),
    )
    .unwrap();
    let build = super::super::BuildExecutionView {
        repository: [2; 16],
        token: None,
        review: vec![],
        running: false,
        available: true,
        message:
            "Synthetic UI preview. Real preparation requires a configured runner and owner session."
                .into(),
    };
    std::fs::write(
        source.join("run/index.html"),
        crate::render_build_execution(&project, &build),
    )
    .unwrap();
    view.headline = "Changes needed";
    view.checks[1].outcome = "Needs changes";
    view.checks[1].detail = "0 staged · 1 modified · 0 untracked · 0 conflicted. Review and commit or resolve these changes with Git, then check again.";
    write("changes.html", &view);
    view = sample_view();
    view.commit = None;
    view.headline = "Changes needed";
    view.checks[0].outcome = "Needs changes";
    view.checks[0].detail = "Create the first commit with Git, then check again.";
    write("no-commit.html", &view);
    view = sample_view();
    view.execution_available = false;
    write("unconfigured.html", &view);
    view = sample_view();
    view.branch = None;
    write("detached.html", &view);
    view = sample_view();
    view.headline = "Changes needed";
    view.checks[4].outcome = "Needs changes";
    view.checks[4].detail = "This file is not tracked. Add it and commit with Git.";
    write("library-layout.html", &view);
    view = sample_view();
    view.headline = "Source checks unavailable";
    view.unavailable = Some(
        "Repository storage or the saved checkout is unavailable. Open Code to inspect its attachment.",
    );
    view.branch = None;
    view.commit = None;
    view.checks.clear();
    view.package = None;
    write("unavailable.html", &view);
}
