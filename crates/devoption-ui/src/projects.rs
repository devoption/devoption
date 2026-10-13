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

pub(super) fn content(projects: &[ProjectSummary<'_>], attachment_available: bool) -> String {
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
        html.push_str(r#"<div class="empty-state"><h3>No projects yet</h3><p>Create your first project, then attach a repository when you are ready.</p></div>"#);
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
                html.push_str("No repositories attached");
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
    if !attachment_available {
        unavailable_attachment(&mut html);
    }
    html.push_str(&dialog(
        "project-dialog",
        "New project",
        &project_form(None),
    ));
    html.push_str("<noscript>JavaScript is required to create a project.</noscript>");
    html
}

pub(super) fn detail(project: &ProjectSummary<'_>, attachment_available: bool) -> String {
    let id = identifier(&project.id);
    let mut html = String::from(
        r#"<div class="project-detail"><section aria-labelledby="repositories-title"><div class="section-heading"><h2 id="repositories-title">Repositories</h2>"#,
    );
    if attachment_available {
        html.push_str(r#"<button class="text-button" type="button" data-open-dialog="repository-dialog" disabled><span aria-hidden="true">＋</span>Attach repository</button>"#);
    }
    html.push_str("</div>");
    if project.repositories.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No repositories attached yet.</h3><p>Attach a checkout to see its branch, commit and working-tree status here.</p></div>"#);
    } else {
        html.push_str(r#"<ul class="repository-list" aria-label="Repositories">"#);
        for repository in &project.repositories {
            let repository_id = identifier(&repository.id);
            html.push_str(&format!(r#"<li><div class="repository-summary"><span class="repository-icon" aria-hidden="true">{}</span><div><h3>"#, crate::icon("M6 3v12a4 4 0 0 0 8 0V9M3 6a3 3 0 1 0 6 0a3 3 0 0 0-6 0M11 6a3 3 0 1 0 6 0a3 3 0 0 0-6 0")));
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
    if !attachment_available {
        unavailable_attachment(&mut html);
    }
    html.push_str(r#"</section><section class="availability-note" aria-labelledby="builds-title"><h2 id="builds-title">Builds</h2><p>Build execution is not available yet.</p></section></div>"#);
    html.push_str(&dialog(
        "project-dialog",
        "Rename project",
        &project_form(Some(project)),
    ));
    html.push_str("<noscript>JavaScript is required to rename a project.</noscript>");
    if attachment_available {
        attachment(&mut html, &id);
    }
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
    html.push_str(&format!(r#"><p id="project-status" role="status" aria-live="polite"></p>
      <div class="form-actions"><button class="action-button" type="button" data-close-dialog>Cancel</button><button id="project-submit" class="action-button primary" type="submit" disabled>{submit}</button></div></form>"#));
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
        assert!(!detail(&project, false).contains("id=\"project-filter\""));
    }

    #[test]
    fn rename_form_escapes_the_saved_name_and_selects_the_stable_project() {
        let project = ProjectSummary {
            id: [0xab; 16],
            name: "<Project> & \"工具\"",
            repositories: vec![],
        };
        let html = detail(&project, false);
        assert!(html.contains("data-project=\"abababababababababababababababab\" action=\"/projects/abababababababababababababababab/name\""));
        assert!(html.contains("disabled value=\"&lt;Project&gt; &amp; &quot;工具&quot;\""));
        assert!(html.contains("id=\"project-dialog-title\" tabindex=\"-1\">Rename project</h2>"));
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
        let html = detail(&project, true);
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
        assert!(!detail(&project, false).contains("data-repository-status-view"));
        assert!(
            !content(std::slice::from_ref(&project), true).contains("data-repository-status-view")
        );
        project.repositories[0].default_checkout = None;
        assert!(!detail(&project, true).contains("data-repository-status-view"));
    }

    #[test]
    fn empty_details_offer_the_first_attachment_only_when_configured() {
        let mut project = ProjectSummary {
            id: [0; 16],
            name: "New project",
            repositories: vec![],
        };
        let empty = detail(&project, true);
        assert!(empty.contains("<dialog id=\"repository-dialog\""));
        assert!(empty.contains("No repositories attached yet."));
        let unavailable = detail(&project, false);
        assert!(!unavailable.contains("class=\"project-form repository-form\""));
        assert!(unavailable.contains("Repository attachment is not configured"));
        project.repositories.push(RepositorySummary {
            id: [1; 16],
            name: "Existing",
            default_checkout: Some("app"),
        });
        let attached = detail(&project, true);
        assert!(attached.contains("<dialog id=\"repository-dialog\""));
        assert!(!attached.contains("<details"));
    }

    #[test]
    fn detail_escapes_title_and_paths_and_reuses_the_attachment_form() {
        let html = crate::render_project(
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
        );
        assert!(html.contains("<title>&lt;Project&gt; &amp; 工具 · DevOption</title>"));
        assert!(html.contains("tabindex=\"-1\">&lt;Project&gt; &amp; 工具</h1>"));
        assert!(html.contains("team/&lt;checkout&gt;&amp;工具"));
        assert!(html.contains("&lt;repo&gt;"));
        assert!(html.contains("href=\"/projects\" aria-current=\"page\""));
        assert!(html.contains("aria-labelledby=\"repository-dialog-title\" action=\"/projects/abababababababababababababababab/repositories\""));
        assert!(html.contains("Build execution is not available yet."));
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
        assert!(one.contains("Repository attachment is not configured"));
        assert!(!one.contains("data-project="));
        assert!(one.contains("Create project"));
        assert!(
            crate::render(crate::Page::Projects)
                .contains("Project management is not available yet.")
        );
    }
}
