//! Project-scoped pages. Navigation is ordinary links so refresh, history and
//! keyboard access work without a client-side router.
use super::{
    ProjectSummary, attachment, escape_into, identifier, project_form, repository_count,
    unavailable_attachment,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectSection {
    Overview,
    Code,
    Builds,
    Releases,
    Packages,
    Settings,
}

impl ProjectSection {
    pub const ALL: [Self; 6] = [
        Self::Overview,
        Self::Code,
        Self::Builds,
        Self::Releases,
        Self::Packages,
        Self::Settings,
    ];

    pub fn from_segment(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|section| section.segment() == value)
    }

    pub fn segment(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Code => "code",
            Self::Builds => "builds",
            Self::Releases => "releases",
            Self::Packages => "packages",
            Self::Settings => "settings",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Code => "Code",
            Self::Builds => "Builds",
            Self::Releases => "Releases",
            Self::Packages => "Packages",
            Self::Settings => "Settings",
        }
    }
}

#[cfg(test)]
pub(crate) fn content(
    project: &ProjectSummary<'_>,
    attachment_available: bool,
    section: ProjectSection,
) -> String {
    content_with_creation(project, attachment_available, false, section)
}

pub(crate) fn content_with_creation(
    project: &ProjectSummary<'_>,
    attachment_available: bool,
    creation_available: bool,
    section: ProjectSection,
) -> String {
    let id = identifier(&project.id);
    let mut html = navigation(project, section);
    match section {
        ProjectSection::Overview => {
            html.push_str(r#"<section aria-labelledby="project-overview-title"><div class="section-heading"><h2 id="project-overview-title">Repositories</h2>"#);
            html.push_str(&format!(r#"<a class="text-button" href="/projects/{id}/code">Open Code <span aria-hidden="true">→</span></a></div>"#));
            if project.repositories.is_empty() {
                html.push_str(r#"<div class="empty-state"><h3>No repositories yet.</h3><p>This project is ready for its source code.</p></div>"#);
            } else {
                html.push_str(r#"<p class="project-summary-count">"#);
                repository_count(&mut html, project);
                html.push_str(r#"</p><div class="project-table-wrap"><table class="project-table"><caption class="visually-hidden">Repositories for "#);
                escape_into(&mut html, project.name);
                html.push_str("</caption><thead><tr><th scope=\"col\">Repository</th><th scope=\"col\">Default checkout</th></tr></thead><tbody>");
                for repository in &project.repositories {
                    let repository_id = identifier(&repository.id);
                    html.push_str(&format!(r#"<tr><th scope="row"><a href="/projects/{id}/repositories/{repository_id}/files">"#));
                    escape_into(&mut html, repository.name);
                    html.push_str("</a></th><td>");
                    if let Some(path) = repository.default_checkout { escape_into(&mut html, path); } else { html.push_str("Not attached"); }
                    html.push_str("</td></tr>");
                }
                html.push_str("</tbody></table></div>");
            }
            html.push_str("</section>");
        }
        ProjectSection::Code => html.push_str(&repositories(project, attachment_available, creation_available)),
        ProjectSection::Settings => {
            html.push_str(r#"<section class="project-settings" aria-labelledby="project-settings-title"><h2 id="project-settings-title">Project details</h2>"#);
            html.push_str(&project_form(Some(project)));
            html.push_str("<noscript>JavaScript is required to rename a project.</noscript></section>");
        }
        ProjectSection::Builds => html.push_str(&super::builds::list(project, attachment_available)),
        ProjectSection::Releases => html.push_str(r#"<section class="empty-state"><h2>Releases are not available yet</h2><p>Release creation and deployment are not implemented for this project.</p></section>"#),
        ProjectSection::Packages => html.push_str(r#"<section class="empty-state"><h2>Packages are not available yet</h2><p>Package management is not implemented for this project.</p></section>"#),
    }
    html
}

pub(super) fn navigation(project: &ProjectSummary<'_>, section: ProjectSection) -> String {
    let id = identifier(&project.id);
    let mut html =
        String::from(r#"<nav class="project-navigation" aria-label="Project sections">"#);
    for item in ProjectSection::ALL {
        let path = if item == ProjectSection::Overview {
            format!("/projects/{id}")
        } else {
            format!("/projects/{id}/{}", item.segment())
        };
        let current = if section == item {
            r#" aria-current="page""#
        } else {
            ""
        };
        html.push_str(&format!(
            r#"<a href="{path}"{current}>{}</a>"#,
            item.label()
        ));
    }
    html.push_str("</nav>");
    html
}

pub(super) fn repositories(
    project: &ProjectSummary<'_>,
    attachment_available: bool,
    creation_available: bool,
) -> String {
    let id = identifier(&project.id);
    let mut html = String::from(
        r#"<section class="repository-details" aria-labelledby="repositories-title"><div class="section-heading"><h2 id="repositories-title">Repository details</h2>"#,
    );
    html.push_str(r#"<div class="page-actions">"#);
    if attachment_available {
        html.push_str(r#"<button class="text-button" type="button" data-open-dialog="repository-dialog" disabled><span aria-hidden="true">＋</span>Attach repository</button>"#);
    }
    if creation_available {
        html.push_str(r#"<button class="action-button primary" type="button" data-open-dialog="repository-create-dialog" disabled><span aria-hidden="true">＋</span>New repository</button>"#);
    }
    html.push_str("</div></div>");
    if project.repositories.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No repositories yet.</h3><p>Repositories registered for this project will appear here.</p></div>"#);
    } else {
        html.push_str(r#"<ul class="repository-list" aria-label="Repositories">"#);
        for repository in &project.repositories {
            let repository_id = identifier(&repository.id);
            html.push_str(&format!(r#"<li id="repository-{repository_id}"><div class="repository-summary"><span class="repository-icon" aria-hidden="true">{}</span><div><h3>"#, crate::icon("M6 3v12a4 4 0 0 0 8 0V9M3 6a3 3 0 1 0 6 0a3 3 0 0 0-6 0M11 6a3 3 0 1 0 6 0a3 3 0 0 0-6 0")));
            escape_into(&mut html, repository.name);
            html.push_str("</h3><p>");
            if let Some(path) = repository.default_checkout {
                html.push_str("Default checkout: <code>");
                escape_into(&mut html, path);
                html.push_str("</code>");
            } else {
                html.push_str("Not attached");
            }
            html.push_str("</p></div></div>");
            if attachment_available && repository.default_checkout.is_some() {
                html.push_str(
                    r#"<div class="repository-inspection" role="group" aria-label="Status for "#,
                );
                escape_into(&mut html, repository.name);
                html.push_str(&format!(r#"" data-repository-status-view data-status-url="/projects/{id}/repositories/{repository_id}/status">
                  <dl data-status-fields hidden><div><dt>Branch</dt><dd data-status-branch></dd></div><div><dt>Commit</dt><dd class="commit-value" data-status-commit></dd></div><div><dt>Working tree</dt><dd data-status-changes></dd></div></dl>
                  <div class="status-actions"><button class="text-button" type="button" data-status-refresh disabled>View status</button><p data-status-note role="status" aria-live="polite">Enable JavaScript to read repository status.</p></div>
                </div>"#));
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    if !creation_available {
        html.push_str(r#"<p class="repository-notice">Repository creation is not configured on this server. An administrator can select managed repository storage.</p>"#);
    }
    if !attachment_available {
        unavailable_attachment(&mut html);
    }
    html.push_str("</section>");
    if creation_available {
        html.push_str(&super::dialog("repository-create-dialog", "New repository", &format!(r#"
<form id="repository-create-form" class="project-form" data-project="{id}" aria-labelledby="repository-create-dialog-title">
<label for="repository-create-name">Repository name</label>
<p id="repository-create-hint">Create an empty Git repository on this server. Names are unique within this project. Use 1–64 letters, numbers, hyphens or underscores, starting with a letter or number.</p>
<input id="repository-create-name" name="name" type="text" required maxlength="64" pattern="[A-Za-z0-9][A-Za-z0-9_\-]*" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="repository-create-hint repository-create-status" disabled>
<p id="repository-create-status" role="status" aria-live="polite">Checking your session…</p>
<a id="repository-create-reload" href="/projects/{id}/code" hidden>Reload Code</a>
<a id="repository-create-signin" href="/sign-in" hidden>Check account</a>
<div class="form-actions"><button class="action-button" type="button" data-close-dialog>Cancel</button><button id="repository-create-submit" class="action-button primary" type="submit" disabled>Create repository</button></div>
</form>"#)));
        html.push_str("<noscript><p>JavaScript is required to create a repository.</p></noscript>");
    }
    if attachment_available {
        attachment(&mut html, &id);
    }
    html
}
