use super::{ProjectSection, ProjectSummary, escape_into, identifier};

pub struct PreparationCheck<'a> {
    pub label: &'a str,
    pub outcome: &'static str,
    pub detail: &'a str,
}

pub struct PreparationView<'a> {
    pub repository_id: [u8; 16],
    pub repository_name: &'a str,
    pub headline: &'static str,
    pub commit: Option<&'a str>,
    pub package: Option<(&'a str, &'a str, usize)>,
    pub checks: Vec<PreparationCheck<'a>>,
    pub unavailable: Option<&'a str>,
}

const LIMITATION: &str =
    "Build execution is not available yet. Source checks do not approve or run a build.";

pub(super) fn list(project: &ProjectSummary<'_>, available: bool) -> String {
    let mut html = String::from(
        r#"<section class="build-preparation" aria-labelledby="preparation-title"><h2 id="preparation-title">Prepare a build</h2><p>Check an attached repository's current source before the future build workflow.</p>"#,
    );
    html.push_str(&format!("<p class=\"preparation-note\">{LIMITATION}</p>"));
    if project.repositories.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No repositories yet</h3><p>Open Code to create or attach a repository.</p></div>"#);
    } else if !available {
        html.push_str(
            "<p>Repository storage is not configured. Source checks are unavailable.</p>",
        );
    } else {
        html.push_str(
            r#"<ul class="preparation-repositories" aria-label="Repositories to check">"#,
        );
        for repository in &project.repositories {
            html.push_str("<li><h3>");
            escape_into(&mut html, repository.name);
            html.push_str("</h3>");
            if repository.default_checkout.is_some() {
                html.push_str(&format!(r#"<a class="text-button" href="/projects/{}/builds/{}">Check source <span class="visually-hidden">for "#, identifier(&project.id), identifier(&repository.id)));
                escape_into(&mut html, repository.name);
                html.push_str("</span><span aria-hidden=\"true\">→</span></a>");
            } else {
                html.push_str("<p>No default checkout is attached.</p>");
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    html.push_str(&format!(
        r#"<a class="text-button" href="/projects/{}/code">Open Code</a></section>"#,
        identifier(&project.id)
    ));
    html
}

pub(crate) fn content(project: &ProjectSummary<'_>, view: &PreparationView<'_>) -> String {
    let id = identifier(&project.id);
    let url = format!("/projects/{id}/builds/{}", identifier(&view.repository_id));
    let mut html = super::workspace::navigation(project, ProjectSection::Builds);
    html.push_str(&format!(r#"<section class="build-preparation" aria-labelledby="preparation-title"><a class="text-button" href="/projects/{id}/builds">← Back to Builds</a><h2 id="preparation-title">"#));
    escape_into(&mut html, view.repository_name);
    html.push_str(" · Source checks</h2><p class=\"preparation-result\" role=\"status\">");
    escape_into(&mut html, view.headline);
    html.push_str(&format!(
        "</p><p class=\"preparation-note\">{LIMITATION}</p>"
    ));
    if let Some(message) = view.unavailable {
        html.push_str("<p>");
        escape_into(&mut html, message);
        html.push_str("</p>");
    } else {
        html.push_str(r#"<dl class="preparation-summary"><div><dt>Observed HEAD</dt><dd><code>"#);
        escape_into(&mut html, view.commit.unwrap_or("No commits yet"));
        html.push_str("</code></dd></div>");
        if let Some((package, version, count)) = view.package {
            html.push_str("<div><dt>Package</dt><dd>");
            escape_into(&mut html, package);
            html.push(' ');
            escape_into(&mut html, version);
            html.push_str(&format!(
                "</dd></div><div><dt>Locked registry dependencies</dt><dd>{count}</dd></div>"
            ));
        }
        html.push_str(r#"</dl><ol class="preparation-checks" aria-label="Source check results">"#);
        for check in &view.checks {
            html.push_str("<li><div class=\"preparation-check-heading\"><h3>");
            escape_into(&mut html, check.label);
            html.push_str("</h3><span>");
            escape_into(&mut html, check.outcome);
            html.push_str("</span></div><p>");
            escape_into(&mut html, check.detail);
            html.push_str("</p></li>");
        }
        html.push_str("</ol><p class=\"preparation-note\">These observations describe current working-tree files, not a verified immutable source snapshot. Refreshing checks the repository again.</p>");
    }
    html.push_str(&format!(r#"<div class="preparation-actions"><a class="action-button" href="{url}">Check again</a><a class="text-button" href="/projects/{id}/code">Open Code</a></div><details class="preparation-help"><summary>Supported source format</summary><p>One standalone Rust package with a tracked Cargo.toml, version-4 Cargo.lock and nonempty src/main.rs. The manifest must declare name, version, edition (2021 or 2024) and build = false. Direct dependencies must use exact version pins; workspaces, path and Git dependencies are not supported by this check.</p><p>Each file is limited to 64 KiB. Cargo and the compiler are not run. Complete source verification, toolchain selection, dependency preparation and build approval are still unavailable.</p></details></section>"#));
    html
}

#[cfg(test)]
mod tests {
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
        assert!(html.contains("Check again</a>"));
        assert!(!html.contains("<button") || !html.contains(">Run build</button>"));
        assert!(list(&project, true).contains("No repositories yet"));
    }
}
