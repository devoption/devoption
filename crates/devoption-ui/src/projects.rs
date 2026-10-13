/// Display-only inputs prepared by the server from a validated catalog snapshot.
pub struct ProjectSummary<'a> {
    pub id: [u8; 16],
    pub name: &'a str,
    pub repositories: Vec<RepositorySummary<'a>>,
}

pub struct RepositorySummary<'a> {
    pub name: &'a str,
    pub default_checkout: Option<&'a str>,
}

pub(super) fn content(projects: &[ProjectSummary<'_>], attachment_available: bool) -> String {
    let mut html = String::from(
        r#"<form id="project-form" class="project-form" action="/projects" method="post">
      <label for="project-name">Project name</label>
      <p id="project-hint">Start with an empty project.</p>
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
        html.push_str("<ul class=\"project-list\" aria-label=\"Saved projects\">");
        for project in projects {
            let id: String = project
                .id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            html.push_str(&format!(
                "<li aria-labelledby=\"project-{id}\"><h2 id=\"project-{id}\">"
            ));
            escape_into(&mut html, project.name);
            let count = project.repositories.len();
            let label = if count == 1 {
                "repository"
            } else {
                "repositories"
            };
            html.push_str(&format!("</h2><p>{count} {label}</p>"));
            if count > 0 {
                html.push_str("<ul class=\"repository-list\" aria-label=\"Repositories\">");
                for repository in &project.repositories {
                    html.push_str("<li><span>");
                    escape_into(&mut html, repository.name);
                    html.push_str("</span><small>");
                    if let Some(path) = repository.default_checkout {
                        html.push_str("Default checkout: <code>");
                        escape_into(&mut html, path);
                        html.push_str("</code>");
                    } else {
                        html.push_str("Not attached");
                    }
                    html.push_str("</small></li>");
                }
                html.push_str("</ul>");
            }
            if attachment_available {
                html.push_str(&format!(r#"<details class="repository-attachment"><summary>Attach repository</summary>
                  <form class="project-form repository-form" data-project="{id}" aria-labelledby="project-{id}" action="/projects/{id}/repositories" method="post">
                    <label for="repository-path-{id}">Checkout path</label>
                    <p id="repository-hint-{id}">Relative to this server’s trusted root, for example <code>team/app</code>. Attach an existing local checkout.</p>
                    <input id="repository-path-{id}" data-repository-path name="path" type="text" required maxlength="4096" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="repository-hint-{id} repository-status-{id}" disabled>
                    <button class="action-button" data-repository-submit type="submit" disabled>Attach</button>
                    <p id="repository-status-{id}" data-repository-status role="status" aria-live="polite"></p>
                    <noscript>JavaScript is required to attach a repository.</noscript>
                  </form></details>"#));
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    html
}

fn escape_into(html: &mut String, value: &str) {
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
    fn project_repository_names_and_paths_are_escaped_in_their_project_context() {
        let html = crate::render_projects(
            &[ProjectSummary {
                id: [0xab; 16],
                name: "<script>alert('研究 & \"tools\"')</script>",
                repositories: vec![
                    RepositorySummary {
                        name: "<repo>",
                        default_checkout: Some("team/<checkout>&工具"),
                    },
                    RepositorySummary {
                        name: "Legacy",
                        default_checkout: None,
                    },
                ],
            }],
            true,
        );
        assert!(html.contains(
            "&lt;script&gt;alert(&#39;研究 &amp; &quot;tools&quot;&#39;)&lt;/script&gt;</h2>"
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
