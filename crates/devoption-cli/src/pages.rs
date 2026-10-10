use axum::response::Html;

pub enum Page {
    Home,
    About,
}

const STYLES: &str = include_str!("../assets/theme.css");
const LOGO: &str = include_str!("../assets/devoption.svg");

pub fn render(page: Page) -> Html<String> {
    // All rendered values are static; no request data is interpolated into HTML.
    let (title, path, message) = match page {
        Page::Home => ("Home", "/", "No home content yet."),
        Page::About => ("About", "/about", "No about content yet."),
    };
    let navigation: String = [("/", "Home"), ("/about", "About")]
        .into_iter()
        .map(|(href, label)| {
            let current = if href == path {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!("<a href=\"{href}\"{current}>{label}</a>")
        })
        .collect();

    Html(format!(
        "<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\">
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
  <title>{title} · DevOption</title>
  <style>{STYLES}</style>
</head>
<body>
  <a class=\"skip-link\" href=\"#main-content\">Skip to content</a>
  <header>
    <a class=\"brand\" href=\"/\" aria-label=\"DevOption home\">{LOGO}</a>
    <nav aria-label=\"Main navigation\">{navigation}</nav>
  </header>
  <main id=\"main-content\" tabindex=\"-1\">
    <h1>{title}</h1>
    <p>{message}</p>
  </main>
  <footer>In development</footer>
</body>
</html>"
    ))
}
