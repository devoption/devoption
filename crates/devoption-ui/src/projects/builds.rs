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
    pub branch: Option<&'a str>,
    pub execution_available: bool,
    pub package: Option<(&'a str, &'a str, usize)>,
    pub checks: Vec<PreparationCheck<'a>>,
    pub unavailable: Option<&'a str>,
}

const LIMITATION: &str = "These checks do not approve or run a build.";

pub(super) fn list(project: &ProjectSummary<'_>, available: bool) -> String {
    let mut html = String::from(
        r#"<section class="build-preparation" aria-labelledby="preparation-title"><h2 id="preparation-title">Prepare a build</h2><p>Inspect an attached repository’s current source. The build runner is not enabled on this server.</p>"#,
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
    let repository = identifier(&view.repository_id);
    let url = format!("/projects/{id}/builds/{repository}");
    let git_ready = view.unavailable.is_none()
        && view.commit.is_some()
        && ["Commit", "Working tree"].iter().all(|label| {
            view.checks
                .iter()
                .any(|c| c.label == *label && c.outcome == "Passed")
        });
    let initial_passed =
        !view.checks.is_empty() && view.checks.iter().all(|c| c.outcome == "Passed");
    let mut files = String::new();
    let mut html = super::workspace::navigation(project, ProjectSection::Builds);
    html.push_str(r#"<section class="source-readiness" aria-labelledby="source-readiness-title"><header class="source-readiness-heading"><h2 id="source-readiness-title">Review your source</h2><p>Inspect the checkout before preparing a build.</p></header><ol class="source-readiness-stages" aria-label="Delivery stages"><li aria-current="step">Source <span>Inspect checkout</span></li><li>Build <span>Review and approve</span></li><li>Release <span>Save a successful build</span></li><li>Deploy <span>Separate approval</span></li></ol><div class="source-readiness-layout"><div class="source-readiness-main"><p class="source-readiness-label">Repository</p><h3>"#);
    escape_into(&mut html, view.repository_name);
    html.push_str(r#"</h3><dl class="source-identity"><div><dt>Branch</dt><dd>"#);
    escape_into(
        &mut html,
        if view.unavailable.is_some() {
            "Unavailable"
        } else {
            view.branch.unwrap_or("Detached HEAD")
        },
    );
    html.push_str("</dd></div><div><dt>Commit</dt><dd><code>");
    escape_into(
        &mut html,
        &view
            .commit
            .map(|value| value.chars().take(12).collect::<String>())
            .unwrap_or_else(|| {
                if view.unavailable.is_some() {
                    "Unavailable"
                } else {
                    "No commits yet"
                }
                .into()
            }),
    );
    html.push_str(r#"</code></dd></div></dl><p class="source-readiness-status" role="status">"#);
    escape_into(&mut html, view.headline);
    html.push_str("</p>");
    if let Some(message) = view.unavailable {
        html.push_str("<p class=\"source-readiness-problem\">");
        escape_into(&mut html, message);
        html.push_str("</p>");
    } else {
        html.push_str(r#"<ul class="source-observations" aria-label="Source observations">"#);
        for label in ["Commit", "Working tree", "Cargo inputs"] {
            if let Some(check) = view.checks.iter().find(|check| check.label == label) {
                let summary = PreparationCheck {
                    label: check.label,
                    outcome: check.outcome,
                    detail: if label == "Cargo inputs" && check.outcome == "Passed" {
                        "Manifest and lockfile checks passed; dependencies are not verified."
                    } else {
                        check.detail
                    },
                };
                check_row(
                    &mut html,
                    &summary,
                    if label == "Cargo inputs" {
                        "Initial build inputs"
                    } else {
                        label
                    },
                    None,
                );
            }
        }
        html.push_str("</ul>");
        files.push_str(r#"<section class="source-files-panel" aria-labelledby="source-files-title"><h3 id="source-files-title">Inspected files</h3><ul class="source-files" aria-label="Inspected source files">"#);
        for check in &view.checks {
            if matches!(
                check.label,
                "Cargo.toml" | "Cargo.lock" | "src/main.rs" | "src/lib.rs"
            ) {
                let file_url = format!(
                    "/projects/{id}/repositories/{repository}/files/{}",
                    check.label
                );
                check_row(
                    &mut files,
                    check,
                    check.label,
                    (check.outcome == "Passed").then_some(file_url.as_str()),
                );
            }
        }
        files.push_str("</ul></section>");
    }
    html.push_str(&format!(r#"<div class="source-refresh"><a class="text-button" href="{url}">Refresh source</a><p>Observed on this page load.</p></div></div><aside class="source-next" aria-labelledby="source-next-title">"#));
    if view.unavailable.is_some() {
        html.push_str(&format!(r#"<h3 id="source-next-title">Source could not be checked</h3><p>Inspect the checkout attachment in Code, then refresh source. An unavailable read is not a clean result.</p><a class="action-button primary" href="{url}">Retry source checks</a>"#));
    } else if !git_ready {
        html.push_str(&format!(r#"<h3 id="source-next-title">Resolve source changes</h3><p>Review the commit and working-tree observations. Update or commit this checkout using your Git tools, then refresh source.</p><a class="action-button primary" href="/projects/{id}/code">Open Code</a>"#));
    } else if view.execution_available {
        html.push_str(r#"<h3 id="source-next-title">Next: prepare a build</h3><p>Continue to the web-app build page to capture committed source and review the build settings.</p>"#);
        if !initial_passed {
            html.push_str(r#"<p class="source-readiness-problem">The initial file checks need attention. They use a binary-source format; web-app build review validates its supported source layout separately.</p>"#);
        }
        html.push_str(&format!(r#"<a class="action-button primary" href="{url}/run">Continue to build review</a><p class="source-next-note">Opening the page does not prepare, approve or run a build.</p>"#));
    } else {
        html.push_str(&format!(r#"<h3 id="source-next-title">Build runner not enabled</h3><p>Source inspection is available. An administrator must enable the Linux web-app build runner before a build review can be prepared.</p><a class="text-button" href="/projects/{id}/code">Open Code</a>"#));
    }
    html.push_str(r#"<div class="source-git-guidance"><h4>Pushing a change?</h4><p>DevOption reads the attached server checkout. Update and commit it using your Git tools, then refresh source here.</p></div></aside>"#);
    html.push_str(&files);
    html.push_str(r#"</div><p class="source-observation-note">These are current working-tree observations, not a verified immutable source snapshot. These checks do not approve or run a build.</p><details class="source-technical-details"><summary>Repository details and initial source format</summary><dl class="build-identities"><div><dt>Observed HEAD</dt><dd><code>"#);
    escape_into(
        &mut html,
        view.commit.unwrap_or(if view.unavailable.is_some() {
            "Unavailable"
        } else {
            "No commits yet"
        }),
    );
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
    html.push_str(
        r#"</dl><ol class="preparation-checks" aria-label="Detailed source check results">"#,
    );
    for check in &view.checks {
        check_row(&mut html, check, check.label, None);
    }
    html.push_str(r#"</ol><p>The initial checks inspect a supported standalone package: tracked Cargo.toml, version-4 Cargo.lock and the declared nonempty src/main.rs or src/lib.rs. Each inspected text file is limited to 64 KiB. These checks do not verify compiler validity, dependency availability or complete committed bytes.</p><p>The opt-in web-app build review checks its supported library source, toolchain and prepared dependencies independently before approval. A passing source report is not build admission.</p><a class="text-button" href="https://github.com/devoption/devoption/blob/main/docs/build-preparation.md" target="_blank" rel="noopener noreferrer">Source inspection limits</a></details>"#);
    html.push_str(&format!(r#"<footer class="source-readiness-links"><a class="text-button" href="/projects/{id}/builds">Back to Builds</a><a class="text-button" href="/projects/{id}/code">Browse source in Code</a></footer></section>"#));
    html
}

fn check_row(html: &mut String, check: &PreparationCheck<'_>, label: &str, link: Option<&str>) {
    html.push_str("<li><div class=\"source-check-heading\"><strong>");
    if let Some(link) = link {
        html.push_str("<a href=\"");
        escape_into(html, link);
        html.push_str("\">");
    }
    escape_into(html, label);
    if link.is_some() {
        html.push_str("</a>");
    }
    html.push_str("</strong><span>");
    escape_into(html, check.outcome);
    html.push_str("</span></div>");
    if link.is_none() {
        html.push_str("<p>");
        escape_into(html, check.detail);
        html.push_str("</p>");
    }
    html.push_str("</li>");
}

#[cfg(test)]
mod tests;
