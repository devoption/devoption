/// The pages currently available in DevOption.
#[derive(Clone, Copy)]
pub enum Page {
    Home,
    Projects,
    Releases,
    Infrastructure,
    Appearance,
    SignIn,
}

mod auth;
mod projects;
pub use projects::{ProjectSection, ProjectSummary, RepositorySummary};

const STYLES: &str = include_str!("../assets/theme.css");
const APPEARANCE: &str = include_str!("../assets/appearance.js");
const AUTH: &str = include_str!("../assets/auth.js");
const REPOSITORY_STATUS: &str = include_str!("../assets/repository-status.js");
const PROJECT_FILTER: &str = include_str!("../assets/project-filter.js");
const INTERFACE: &str = include_str!("../assets/interface.js");
const LOGO: &str = include_str!("../assets/devoption.svg");
const APPEARANCE_CONTROLS: &str = r##"
      <section class="appearance" aria-label="Appearance preferences">
        <div id="appearance-mode-options" class="appearance-options" hidden><h2>Mode</h2><div class="choice-buttons" role="group" aria-label="Color mode">
          <button type="button" data-appearance-setting="appearance-mode" data-appearance-value="light" aria-pressed="false">Light</button>
          <button type="button" data-appearance-setting="appearance-mode" data-appearance-value="dark" aria-pressed="true">Dark</button>
          <button type="button" data-appearance-setting="appearance-mode" data-appearance-value="system" aria-pressed="false">System</button>
        </div></div>
        <div id="appearance-theme-options" class="appearance-options theme-options" hidden><h2>Theme</h2><div class="choice-buttons" role="group" aria-label="Theme">
          <button type="button" data-appearance-setting="appearance-theme" data-appearance-value="nord" aria-pressed="true"><span class="palette-swatch" aria-hidden="true"></span>Nord</button>
          <button type="button" data-appearance-setting="appearance-theme" data-appearance-value="dracula" aria-pressed="false"><span class="palette-swatch" aria-hidden="true"></span>Dracula</button>
          <button type="button" data-appearance-setting="appearance-theme" data-appearance-value="tokyo-night" aria-pressed="false"><span class="palette-swatch" aria-hidden="true"></span>Tokyo Night</button>
        </div></div>
        <div id="appearance-accent-options" class="appearance-options" hidden><h2>Accent</h2><div class="choice-buttons" role="group" aria-label="Accent">
          <button type="button" data-appearance-setting="appearance-accent" data-appearance-value="purple" aria-pressed="true">Purple</button>
          <button type="button" data-appearance-setting="appearance-accent" data-appearance-value="blue" aria-pressed="false">Blue</button>
          <button type="button" data-appearance-setting="appearance-accent" data-appearance-value="green" aria-pressed="false">Green</button>
        </div></div>
        <div id="appearance-theme-native" class="theme-preview">
          <span class="theme-swatch" aria-hidden="true"></span>
          <label for="appearance-theme">Theme</label>
          <select id="appearance-theme" aria-describedby="appearance-status" disabled>
            <option value="nord">Nord</option>
            <option value="dracula">Dracula</option>
            <option value="tokyo-night">Tokyo Night</option>
          </select>
        </div>
        <p id="appearance-theme-note" hidden>Dracula light is a DevOption adaptation.</p>
        <div id="appearance-mode-native" class="mode-choice">
          <label for="appearance-mode">Mode</label>
          <select id="appearance-mode" aria-describedby="appearance-status" disabled>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
            <option value="system">System</option>
          </select>
        </div>
        <div id="appearance-accent-native" class="mode-choice">
          <label for="appearance-accent">Accent</label>
          <select id="appearance-accent" aria-describedby="appearance-status" disabled>
            <option value="purple">Purple</option>
            <option value="blue">Blue</option>
            <option value="green">Green</option>
            <option value="custom">Custom</option>
          </select>
        </div>
        <div class="mode-choice">
          <label for="appearance-primary">Primary color</label>
          <span class="primary-choice">
            <output id="appearance-hex" for="appearance-primary">#b48ead</output>
            <input id="appearance-primary" type="color" value="#b48ead" aria-describedby="appearance-contrast appearance-status" disabled>
          </span>
        </div>
        <p id="appearance-contrast" role="status" hidden></p>
        <button id="appearance-reset" type="button" aria-describedby="appearance-status" disabled>Reset to Purple</button>
        <p id="appearance-status" role="status">Enable JavaScript to change appearance.</p>
      </section>"##;

/// Render a complete HTML document independently of the HTTP server.
pub fn render(page: Page) -> String {
    render_with_account(page, false)
}

/// Account availability is validated by server startup, not supplied by a visitor.
pub fn render_with_account(page: Page, account_available: bool) -> String {
    render_page(page, account_available, None)
}

/// Render a configured owner's project snapshot using the shared page shell.
/// The caller must authenticate the request; display names are escaped here.
pub fn render_projects(projects: &[ProjectSummary<'_>], attachment_available: bool) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::List(projects, attachment_available)),
    )
}

/// Render one authenticated project with its current repository snapshot.
pub fn render_project(project: &ProjectSummary<'_>, attachment_available: bool) -> String {
    render_project_section(project, attachment_available, ProjectSection::Overview)
}

/// Render one authenticated section of a saved project workspace.
pub fn render_project_section(
    project: &ProjectSummary<'_>,
    attachment_available: bool,
    section: ProjectSection,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Detail(project, attachment_available, section)),
    )
}

enum ProjectView<'a> {
    List(&'a [ProjectSummary<'a>], bool),
    Detail(&'a ProjectSummary<'a>, bool, ProjectSection),
}

fn render_page(page: Page, account_available: bool, projects: Option<ProjectView<'_>>) -> String {
    if matches!(page, Page::SignIn) {
        return auth::render(account_available);
    }
    // Dynamic project content is escaped by its renderer; other values are static.
    let (title, path, message) = match page {
        Page::Home => ("Overview", "/", "One place for your software projects."),
        Page::Projects => (
            "Projects",
            "/projects",
            match &projects {
                Some(ProjectView::List([], _)) => "No projects yet.",
                Some(ProjectView::List(..)) => "Your saved projects.",
                Some(ProjectView::Detail(..)) => "",
                None => "Project management is not available yet.",
            },
        ),
        Page::Releases => (
            "Releases",
            "/releases",
            "Review and manage project releases.",
        ),
        Page::Infrastructure => (
            "Infrastructure",
            "/infrastructure",
            "Where your factory and applications will run.",
        ),
        Page::Appearance => (
            "Appearance",
            "/settings/appearance",
            "Choose how DevOption looks in this browser.",
        ),
        Page::SignIn => unreachable!("Authentication uses its dedicated layout"),
    };
    let (title, main_class) = if let Some(ProjectView::Detail(project, _, _)) = &projects {
        let mut escaped = String::new();
        projects::escape_into(&mut escaped, project.name);
        (escaped, r#" class="project-detail-page""#)
    } else {
        (title.to_owned(), "")
    };
    let actions = match &projects {
        Some(ProjectView::List(..)) => {
            r#"<button class="action-button primary" type="button" data-open-dialog="project-dialog" disabled><span aria-hidden="true">＋</span>New project</button>"#
        }
        Some(ProjectView::Detail(..)) => "",
        None => "",
    };
    let breadcrumb = if matches!(&projects, Some(ProjectView::Detail(..))) {
        format!(
            r#"<p class="breadcrumbs"><a href="/projects">Projects</a><span aria-hidden="true">/</span><span>{title}</span></p>"#
        )
    } else if matches!(page, Page::Appearance) {
        String::new()
    } else {
        format!(r#"<p class="breadcrumbs">{title}</p>"#)
    };
    let document_title = match &projects {
        Some(ProjectView::Detail(_, _, section)) if *section != ProjectSection::Overview => {
            format!("{title} · {}", section.label())
        }
        _ => title.clone(),
    };
    let project_content = projects.map(|view| match view {
        ProjectView::List(projects, available) => projects::content(projects, available),
        ProjectView::Detail(project, available, section) => {
            projects::detail(project, available, section)
        }
    });
    let (appearance_current, content) = if let Some(content) = project_content.as_deref() {
        ("", content)
    } else if path == "/settings/appearance" {
        (" aria-current=\"page\"", APPEARANCE_CONTROLS)
    } else {
        (
            "",
            match page {
                Page::Home => {
                    r#"<section class="empty-state"><h2>Start with a project</h2><p>Create a project to organize your software. Builds and deployments are not available yet.</p><a class="action-button primary" href="/projects">Open Projects <span aria-hidden="true">→</span></a></section>"#
                }
                Page::Releases => {
                    r#"<section class="empty-state"><h2>Releases are not available yet</h2><p>Release creation and publishing are not implemented. Your saved projects and repositories are available in Projects.</p><a class="action-button" href="/projects">Open Projects <span aria-hidden="true">→</span></a></section>"#
                }
                Page::Infrastructure => {
                    r#"<section class="empty-state"><h2>Runtime management is not available yet</h2><p>Deployment, runtime enrollment and live health monitoring are not implemented.</p></section>"#
                }
                _ => "",
            },
        )
    };
    let navigation: String = [
        ("/", "Overview", "M3 10 12 3l9 7v11h-6v-7H9v7H3Z"),
        ("/projects", "Projects", "M3 7V4h6l3 3h9v13H3Z"),
        (
            "/releases",
            "Releases",
            "M6 3h8l4 4v14H6ZM14 3v5h4M9 12h6M9 16h6",
        ),
        (
            "/infrastructure",
            "Infrastructure",
            "M3 3h18v7H3ZM3 14h18v7H3ZM7 6h.01M7 17h.01M12 6h5M12 17h5",
        ),
    ]
    .into_iter()
    .map(|(href, label, icon_path)| {
        let current = if href == path {
            " aria-current=\"page\""
        } else {
            ""
        };
        format!(
            "<a href=\"{href}\"{current}>{}<span>{label}</span></a>",
            icon(icon_path)
        )
    })
    .collect();
    let profile_icon = icon("M20 21v-2a8 8 0 0 0-16 0v2M16 7a4 4 0 1 1-8 0a4 4 0 0 1 8 0");

    document(&document_title, &format!(
        "<body data-owner-configured=\"{account_available}\">
  <a class=\"skip-link\" href=\"#main-content\">Skip to content</a>
  <aside class=\"sidebar\" aria-label=\"Sidebar\">
    <a class=\"brand\" href=\"/\" aria-label=\"DevOption home\">{LOGO}</a>
    <nav aria-label=\"Main navigation\">{navigation}</nav>
    <div class=\"profile-area\">
      <button class=\"profile-trigger\" type=\"button\" popovertarget=\"profile-popup\" aria-label=\"Profile menu\">
        <span class=\"profile-avatar\">{profile_icon}</span>
        <span id=\"profile-state\" class=\"profile-label\">Account</span><span class=\"chevron\" aria-hidden=\"true\"></span>
      </button>
      <section id=\"profile-popup\" popover aria-labelledby=\"profile-title\">
        <h2 id=\"profile-title\">Account</h2>
        <p id=\"profile-note\" class=\"profile-note\">Sign in to your account.</p>
        <a id=\"profile-account\" class=\"profile-link\" href=\"/sign-in\">Sign in</a>
        <button class=\"profile-link\" type=\"button\" data-signout hidden disabled>Sign out</button>
        <p id=\"profile-status\" class=\"profile-note\" role=\"status\"></p>
        <a class=\"profile-link\" href=\"/settings/appearance\"{appearance_current}>Appearance</a>
      </section>
    </div>
  </aside>
  <div class=\"workspace\">
    <main id=\"main-content\" tabindex=\"-1\" aria-labelledby=\"page-title\"{main_class}>
      {breadcrumb}
      <div class=\"page-heading\"><div><h1 id=\"page-title\" tabindex=\"-1\">{title}</h1>
      <p id=\"page-message\">{message}</p></div><div class=\"page-actions\">{actions}</div></div>
      {content}
    </main>
  </div>
</body>"
    ))
}

fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\">
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
  <meta name=\"referrer\" content=\"no-referrer\">
  <title>{title} · DevOption</title>
  <style>{STYLES}</style>
  <script>{APPEARANCE}</script>
  <script>{AUTH}</script>
  <script>{REPOSITORY_STATUS}</script>
  <script>{PROJECT_FILTER}</script>
  <script>{INTERFACE}</script>
</head>
{body}
</html>"
    )
}

fn icon(path: &str) -> String {
    format!(
        "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\" focusable=\"false\"><path d=\"{path}\"/></svg>"
    )
}
