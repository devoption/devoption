//! Render a deliberately inert Markdown subset. Raw HTML is displayed as text;
//! images become labels and only explicit safe links survive.
use super::{FileEntry, encode_path, escape_into};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

pub(super) fn render(text: &str, path: &str, base: &str, entries: &[FileEntry<'_>]) -> String {
    let mut links = Vec::new();
    let events = Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).map(
        |event| match event {
            Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
            Event::Start(Tag::Image { .. }) => {
                Event::Html("<span class=\"markdown-image\">Image: ".into())
            }
            Event::End(TagEnd::Image) => Event::Html("</span>".into()),
            Event::Start(Tag::Link { dest_url, .. }) => {
                let target = safe_link(&dest_url, path, base, entries);
                links.push(target.is_some());
                let mut tag = String::from("<span>");
                if let Some(target) = target {
                    tag = String::from("<a rel=\"noreferrer\" href=\"");
                    escape_into(&mut tag, &target);
                    tag.push_str("\">");
                }
                Event::Html(tag.into())
            }
            Event::End(TagEnd::Link) => Event::Html(
                if links.pop().unwrap_or(false) {
                    "</a>"
                } else {
                    "</span>"
                }
                .into(),
            ),
            other => other,
        },
    );
    let mut output = String::new();
    html::push_html(&mut output, events);
    output
}

fn safe_link(url: &str, path: &str, base: &str, entries: &[FileEntry<'_>]) -> Option<String> {
    if url.chars().any(|c| c.is_control()) || url.contains('\\') {
        return None;
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(url.to_owned());
    }
    if url.starts_with('/') || url.contains(':') {
        return None;
    }
    let url = url.split('#').next()?;
    if url.is_empty() || url.contains('?') {
        return None;
    }
    // Decode URL path bytes before comparing with the tracked-file allowlist.
    let mut bytes = Vec::new();
    let mut input = url.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    if decoded.starts_with('/') || decoded.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = path
        .rsplit_once('/')
        .map_or(Vec::new(), |(dir, _)| dir.split('/').collect());
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    let target = parts.join("/");
    entries
        .iter()
        .any(|entry| entry.path == target)
        .then(|| format!("{base}/{}", encode_path(&target)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_keeps_formatting_but_never_active_repository_html_or_images() {
        let text = "# Hello\n\n**bold** and ~~old~~\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n![badge](https://example.com/tracker) [bad](javascript:alert%281%29) [data](data:text/html,hi) [protocol](//example.com) [safe](https://example.com)\n\n```rust\nfn main() {}\n```";
        let result = render(text, "README.md", "/files", &[]);
        assert!(result.contains("<h1>Hello</h1>") && result.contains("<strong>bold</strong>"));
        assert!(result.contains("&lt;script&gt;") && result.contains("language-rust"));
        assert!(!result.contains("<script") && !result.contains("<img"));
        assert!(!result.contains("href=\"javascript:") && !result.contains("href=\"data:"));
        assert!(!result.contains("href=\"//"));
        assert!(result.contains("href=\"https://example.com\""));
        assert!(!result.contains("https://example.com/tracker"));
    }

    #[test]
    fn relative_links_only_open_tracked_files_inside_the_repository() {
        let entries = [
            FileEntry {
                path: "src/main.rs",
            },
            FileEntry {
                path: "notes & tips.md",
            },
        ];
        assert_eq!(
            safe_link("../src/main.rs", "docs/README.md", "/files", &entries).as_deref(),
            Some("/files/src/main.rs")
        );
        assert_eq!(
            safe_link("notes%20%26%20tips.md", "README.md", "/files", &entries).as_deref(),
            Some("/files/notes%20%26%20tips.md")
        );
        for link in [
            "../../src/main.rs",
            "untracked",
            "/sign-out",
            "%2fsource",
            "https:\\evil",
            "%GG",
        ] {
            assert!(
                safe_link(link, "README.md", "/files", &entries).is_none(),
                "{link}"
            );
        }
    }
}
