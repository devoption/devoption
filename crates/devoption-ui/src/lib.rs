/// The pages currently available in DevOption.
#[derive(Clone, Copy)]
pub enum Page {
    Home,
    Projects,
    About,
    Appearance,
    SignIn,
}

mod projects;
pub use projects::{ProjectSummary, RepositorySummary};

const STYLES: &str = include_str!("../assets/theme.css");
const APPEARANCE: &str = include_str!("../assets/appearance.js");
const AUTH: &str = include_str!("../assets/auth.js");
const LOGO: &str = include_str!("../assets/devoption.svg");
const APPEARANCE_CONTROLS: &str = r##"
      <section class="appearance" aria-label="Appearance preferences">
        <div class="theme-preview">
          <span class="theme-swatch" aria-hidden="true"></span>
          <label for="appearance-theme">Theme</label>
          <select id="appearance-theme" aria-describedby="appearance-status" disabled>
            <option value="nord">Nord</option>
            <option value="dracula">Dracula</option>
            <option value="tokyo-night">Tokyo Night</option>
          </select>
        </div>
        <p id="appearance-theme-note" hidden>Dracula light is a DevOption adaptation.</p>
        <div class="mode-choice">
          <label for="appearance-mode">Mode</label>
          <select id="appearance-mode" aria-describedby="appearance-status" disabled>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
            <option value="system">System</option>
          </select>
        </div>
        <div class="mode-choice">
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

const SIGN_IN: &str = r#"
      <section class="auth" aria-label="Account access">
        <form id="signin-form" action="/auth/sign-in" method="post">
          <label for="auth-password">Password</label>
          <input id="auth-password" name="password" type="password" autocomplete="current-password" required maxlength="1024" aria-describedby="auth-status" disabled>
          <button id="auth-submit" class="action-button" type="submit" disabled>Sign in</button>
        </form>
        <div id="auth-signed-in" hidden>
          <a class="action-button" href="/">Continue to Home</a>
          <button class="action-button secondary" type="button" data-signout disabled>Sign out</button>
        </div>
        <a id="auth-recheck" class="action-button" href="/sign-in" hidden>Check account</a>
        <p id="auth-status" role="status">Enable JavaScript to sign in.</p>
      </section>"#;

const SETUP_REQUIRED: &str = r#"
      <section class="auth" aria-label="Owner setup required">
        <p>Create the first owner from a local or SSH terminal, then restart DevOption.</p>
        <p>Browser account creation is not available.</p>
      </section>"#;

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
    render_page(Page::Projects, true, Some((projects, attachment_available)))
}

fn render_page(
    page: Page,
    account_available: bool,
    projects: Option<(&[ProjectSummary<'_>], bool)>,
) -> String {
    // Dynamic project content is escaped by its renderer; other values are static.
    let (title, path, message) = match page {
        Page::Home => ("Home", "/", "No home content yet."),
        Page::Projects => (
            "Projects",
            "/projects",
            match projects {
                Some(([], _)) => "No projects yet.",
                Some(_) => "Your saved projects.",
                None => "Project management is not available yet.",
            },
        ),
        Page::About => ("About", "/about", "No about content yet."),
        Page::Appearance => (
            "Appearance",
            "/settings/appearance",
            "Choose how DevOption looks in this browser.",
        ),
        Page::SignIn => (
            "Sign in",
            "/sign-in",
            if account_available {
                "Use the owner password for this server."
            } else {
                "Owner setup is required before you can sign in."
            },
        ),
    };
    let project_content =
        projects.map(|(projects, available)| projects::content(projects, available));
    let (appearance_current, content) = if let Some(content) = project_content.as_deref() {
        ("", content)
    } else if path == "/settings/appearance" {
        (" aria-current=\"page\"", APPEARANCE_CONTROLS)
    } else if path == "/sign-in" {
        (
            "",
            if account_available {
                SIGN_IN
            } else {
                SETUP_REQUIRED
            },
        )
    } else {
        ("", "")
    };
    let navigation: String = [
        ("/", "Home", "M3 10 12 3l9 7v11h-6v-7H9v7H3Z"),
        ("/projects", "Projects", "M3 7V4h6l3 3h9v13H3Z"),
        (
            "/about",
            "About",
            "M12 16v-4M12 8h.01M21 12a9 9 0 1 1-18 0a9 9 0 0 1 18 0",
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
</head>
<body data-owner-configured=\"{account_available}\">
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
        <p id=\"profile-note\" class=\"profile-note\">Sign in to your local account.</p>
        <a id=\"profile-account\" class=\"profile-link\" href=\"/sign-in\">Sign in</a>
        <button class=\"profile-link\" type=\"button\" data-signout hidden disabled>Sign out</button>
        <p id=\"profile-status\" class=\"profile-note\" role=\"status\"></p>
        <a class=\"profile-link\" href=\"/settings/appearance\"{appearance_current}>Appearance</a>
      </section>
    </div>
  </aside>
  <div class=\"workspace\">
    <header class=\"content-header\"><p class=\"stage\"><span aria-hidden=\"true\"></span>In development</p></header>
    <main id=\"main-content\" tabindex=\"-1\">
      <h1 id=\"page-title\" tabindex=\"-1\">{title}</h1>
      <p id=\"page-message\">{message}</p>
      {content}
    </main>
  </div>
</body>
</html>"
    )
}

fn icon(path: &str) -> String {
    format!(
        "<svg class=\"icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\" focusable=\"false\"><path d=\"{path}\"/></svg>"
    )
}
