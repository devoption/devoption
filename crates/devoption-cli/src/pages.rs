use axum::response::Html;

pub enum Page {
    Home,
    About,
}

const STYLES: &str = "
body { margin: 0 auto; max-width: 60rem; padding: 1rem; font: 1rem/1.6 system-ui, sans-serif; color: #222; background: #fff; }
a { color: inherit; text-underline-offset: 0.2em; }
a:focus-visible { outline: 2px solid currentColor; outline-offset: 4px; }
header { display: flex; flex-wrap: wrap; align-items: center; gap: 1rem 2rem; padding: 1rem 0; border-bottom: 1px solid #ccc; }
nav { display: flex; gap: 1.5rem; }
nav a[aria-current] { font-weight: bold; text-decoration-thickness: 0.15em; }
main { padding: 2rem 0; }
h1 { line-height: 1.2; }
footer { border-top: 1px solid #ccc; padding-top: 1rem; }
";

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
  <a href=\"#main-content\">Skip to content</a>
  <header>
    <strong>DevOption</strong>
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
