use super::render::action_status;
use super::{ProjectSection, ProjectSummary, escape_into, identifier};
pub struct BuildExecutionView {
    pub repository: [u8; 16],
    pub library: bool,
    pub token: Option<String>,
    pub review: Vec<(String, String)>,
    pub running: bool,
    pub available: bool,
    pub message: String,
}
pub(crate) fn content(project: &ProjectSummary<'_>, view: &BuildExecutionView) -> String {
    let base = format!(
        "/projects/{}/builds/{}",
        identifier(&project.id),
        identifier(&view.repository)
    );
    let ready = view.token.is_some() && !view.running;
    let mut html = super::workspace::navigation(project, ProjectSection::Builds);
    let description = if view.library {
        "Compile and test the committed library, then save its source package."
    } else if ready {
        "Build the committed app into a web app component (Wasm)."
    } else {
        "Prepare a supported web app or Rust library for review."
    };
    html.push_str(&format!(r#"<section class="build-review" id="build-execution" data-base="{base}" aria-labelledby="build-review-title"><header class="build-review-heading"><h2 id="build-review-title">Review your build</h2><p>{description}</p></header>"#));
    if ready && view.library {
        html.push_str(r#"<ol class="build-review-stages" aria-label="Library build stages"><li>Source <span>Captured</span></li><li aria-current="step">Build <span>Compile and test</span></li><li>Package <span>Save source</span></li></ol>"#);
    } else if ready {
        html.push_str(r#"<ol class="build-review-stages" aria-label="Delivery stages"><li>Source <span>Captured</span></li><li aria-current="step">Build <span>Review and approve</span></li><li>Release <span>Next, after success</span></li><li>Deploy <span>Separate approval</span></li></ol>"#);
    }
    html.push_str(r#"<div class="build-review-layout"><div class="build-review-main">"#);
    if ready {
        html.push_str(r#"<h3 class="build-review-state">Ready for your approval</h3>"#);
    }
    action_status(&mut html, "build-action-status", &view.message);
    if view.running {
        html.push_str(r#"<button class="action-button" data-build-action="cancel">Cancel build</button><p>Cancellation is complete only when the saved attempt says Cancelled.</p>"#);
    } else if let Some(token) = &view.token {
        let source = project
            .repositories
            .iter()
            .find(|r| r.id == view.repository);
        html.push_str(r#"<dl class="build-review-summary">"#);
        summary_row(
            &mut html,
            "Source",
            source.map_or("Attached repository", |r| r.name),
        );
        let commit = field(view, "Commit").map(|value| value.chars().take(12).collect::<String>());
        summary_row(
            &mut html,
            "Commit",
            commit.as_deref().unwrap_or("Unavailable"),
        );
        summary_row(
            &mut html,
            "Output",
            if view.library {
                "Library source package (.tar)"
            } else {
                "Web app component (Wasm)"
            },
        );
        if view.library {
            summary_row(
                &mut html,
                "Validation",
                "Native compilation and unit tests; WASIp2 compilation",
            );
        }
        if let Some(limits) = field(view, "Build limits") {
            summary_row(&mut html, "Build limits", limits);
        }
        if let Some(network) = field(view, "Network access") {
            summary_row(&mut html, "Network access", network);
        }
        html.push_str(r#"</dl><details class="build-review-details"><summary>Build command, inputs &amp; technical details</summary><dl class="build-identities">"#);
        for (label, value) in &view.review {
            if label == "Build limits" || label == "Network access" {
                continue;
            }
            html.push_str("<div><dt>");
            escape_into(&mut html, label);
            html.push_str("</dt><dd><pre>");
            escape_into(&mut html, value);
            html.push_str("</pre></dd></div>");
        }
        if view.library {
            html.push_str(r#"</dl></details></div><aside class="build-review-next" aria-labelledby="build-next-title"><h3 id="build-next-title">What happens next</h3><ol><li><strong>Compile and test the library</strong><p>Run the reviewed native build and unit tests, then check WASIp2 compilation.</p></li><li><strong>Save the source package</strong><p>After success, download the verified source archive and build receipt from the saved attempt.</p></li><li><strong>Distribute separately</strong><p>This build does not publish a crate or deploy an app. Cross-project registry distribution is not available yet.</p></li></ol></aside>"#);
        } else {
            html.push_str(r#"</dl></details></div><aside class="build-review-next" aria-labelledby="build-next-title"><h3 id="build-next-title">What happens next</h3><ol><li><strong>Compile the reviewed source</strong><p>Use the reviewed command and limits in an isolated worker.</p></li><li><strong>Save the build result</strong><p>Check saved status and output in build history.</p></li><li><strong>Review a release separately</strong><p>Save a successful build in Releases. Deployment requires its own approval.</p></li></ol></aside>"#);
        }
        html.push_str(r#"</div><div class="build-review-approval"><div><strong>Only approve source and dependencies you trust.</strong><p>Approval is single-use and expires after five minutes. Changed source or prepared inputs require a new review.</p></div><div class="build-review-actions"><button class="action-button primary" data-build-action="approve" data-token=""#);
        escape_into(&mut html, token);
        html.push_str(r#"">Approve and build</button><button class="text-button" data-build-action="prepare">Prepare new review</button></div></div>"#);
    } else if view.available {
        html.push_str(r#"<button class="action-button primary" data-build-action="prepare">Prepare build review</button>"#);
    }
    if !ready {
        html.push_str("</div></div>");
    }
    html.push_str(&format!(r#"<footer class="build-review-links"><a class="text-button" href="/projects/{}/builds">Open build history and logs</a><a class="text-button" href="{base}">Check source</a></footer><noscript><p>JavaScript is required to prepare, approve or cancel a build.</p></noscript></section>"#, identifier(&project.id)));
    html
}
fn field<'a>(view: &'a BuildExecutionView, label: &str) -> Option<&'a str> {
    view.review
        .iter()
        .find(|(key, _)| key == label)
        .map(|(_, value)| value.as_str())
}
fn summary_row(html: &mut String, label: &str, value: &str) {
    html.push_str("<div><dt>");
    escape_into(html, label);
    html.push_str("</dt><dd>");
    escape_into(html, value);
    html.push_str("</dd></div>");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_escapes_inputs_and_separates_approval_from_preparation() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "<project>",
            repositories: vec![],
        };
        let mut view = BuildExecutionView {
            repository: [2; 16],
            library: false,
            token: None,
            review: vec![],
            running: false,
            available: false,
            message: "<unavailable>".into(),
        };
        let unavailable = content(&project, &view);
        assert!(unavailable.contains("&lt;unavailable&gt;"));
        assert!(!unavailable.contains("data-build-action"));
        view.available = true;
        assert!(content(&project, &view).contains("Prepare build review"));
        view.token = Some("ab".repeat(16));
        view.review = vec![("<input>".into(), "<script>source</script>".into()); 6];
        let review = content(&project, &view);
        assert!(review.contains("&lt;script&gt;source&lt;/script&gt;"));
        assert!(review.contains("Build command, inputs &amp; technical details"));
        assert!(review.contains("Approve and build"));
        assert!(review.contains("Prepare new review"));
        view.running = true;
        let active = content(&project, &view);
        assert!(active.contains("Cancel build"));
        assert!(!active.contains("data-token"));
        assert!(!active.contains("Ready for your approval"));
        assert!(!active.contains("aria-current=\"step\""));
    }

    #[test]
    fn review_summary_keeps_exact_inputs_in_a_closed_disclosure() {
        let (project, view) = capture_view();
        let html = content(&project, &view);
        let summary = html
            .split("<dl class=\"build-review-summary\">")
            .nth(1)
            .unwrap()
            .split("</dl>")
            .next()
            .unwrap();
        assert!(summary.contains("sample-site"));
        assert!(summary.contains("50ade5be0f9e"));
        assert!(!summary.contains(&"ab".repeat(32)));
        assert!(!summary.contains("/toolchain/bin/cargo"));
        let details = html
            .split("<details class=\"build-review-details\">")
            .nth(1)
            .unwrap()
            .split("</details>")
            .next()
            .unwrap();
        for (label, value) in &view.review {
            if label != "Build limits" && label != "Network access" {
                assert!(details.contains(label));
                assert!(details.contains(value));
            }
        }
        assert!(!html.contains("<details class=\"build-review-details\" open"));
        assert!(html.contains("class=\"action-button primary\" data-build-action=\"approve\""));
        assert!(html.contains("Deployment requires its own approval"));
        assert!(html.contains("Approval is single-use and expires after five minutes"));
    }

    fn capture_view() -> (ProjectSummary<'static>, BuildExecutionView) {
        let project = ProjectSummary {
            id: [1; 16],
            name: "Wasm web app",
            repositories: vec![super::super::RepositorySummary {
                id: [2; 16],
                name: "sample-site",
                default_checkout: None,
            }],
        };
        let view = BuildExecutionView {
            repository: [2; 16],
            library: false,
            token: Some("cd".repeat(16)),
            running: false,
            available: true,
            message: "The source snapshot is prepared. Review the details before approving.".into(),
            review: vec![
                ("Build limits".into(), "1 CPU · 512 MiB memory · 60-second limit".into()),
                ("Network access".into(), "Off".into()),
                ("Commit".into(), "50ade5be0f9eaf56f039815032518ca9e358f669".into()),
                ("Command inside isolated worker".into(), "/toolchain/bin/cargo build --release --locked --offline --target wasm32-wasip2 --package sample-site --lib".into()),
                ("Working directory".into(), "/source (read-only owned snapshot)".into()),
                ("Artifact".into(), "wasm32-wasip2/release/sample_site.wasm".into()),
                ("Policy".into(), "WasmBuildV1: 60 seconds; 512 MiB RAM; no swap; one CPU; 64 tasks; 256 MiB job storage (240 MiB scratch; 16 MiB reserved for staged inputs, artifact and logs); 1 MiB logs; 4 MiB component. No network. Target scratch permits dependency build scripts.".into()),
                ("Toolchain SHA-256".into(), "ab".repeat(32)),
                ("Dependency bundle SHA-256".into(), "12".repeat(32)),
                ("Source SHA-256".into(), "34".repeat(32)),
                ("Configuration SHA-256".into(), "56".repeat(32)),
                ("Execution UID".into(), "20000".into()),
                ("Environment (all values)".into(), "CARGO_HOME=/cargo-home\nCARGO_NET_OFFLINE=true".into()),
            ],
        };
        (project, view)
    }

    fn library_capture_view() -> (ProjectSummary<'static>, BuildExecutionView) {
        let (mut project, mut view) = capture_view();
        project.name = "Reusable library";
        project.repositories[0].name = "shared-components";
        view.library = true;
        for (key, value) in &mut view.review {
            match key.as_str() {
                "Command inside isolated worker" => *value = [
                    "/toolchain/bin/cargo build --release --locked --offline --package shared-components --lib",
                    "/toolchain/bin/cargo test --release --locked --offline --package shared-components --lib",
                    "/toolchain/bin/cargo build --release --locked --offline --package shared-components --lib --target wasm32-wasip2",
                ].join("\n"),
                "Artifact" => *value = "source.tar".into(),
                "Policy" => *value = "LibraryBuildV1: native compilation and unit tests, then WASIp2 compilation; one shared 60-second limit; 512 MiB RAM; no network; 4 MiB source package.".into(),
                _ => {},
            }
        }
        (project, view)
    }

    #[test]
    fn library_review_identifies_all_commands_and_source_output_without_deployment_steps() {
        let (project, view) = library_capture_view();
        let html = content(&project, &view);
        assert!(html.contains("Library source package (.tar)"));
        assert!(html.contains("Native compilation and unit tests; WASIp2 compilation"));
        for (_, value) in &view.review {
            assert!(html.contains(value));
        }
        assert!(html.contains("does not publish a crate or deploy an app"));
        assert!(!html.contains("Deployment requires its own approval"));
        assert!(!html.contains("<li>Deploy"));
        assert!(html.contains("data-build-action=\"approve\""));
    }

    #[test]
    #[ignore = "explicit synthetic build review screenshot fixture; executes no build"]
    fn manual_build_review_fixture() {
        let directory = std::env::var_os("DEVOPTION_BUILD_REVIEW_CAPTURE_DIR")
            .expect("explicit existing output directory");
        let directory = std::path::Path::new(&directory);
        let (library, library_view) = library_capture_view();
        std::fs::write(
            directory.join("library.html"),
            crate::render_build_execution(&library, &library_view),
        )
        .unwrap();
        let (project, mut view) = capture_view();
        std::fs::write(
            directory.join("index.html"),
            crate::render_build_execution(&project, &view),
        )
        .unwrap();
        view.token = None;
        view.review.clear();
        view.message =
            "Prepare the committed web app for review. Preparation does not run Cargo.".into();
        std::fs::write(
            directory.join("prepare.html"),
            crate::render_build_execution(&project, &view),
        )
        .unwrap();
        view.running = true;
        view.available = false;
        view.message =
            "Build running. Open build history for saved output, or request cancellation.".into();
        std::fs::write(
            directory.join("running.html"),
            crate::render_build_execution(&project, &view),
        )
        .unwrap();
        view.running = false;
        view.message =
            "Install and explicitly enable the Linux build runner to build this app.".into();
        std::fs::write(
            directory.join("unavailable.html"),
            crate::render_build_execution(&project, &view),
        )
        .unwrap();
    }
}
