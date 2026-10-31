/// The pages currently available in DevOption.
#[derive(Clone, Copy)]
pub enum Page {
    Home,
    Projects,
    Releases,
    Infrastructure,
    Appearance,
    Profile,
    Users,
    SignIn,
}

mod auth;
mod users;
const ACCOUNTS: &str = include_str!("../assets/accounts.js");
pub fn render_users() -> String {
    render_with_account(Page::Users, true)
}
pub fn render_join() -> String {
    auth::render_join()
}
mod overview;
mod platform_updates;
pub use overview::{MetricPoint, Monitoring, Overview, OverviewOutcome, OverviewProject};
mod profile;
pub use platform_updates::{PlatformUpdates, UpgradeNotice};
mod projects;
pub use projects::{
    BuildAttemptView, BuildExecutionView, BuildHistoryView, BuildLogView, DeploymentView,
    EnvironmentSecretsView, FileEntry, FilePreview, HistoryAvailability, PreparationCheck,
    PreparationView, ProjectSection, ProjectSummary, ReleaseView, RepositoryFilesView,
    RepositorySummary, SecretEventView, SecretMetadataView,
};

const HOME: &str = r#"<section class="empty-state"><h2>Start with a project</h2><p>Create a project to organize your source code, builds and app releases.</p><a class="action-button primary" href="/projects">Open Projects <span aria-hidden="true">→</span></a></section>"#;

const STYLES: &str = include_str!("../assets/theme.css");
const APPEARANCE: &str = include_str!("../assets/appearance.js");
const REPOSITORY_CREATE: &str = include_str!("../assets/repository-create.js");
const PROJECT_SECRETS: &str = include_str!("../assets/project-secrets.js");
const PROFILE: &str = include_str!("../assets/profile.js");
const BUILD_EXECUTION: &str = concat!(
    include_str!("../assets/build-execution.js"),
    "\n",
    include_str!("../assets/deployment.js")
);
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
    render_project_section_with_creation(project, attachment_available, false, section)
}

/// Render creation controls only when startup granted managed storage authority.
pub fn render_project_section_with_creation(
    project: &ProjectSummary<'_>,
    attachment_available: bool,
    creation_available: bool,
    section: ProjectSection,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Detail(
            project,
            attachment_available,
            creation_available,
            section,
        )),
    )
}

pub fn render_repository_files(
    project: &ProjectSummary<'_>,
    files: &RepositoryFilesView<'_>,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Files(project, files)),
    )
}

pub fn render_build_preparation(
    project: &ProjectSummary<'_>,
    view: &PreparationView<'_>,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Preparation(project, view)),
    )
}

pub fn render_deployment(project: &ProjectSummary<'_>, view: &DeploymentView) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Deployment(project, view)),
    )
}

pub fn render_build_execution(project: &ProjectSummary<'_>, view: &BuildExecutionView) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Execution(project, view)),
    )
}

pub fn render_build_history(project: &ProjectSummary<'_>, view: &BuildHistoryView) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::History(project, view)),
    )
}

pub fn render_project_secrets(
    project: &ProjectSummary<'_>,
    view: &EnvironmentSecretsView,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Secrets(project, view)),
    )
}

enum ProjectView<'a> {
    Monitoring(&'a ProjectSummary<'a>, &'a Monitoring, u64),
    Secrets(&'a ProjectSummary<'a>, &'a EnvironmentSecretsView),
    Deployment(&'a ProjectSummary<'a>, &'a DeploymentView),
    Execution(&'a ProjectSummary<'a>, &'a BuildExecutionView),
    History(&'a ProjectSummary<'a>, &'a BuildHistoryView),
    List(&'a [ProjectSummary<'a>], bool),
    Detail(&'a ProjectSummary<'a>, bool, bool, ProjectSection),
    Files(&'a ProjectSummary<'a>, &'a RepositoryFilesView<'a>),
    Preparation(&'a ProjectSummary<'a>, &'a PreparationView<'a>),
}

pub fn render_platform_home(updates: &PlatformUpdates) -> String {
    render_overview(&Overview::default(), updates)
}

/// Render authenticated operational evidence inside the unchanged page shell.
pub fn render_overview(view: &Overview, updates: &PlatformUpdates) -> String {
    render_page_with_updates(Page::Home, true, None, Some(updates), Some(view))
}

/// Per-project traffic details use the existing project workspace shell.
pub fn render_project_monitoring(
    project: &ProjectSummary<'_>,
    monitoring: &Monitoring,
    hours: u64,
) -> String {
    render_page(
        Page::Projects,
        true,
        Some(ProjectView::Monitoring(project, monitoring, hours)),
    )
}

fn render_page(page: Page, account_available: bool, projects: Option<ProjectView<'_>>) -> String {
    render_page_with_updates(page, account_available, projects, None, None)
}

fn render_page_with_updates(
    page: Page,
    account_available: bool,
    projects: Option<ProjectView<'_>>,
    updates: Option<&PlatformUpdates>,
    overview: Option<&Overview>,
) -> String {
    if matches!(page, Page::SignIn) {
        return auth::render(account_available);
    }
    // Dynamic project content is escaped by its renderer; other values are static.
    let (title, path, message) = match page {
        Page::Home => (
            "Overview",
            "/",
            "Application health, ongoing work, and decisions that need you.",
        ),
        Page::Projects => (
            "Projects",
            "/projects",
            match &projects {
                Some(ProjectView::List([], _)) => "No projects yet.",
                Some(ProjectView::List(..)) => "Your saved projects.",
                Some(
                    ProjectView::Monitoring(..)
                    | ProjectView::Detail(..)
                    | ProjectView::Files(..)
                    | ProjectView::Preparation(..)
                    | ProjectView::History(..)
                    | ProjectView::Deployment(..)
                    | ProjectView::Secrets(..)
                    | ProjectView::Execution(..),
                ) => "",
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
        Page::Users => (
            "Users",
            "/settings/users",
            "Manage access to your shared workspace.",
        ),
        Page::Profile => (
            "Profile",
            "/settings/profile",
            "Choose the name shown in your account menu.",
        ),
        Page::SignIn => unreachable!("Authentication uses its dedicated layout"),
    };
    let (title, main_class) = if let Some(
        ProjectView::Monitoring(project, _, _)
        | ProjectView::Detail(project, _, _, _)
        | ProjectView::Files(project, _)
        | ProjectView::Preparation(project, _)
        | ProjectView::History(project, _)
        | ProjectView::Deployment(project, _)
        | ProjectView::Secrets(project, _)
        | ProjectView::Execution(project, _),
    ) = &projects
    {
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
        Some(
            ProjectView::Monitoring(..)
            | ProjectView::Detail(..)
            | ProjectView::Files(..)
            | ProjectView::Preparation(..)
            | ProjectView::History(..)
            | ProjectView::Deployment(..)
            | ProjectView::Secrets(..)
            | ProjectView::Execution(..),
        ) => "",
        None => "",
    };
    let breadcrumb = if matches!(
        &projects,
        Some(
            ProjectView::Monitoring(..)
                | ProjectView::Detail(..)
                | ProjectView::Files(..)
                | ProjectView::Preparation(..)
                | ProjectView::History(..)
                | ProjectView::Deployment(..)
                | ProjectView::Secrets(..)
                | ProjectView::Execution(..)
        )
    ) {
        format!(
            r#"<p class="breadcrumbs"><a href="/projects">Projects</a><span aria-hidden="true">/</span><span>{title}</span></p>"#
        )
    } else {
        format!(r#"<p class="breadcrumbs">{title}</p>"#)
    };
    let document_title = match &projects {
        Some(ProjectView::Secrets(..)) => format!("{title} · Environments"),
        Some(ProjectView::Deployment(..)) => format!("{title} · Releases"),
        Some(ProjectView::Monitoring(..)) => format!("{title} · Monitoring"),
        Some(ProjectView::Files(..)) => format!("{title} · Files"),
        Some(
            ProjectView::Preparation(..) | ProjectView::History(..) | ProjectView::Execution(..),
        ) => {
            format!("{title} · Builds")
        }
        Some(ProjectView::Detail(_, _, _, section)) if *section != ProjectSection::Overview => {
            format!("{title} · {}", section.label())
        }
        _ => title.clone(),
    };
    let project_content = projects.map(|view| match view {
        ProjectView::Monitoring(project, monitoring, hours) => {
            let base = format!("/projects/{}/monitoring", overview::hex(&project.id));
            format!(
                "<p><a href=\"/projects/{}\">← Project overview</a></p>{}",
                overview::hex(&project.id),
                overview::monitoring(monitoring, hours, &base)
            )
        }
        ProjectView::Secrets(project, view) => projects::secrets_content(project, view),
        ProjectView::Files(project, files) => projects::files_content(project, files),
        ProjectView::History(project, view) => projects::history_content(project, view),
        ProjectView::Deployment(project, view) => projects::deployment_content(project, view),
        ProjectView::Execution(project, view) => projects::execution_content(project, view),
        ProjectView::Preparation(project, view) => projects::preparation_content(project, view),
        ProjectView::List(projects, available) => projects::content(projects, available),
        ProjectView::Detail(project, available, creation, section) => {
            projects::detail_with_creation(project, available, creation, section)
        }
    });
    let update_content = updates.map(|updates| format!("{}<details class=\"overview-maintenance\"><summary>Platform maintenance</summary>{}</details>", overview::content(overview.unwrap_or(&Overview::default())), platform_updates::content(updates)));
    let (appearance_current, content) = if let Some(content) =
        project_content.as_deref().or(update_content.as_deref())
    {
        ("", content)
    } else if path == "/settings/appearance" {
        (" aria-current=\"page\"", APPEARANCE_CONTROLS)
    } else {
        (
            "",
            match page {
                Page::Profile => profile::CONTENT,
                Page::Users => users::CONTENT,
                Page::Home => HOME,
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
        <span class=\"profile-avatar\" aria-hidden=\"true\"><span id=\"profile-avatar-icon\">{profile_icon}</span><span id=\"profile-initials\" hidden></span></span>
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
    <header class=\"workspace-header\">{breadcrumb}</header>
    <main id=\"main-content\" tabindex=\"-1\" aria-labelledby=\"page-title\"{main_class}>
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
<script>{BUILD_EXECUTION}</script>
<script>{PROJECT_SECRETS}</script>
  <script>{PROFILE}</script>
  <script>{ACCOUNTS}</script>
  <script>{REPOSITORY_CREATE}</script>
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
