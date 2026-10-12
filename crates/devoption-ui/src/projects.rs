/// Display-only input prepared by the server from a validated catalog snapshot.
pub struct ProjectSummary<'a> {
    pub name: &'a str,
    pub repository_count: usize,
}

pub(super) fn content(projects: &[ProjectSummary<'_>]) -> String {
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
    if !projects.is_empty() {
        html.push_str("<ul class=\"project-list\" aria-label=\"Saved projects\">");
        for project in projects {
            html.push_str("<li><h2>");
            escape_into(&mut html, project.name);
            let count = project.repository_count;
            let label = if count == 1 {
                "repository"
            } else {
                "repositories"
            };
            html.push_str(&format!("</h2><p>{count} {label}</p></li>"));
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
    fn names_are_escaped_without_becoming_markup_or_losing_unicode() {
        let html = crate::render_projects(&[ProjectSummary {
            name: "<script>alert('研究 & \"tools\"')</script>",
            repository_count: 2,
        }]);
        assert!(html.contains(
            "<h2>&lt;script&gt;alert(&#39;研究 &amp; &quot;tools&quot;&#39;)&lt;/script&gt;</h2>"
        ));
        assert!(!html.contains("<script>alert("));
        assert!(html.contains("<p>2 repositories</p>"));
        assert!(html.contains("href=\"/projects\" aria-current=\"page\""));
        assert!(html.contains("data-owner-configured=\"true\""));
    }

    #[test]
    fn empty_and_single_repository_views_include_the_creation_form() {
        let empty = crate::render_projects(&[]);
        assert!(empty.contains("No projects yet."));
        assert!(!empty.contains("aria-label=\"Saved projects\""));
        let one = content(&[ProjectSummary {
            name: "Example",
            repository_count: 1,
        }]);
        assert!(one.contains("1 repository"));
        assert!(!one.contains("<a "));
        assert!(one.contains("Create project"));
        assert!(
            crate::render(crate::Page::Projects)
                .contains("Project management is not available yet.")
        );
    }
}
