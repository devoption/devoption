/// The pages currently available in DevOption.
pub enum Page {
    Home,
    Projects,
    About,
}

const STYLES: &str = include_str!("../assets/theme.css");
const APPEARANCE: &str = include_str!("../assets/appearance.js");
const LOGO: &str = include_str!("../assets/devoption.svg");

/// Render a complete HTML document independently of the HTTP server.
pub fn render(page: Page) -> String {
    // All rendered values are static; no request data is interpolated into HTML.
    let (title, path, message) = match page {
        Page::Home => ("Home", "/", "No home content yet."),
        Page::Projects => (
            "Projects",
            "/projects",
            "Project management is not available yet.",
        ),
        Page::About => ("About", "/about", "No about content yet."),
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
  <title>{title} · DevOption</title>
  <style>{STYLES}</style>
  <script>{APPEARANCE}</script>
</head>
<body>
  <a class=\"skip-link\" href=\"#main-content\">Skip to content</a>
  <aside class=\"sidebar\" aria-label=\"Sidebar\">
    <a class=\"brand\" href=\"/\" aria-label=\"DevOption home\">{LOGO}</a>
    <nav aria-label=\"Main navigation\">{navigation}</nav>
    <div class=\"profile-area\">
      <button class=\"profile-trigger\" type=\"button\" popovertarget=\"profile-popup\" aria-label=\"Profile menu\">
        <span class=\"profile-avatar\">{profile_icon}</span>
        <span class=\"profile-label\">Local preview</span><span class=\"chevron\" aria-hidden=\"true\"></span>
      </button>
      <section id=\"profile-popup\" popover aria-labelledby=\"profile-title\">
        <h2 id=\"profile-title\">Local preview</h2>
        <p class=\"profile-note\">Account features are not available yet.</p>
        <details class=\"appearance\">
          <summary>Appearance</summary>
          <p class=\"theme-preview\"><span class=\"theme-swatch\" aria-hidden=\"true\"></span><span><span id=\"appearance-name\">Nord purple</span><small>Current theme</small></span></p>
          <div class=\"mode-choice\">
            <label for=\"appearance-mode\">Mode</label>
            <select id=\"appearance-mode\" aria-describedby=\"appearance-status\" disabled>
              <option value=\"dark\">Dark</option>
              <option value=\"light\">Light</option>
            </select>
          </div>
          <div class=\"mode-choice\">
            <label for=\"appearance-accent\">Accent</label>
            <select id=\"appearance-accent\" aria-describedby=\"appearance-status\" disabled>
              <option value=\"purple\">Purple</option>
              <option value=\"blue\">Blue</option>
              <option value=\"green\">Green</option>
            </select>
          </div>
          <p id=\"appearance-status\" role=\"status\">Enable JavaScript to change appearance.</p>
        </details>
      </section>
    </div>
  </aside>
  <div class=\"workspace\">
    <header class=\"content-header\"><p class=\"stage\"><span aria-hidden=\"true\"></span>In development</p></header>
    <main id=\"main-content\" tabindex=\"-1\">
      <h1>{title}</h1>
      <p>{message}</p>
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
