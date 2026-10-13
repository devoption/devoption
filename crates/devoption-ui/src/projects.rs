mod builds;
pub(super) use builds::content as preparation_content;
pub use builds::{PreparationCheck, PreparationView};
mod files;
pub(super) use files::content as files_content;
pub use files::{FileEntry, FilePreview, RepositoryFilesView};
mod workspace;
pub use workspace::ProjectSection;
#[cfg(test)]
pub(super) use workspace::content as detail;
pub(super) use workspace::content_with_creation as detail_with_creation;

/// Display-only inputs prepared by the server from a validated catalog snapshot.
pub struct ProjectSummary<'a> {
    pub id: [u8; 16],
    pub name: &'a str,
    pub repositories: Vec<RepositorySummary<'a>>,
}

pub struct RepositorySummary<'a> {
    pub id: [u8; 16],
    pub name: &'a str,
    pub default_checkout: Option<&'a str>,
}

pub(super) fn content(projects: &[ProjectSummary<'_>], _attachment_available: bool) -> String {
    let mut html = String::from(
        r#"<section class="projects-section" aria-labelledby="projects-title"><div class="section-heading"><h2 id="projects-title">All projects</h2>"#,
    );
    if !projects.is_empty() {
        html.push_str(r#"<div id="project-filter" class="project-filter" role="search" aria-label="Saved projects" hidden>
          <label class="visually-hidden" for="project-query">Filter projects by name</label>
          <input id="project-query" type="search" placeholder="Find a project…" autocomplete="off" aria-controls="saved-projects" aria-describedby="project-filter-status">
          <button id="project-filter-clear" class="text-button" type="button" disabled>Clear filter</button>
          <p id="project-filter-status" role="status" aria-live="polite" aria-atomic="true"></p>
        </div>"#);
    }
    html.push_str("</div>");
    if projects.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No projects yet</h3><p>Create your first project to organize your software.</p></div>"#);
    } else {
        html.push_str(
            r#"<ul id="saved-projects" class="project-list" aria-label="Saved projects">"#,
        );
        for project in projects {
            let id = identifier(&project.id);
            html.push_str(&format!(r#"<li data-project-item aria-labelledby="project-{id}"><a class="project-row" href="/projects/{id}"><span class="project-symbol" aria-hidden="true">{}</span><div class="project-copy"><h3 id="project-{id}" data-project-name>"#, crate::icon("M3 7V4h6l3 3h9v13H3Z")));
            escape_into(&mut html, project.name);
            html.push_str("</h3><p>");
            if project.repositories.is_empty() {
                html.push_str("No repositories yet");
            } else {
                for (index, repository) in project.repositories.iter().enumerate() {
                    if index > 0 {
                        html.push_str(" · ");
                    }
                    escape_into(&mut html, repository.name);
                }
            }
            html.push_str("</p></div><span class=\"project-count\">");
            repository_count(&mut html, project);
            html.push_str(r#"</span><span class="row-arrow" aria-hidden="true">›</span></a></li>"#);
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html.push_str(&dialog(
        "project-dialog",
        "New project",
        &project_form(None),
    ));
    html.push_str("<noscript>JavaScript is required to create a project.</noscript>");
    html
}

// The form has no dependency on its current dialog container; a longer future
// setup flow can render it in a page without changing its submission contract.
fn project_form(project: Option<&ProjectSummary<'_>>) -> String {
    let (attributes, hint, submit) = if let Some(project) = project {
        let id = identifier(&project.id);
        (
            format!(r#"data-project="{id}" action="/projects/{id}/name""#),
            "Attached repositories keep their names and checkout paths.",
            "Save name",
        )
    } else {
        (
            r#"action="/projects""#.to_owned(),
            "Your new project opens after saving.",
            "Create project",
        )
    };
    let mut html = format!(
        r#"<form id="project-form" class="project-form" {attributes} method="post">
      <label for="project-name">Project name</label><p id="project-hint">{hint}</p>
      <input id="project-name" name="name" type="text" required maxlength="128" autocomplete="off" aria-describedby="project-hint project-status" disabled"#
    );
    if let Some(project) = project {
        html.push_str(r#" value=""#);
        escape_into(&mut html, project.name);
        html.push('"');
    }
    let cancel = if project.is_none() {
        r#"<button class="action-button" type="button" data-close-dialog>Cancel</button>"#
    } else {
        ""
    };
    html.push_str(&format!(r#"><p id="project-status" role="status" aria-live="polite"></p>
      <div class="form-actions">{cancel}<button id="project-submit" class="action-button primary" type="submit" disabled>{submit}</button></div></form>"#));
    html
}

fn identifier(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn repository_count(html: &mut String, project: &ProjectSummary<'_>) {
    let count = project.repositories.len();
    let label = if count == 1 {
        "repository"
    } else {
        "repositories"
    };
    html.push_str(&format!("{count} {label}"));
}

fn unavailable_attachment(html: &mut String) {
    html.push_str(r#"<p class="repository-notice">Repository attachment is not configured on this server. A server administrator must select a trusted repository root before you can attach a checkout.</p>"#);
}

fn dialog(id: &str, title: &str, content: &str) -> String {
    format!(
        r#"<dialog id="{id}" class="form-dialog" aria-labelledby="{id}-title"><div class="dialog-heading"><h2 id="{id}-title" tabindex="-1">{title}</h2><button class="dialog-close" type="button" data-close-dialog aria-label="Close {title}">×</button></div>{content}</dialog>"#
    )
}

fn attachment(html: &mut String, id: &str) {
    html.push_str(&dialog("repository-dialog", "Attach repository", &format!(r#"
      <form class="project-form repository-form" data-project="{id}" aria-labelledby="repository-dialog-title" action="/projects/{id}/repositories" method="post">
        <label for="repository-choice-{id}">Available checkouts</label>
        <div class="input-action"><select id="repository-choice-{id}" data-repository-choice aria-describedby="repository-options-status-{id}" disabled><option value="">Choose a checkout</option></select><button class="action-button" data-repository-refresh type="button" disabled>Refresh</button></div>
        <p id="repository-options-status-{id}" data-repository-options-status role="status" aria-live="polite">Checkouts load when you open this form.</p>
        <label for="repository-path-{id}">Checkout path</label>
        <p id="repository-hint-{id}">Choose above or enter a path relative to this server’s trusted root, for example <code>team/app</code>.</p>
        <input id="repository-path-{id}" data-repository-path name="path" type="text" required maxlength="4096" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="repository-hint-{id} repository-status-{id}" disabled>
        <p id="repository-status-{id}" data-repository-status role="status" aria-live="polite"></p>
        <div class="form-actions"><button class="action-button" type="button" data-close-dialog>Cancel</button><button class="action-button primary" data-repository-submit type="submit" disabled>Attach</button></div>
      </form>"#)));
    html.push_str("<noscript>JavaScript is required to attach a repository.</noscript>");
}

pub(super) fn escape_into(html: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            other => html.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_creation_uses_a_named_dialog_and_explains_javascript_requirement() {
        let empty = content(&[], false);
        assert!(empty.contains("<dialog id=\"project-dialog\""));
        assert!(!empty.contains("<details"));
        assert!(empty.contains("action=\"/projects\" method=\"post\""));
        assert!(empty.contains("disabled>Create project</button>"));
        assert!(empty.contains("<noscript>JavaScript is required to create a project.</noscript>"));
        let saved = content(
            &[ProjectSummary {
                id: [0; 16],
                name: "Saved project",
                repositories: vec![],
            }],
            false,
        );
        assert!(saved.contains("id=\"project-dialog-title\" tabindex=\"-1\">New project</h2>"));
        assert!(!saved.contains("<details"));
    }

    #[test]
    fn name_filter_progressively_enhances_only_nonempty_project_lists() {
        let project = ProjectSummary {
            id: [0xab; 16],
            name: "<Project> & 工具",
            repositories: vec![],
        };
        let html = content(std::slice::from_ref(&project), false);
        assert!(html.contains("role=\"search\" aria-label=\"Saved projects\" hidden"));
        assert!(html.contains(
            "<label class=\"visually-hidden\" for=\"project-query\">Filter projects by name</label>"
        ));
        assert!(html.contains(
            "aria-controls=\"saved-projects\" aria-describedby=\"project-filter-status\""
        ));
        assert!(html.contains("type=\"button\" disabled>Clear filter</button>"));
        assert!(html.contains(
            "<li data-project-item aria-labelledby=\"project-abababababababababababababababab\">"
        ));
        assert!(html.contains("data-project-name>&lt;Project&gt; &amp; 工具</h3>"));
        assert!(!content(&[], false).contains("id=\"project-filter\""));
        assert!(!detail(&project, false, ProjectSection::Code).contains("id=\"project-filter\""));
    }

    #[test]
    fn rename_form_escapes_the_saved_name_and_selects_the_stable_project() {
        let project = ProjectSummary {
            id: [0xab; 16],
            name: "<Project> & \"工具\"",
            repositories: vec![],
        };
        let html = detail(&project, false, ProjectSection::Settings);
        assert!(html.contains("data-project=\"abababababababababababababababab\" action=\"/projects/abababababababababababababababab/name\""));
        assert!(html.contains("disabled value=\"&lt;Project&gt; &amp; &quot;工具&quot;\""));
        assert!(!html.contains("<dialog"));
        assert!(html.contains("disabled>Save name</button>"));
        assert!(!content(&[project], false).contains("Rename project"));
    }

    #[test]
    fn status_markup_selects_the_saved_checkout_only_on_configured_details() {
        let mut project = ProjectSummary {
            id: [0xab; 16],
            name: "Example",
            repositories: vec![RepositorySummary {
                id: [0xcd; 16],
                name: "<repo>",
                default_checkout: Some("app"),
            }],
        };
        let html = detail(&project, true, ProjectSection::Code);
        assert!(html.contains("data-status-url=\"/projects/abababababababababababababababab/repositories/cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd/status\""));
        assert!(html.contains("aria-label=\"Status for &lt;repo&gt;\""));
        for attribute in [
            "data-repository-status-view",
            "data-status-fields hidden",
            "data-status-branch",
            "data-status-commit",
            "data-status-changes",
            "data-status-refresh disabled",
            "data-status-note",
        ] {
            assert!(html.contains(attribute), "{attribute}");
        }
        assert!(
            !detail(&project, false, ProjectSection::Code).contains("data-repository-status-view")
        );
        assert!(
            !content(std::slice::from_ref(&project), true).contains("data-repository-status-view")
        );
        project.repositories[0].default_checkout = None;
        assert!(
            !detail(&project, true, ProjectSection::Code).contains("data-repository-status-view")
        );
    }

    #[test]
    fn empty_details_offer_the_first_attachment_only_when_configured() {
        let mut project = ProjectSummary {
            id: [0; 16],
            name: "New project",
            repositories: vec![],
        };
        let empty = detail(&project, true, ProjectSection::Code);
        assert!(empty.contains("<dialog id=\"repository-dialog\""));
        assert!(empty.contains("No repositories yet."));
        let unavailable = detail(&project, false, ProjectSection::Code);
        assert!(!unavailable.contains("class=\"project-form repository-form\""));
        assert!(unavailable.contains("Repository attachment is not configured"));
        project.repositories.push(RepositorySummary {
            id: [1; 16],
            name: "Existing",
            default_checkout: Some("app"),
        });
        let attached = detail(&project, true, ProjectSection::Code);
        assert!(attached.contains("<dialog id=\"repository-dialog\""));
        assert!(!attached.contains("<details"));
    }

    #[test]
    fn detail_escapes_title_and_paths_and_reuses_the_attachment_form() {
        let html = crate::render_project_section(
            &ProjectSummary {
                id: [0xab; 16],
                name: "<Project> & 工具",
                repositories: vec![RepositorySummary {
                    id: [1; 16],
                    name: "<repo>",
                    default_checkout: Some("team/<checkout>&工具"),
                }],
            },
            true,
            ProjectSection::Code,
        );
        assert!(html.contains("<title>&lt;Project&gt; &amp; 工具 · Code · DevOption</title>"));
        assert!(html.contains("tabindex=\"-1\">&lt;Project&gt; &amp; 工具</h1>"));
        assert!(html.contains("team/&lt;checkout&gt;&amp;工具"));
        assert!(html.contains("&lt;repo&gt;"));
        assert!(html.contains("href=\"/projects\" aria-current=\"page\""));
        assert!(html.contains("aria-labelledby=\"repository-dialog-title\" action=\"/projects/abababababababababababababababab/repositories\""));
        assert!(!html.contains("id=\"builds-title\""));
        assert!(!html.contains("Create project"));
    }

    #[test]
    fn project_repository_names_and_paths_are_escaped_in_their_project_context() {
        let html = crate::render_projects(
            &[ProjectSummary {
                id: [0xab; 16],
                name: "<script>alert('研究 & \"tools\"')</script>",
                repositories: vec![
                    RepositorySummary {
                        id: [1; 16],
                        name: "<repo>",
                        default_checkout: Some("team/<checkout>&工具"),
                    },
                    RepositorySummary {
                        id: [1; 16],
                        name: "Legacy",
                        default_checkout: None,
                    },
                ],
            }],
            true,
        );
        assert!(html.contains(
            "&lt;script&gt;alert(&#39;研究 &amp; &quot;tools&quot;&#39;)&lt;/script&gt;</h3>"
        ));
        assert!(!html.contains("<script>alert("));
        assert!(html.contains("&lt;repo&gt;"));
        assert!(html.contains("Legacy"));
        assert!(!html.contains("<form class=\"project-form repository-form\""));
        assert!(html.contains("2 repositories"));
        assert!(html.contains("href=\"/projects/abababababababababababababababab\""));
        assert!(html.contains("href=\"/projects\" aria-current=\"page\""));
        assert!(html.contains("data-owner-configured=\"true\""));
        assert!(!html.contains("data-repository-submit type=\"submit\""));
    }

    #[test]
    fn empty_and_unconfigured_views_explain_availability_without_enabling_attachment() {
        let empty = crate::render_projects(&[], true);
        assert!(empty.contains("No projects yet"));
        assert!(!empty.contains("aria-label=\"Saved projects\""));
        assert!(!empty.contains("data-project="));
        let one = content(
            &[ProjectSummary {
                id: [0; 16],
                name: "Example",
                repositories: vec![RepositorySummary {
                    id: [1; 16],
                    name: "Legacy",
                    default_checkout: None,
                }],
            }],
            false,
        );
        assert!(one.contains("1 repository"));
        assert!(!one.contains("Repository attachment is not configured"));
        assert!(!one.contains("data-project="));
        assert!(one.contains("Create project"));
        assert!(
            crate::render(crate::Page::Projects)
                .contains("Project management is not available yet.")
        );
    }
}
