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
        r#"<form id="project-form" class="project-form" action="/projects" method="post">
      <label for="project-name">Project name</label>
      <p id="project-hint">Your new project opens after saving.</p>
      <input id="project-name" name="name" type="text" required maxlength="128" autocomplete="off" aria-describedby="project-hint project-status" disabled>
      <button id="project-submit" class="action-button" type="submit" disabled>Create project</button>
      <p id="project-status" role="status" aria-live="polite"></p>
      <noscript>JavaScript is required to create a project.</noscript>
    </form>"#,
    );
    if !attachment_available {
        html.push_str(r#"<p class="repository-notice">Repository attachment is not configured on this server. A server administrator must select a trusted repository root before you can attach a checkout.</p>"#);
    }
    if !projects.is_empty() {
        html.push_str(r#"<div id="project-filter" class="project-form project-filter" role="search" aria-label="Saved projects" hidden>
          <label for="project-query">Filter projects by name</label>
          <input id="project-query" type="search" autocomplete="off" aria-controls="saved-projects" aria-describedby="project-filter-status">
          <button id="project-filter-clear" class="action-button" type="button" disabled>Clear filter</button>
          <p id="project-filter-status" role="status" aria-live="polite" aria-atomic="true"></p>
        </div>
        <ul id="saved-projects" class="project-list" aria-label="Saved projects">"#);
        for project in projects {
            let id: String = project
                .id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            html.push_str(&format!(
                "<li data-project-item aria-labelledby=\"project-{id}\"><h2 id=\"project-{id}\"><a data-project-name href=\"/projects/{id}\">"
            ));
            escape_into(&mut html, project.name);
            html.push_str("</a></h2>");
            repository_count(&mut html, project);
            repositories(&mut html, project, false);
            if attachment_available {
                attachment(&mut html, &id, &format!("project-{id}"), false);
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    html
}

pub(super) fn detail(project: &ProjectSummary<'_>, attachment_available: bool) -> String {
    let id: String = project
        .id
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut html = format!(
        r#"<details class="project-rename"><summary>Rename project</summary>
      <form id="project-form" class="project-form" data-project="{id}" action="/projects/{id}/name" method="post">
        <label for="project-name">Project name</label>
        <p id="project-hint">Attached repositories keep their names and checkout paths.</p>
        <input id="project-name" name="name" type="text" required maxlength="128" autocomplete="off" aria-describedby="project-hint project-status" disabled value=""#
    );
    escape_into(&mut html, project.name);
    html.push_str(r#"">
        <button id="project-submit" class="action-button" type="submit" disabled>Save name</button>
        <p id="project-status" role="status" aria-live="polite"></p>
        <noscript>JavaScript is required to rename a project.</noscript>
      </form></details>
      <div class="project-detail"><section aria-labelledby="repositories-title"><h2 id="repositories-title">Repositories</h2>"#);
    if project.repositories.is_empty() {
        html.push_str("<p>No repositories attached yet.</p>");
    } else {
        repository_count(&mut html, project);
        repositories(&mut html, project, attachment_available);
    }
    if attachment_available {
        let id: String = project
            .id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        attachment(
            &mut html,
            &id,
            "page-title",
            project.repositories.is_empty(),
        );
    } else {
        html.push_str(r#"<p class="repository-notice">Repository attachment is not configured on this server. A server administrator must select a trusted repository root before you can attach a checkout.</p>"#);
    }
    html.push_str(r#"</section><section aria-labelledby="builds-title"><h2 id="builds-title">Builds</h2><p>Build execution is not available yet.</p></section></div>"#);
    html
}

fn repository_count(html: &mut String, project: &ProjectSummary<'_>) {
    let count = project.repositories.len();
    let label = if count == 1 {
        "repository"
    } else {
        "repositories"
    };
    html.push_str(&format!("<p>{count} {label}</p>"));
}

fn repositories(html: &mut String, project: &ProjectSummary<'_>, status_available: bool) {
    if !project.repositories.is_empty() {
        html.push_str("<ul class=\"repository-list\" aria-label=\"Repositories\">");
        for repository in &project.repositories {
            html.push_str("<li><span>");
            escape_into(html, repository.name);
            html.push_str("</span><small>");
            if let Some(path) = repository.default_checkout {
                html.push_str("Default checkout: <code>");
                escape_into(html, path);
                html.push_str("</code>");
            } else {
                html.push_str("Not attached");
            }
            html.push_str("</small>");
            if status_available && repository.default_checkout.is_some() {
                let project: String = project
                    .id
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                let repository_id: String = repository
                    .id
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                html.push_str(&format!(r#"<details class="repository-inspection" data-repository-status-view data-status-url="/projects/{project}/repositories/{repository_id}/status">
                  <summary aria-label="View status for "#));
                escape_into(html, repository.name);
                html.push_str(r#"">View status</summary>
                  <dl data-status-fields hidden><dt>Branch</dt><dd data-status-branch></dd><dt>Commit</dt><dd data-status-commit></dd><dt>Working tree</dt><dd data-status-changes></dd></dl>
                  <button class="action-button" type="button" data-status-refresh disabled>Refresh status</button>
                  <p data-status-note role="status" aria-live="polite">Enable JavaScript to read repository status.</p>
                </details>"#);
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
}

fn attachment(html: &mut String, id: &str, label: &str, open: bool) {
    let expanded = if open { " open" } else { "" };
    html.push_str(&format!(r#"<details class="repository-attachment"{expanded}><summary>Attach repository</summary>
                  <form class="project-form repository-form" data-project="{id}" aria-labelledby="{label}" action="/projects/{id}/repositories" method="post">
                    <label for="repository-choice-{id}">Available checkouts</label>
                    <select id="repository-choice-{id}" data-repository-choice aria-describedby="repository-options-status-{id}" disabled><option value="">Choose a checkout</option></select>
                    <button class="action-button" data-repository-refresh type="button" disabled>Refresh</button>
                    <p id="repository-options-status-{id}" data-repository-options-status role="status" aria-live="polite">Checkouts load when you open this form.</p>
                    <label for="repository-path-{id}">Checkout path</label>
                    <p id="repository-hint-{id}">Choose above or enter a path relative to this server’s trusted root, for example <code>team/app</code>.</p>
                    <input id="repository-path-{id}" data-repository-path name="path" type="text" required maxlength="4096" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="repository-hint-{id} repository-status-{id}" disabled>
                    <button class="action-button" data-repository-submit type="submit" disabled>Attach</button>
                    <p id="repository-status-{id}" data-repository-status role="status" aria-live="polite"></p>
                    <noscript>JavaScript is required to attach a repository.</noscript>
                  </form></details>"#));
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
    fn name_filter_progressively_enhances_only_nonempty_project_lists() {
        let project = ProjectSummary {
            id: [0xab; 16],
            name: "<Project> & 工具",
            repositories: vec![],
        };
        let html = content(std::slice::from_ref(&project), false);
        assert!(html.contains("role=\"search\" aria-label=\"Saved projects\" hidden"));
        assert!(html.contains("<label for=\"project-query\">Filter projects by name</label>"));
        assert!(html.contains(
            "aria-controls=\"saved-projects\" aria-describedby=\"project-filter-status\""
        ));
        assert!(html.contains("type=\"button\" disabled>Clear filter</button>"));
        assert!(html.contains(
            "<li data-project-item aria-labelledby=\"project-abababababababababababababababab\">"
        ));
        assert!(html.contains("data-project-name href=\"/projects/abababababababababababababababab\">&lt;Project&gt; &amp; 工具</a>"));
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
        assert!(html.contains("<summary>Rename project</summary>"));
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
        assert!(html.contains("aria-label=\"View status for &lt;repo&gt;\""));
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
        assert!(empty.contains("<details class=\"repository-attachment\" open>"));
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
        assert!(attached.contains("<details class=\"repository-attachment\">"));
        assert!(!attached.contains("<details class=\"repository-attachment\" open>"));
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
        assert!(html.contains("aria-labelledby=\"page-title\" action=\"/projects/abababababababababababababababab/repositories\""));
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
            "&lt;script&gt;alert(&#39;研究 &amp; &quot;tools&quot;&#39;)&lt;/script&gt;</a></h2>"
        ));
        assert!(!html.contains("<script>alert("));
        assert!(html.contains("&lt;repo&gt;"));
        assert!(html.contains("team/&lt;checkout&gt;&amp;工具"));
        assert!(html.contains("Not attached"));
        assert!(html.contains("<p>2 repositories</p>"));
        assert!(html.contains("data-project=\"abababababababababababababababab\""));
        assert!(html.contains("href=\"/projects\" aria-current=\"page\""));
        assert!(html.contains("data-owner-configured=\"true\""));
        assert!(html.contains("aria-describedby=\"repository-hint-abababababababababababababababab repository-status-abababababababababababababababab\" disabled"));
    }

    #[test]
    fn empty_and_unconfigured_views_explain_availability_without_enabling_attachment() {
        let empty = crate::render_projects(&[], true);
        assert!(empty.contains("No projects yet."));
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
