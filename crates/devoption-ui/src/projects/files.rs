mod markdown;
use super::{ProjectSection, ProjectSummary, escape_into, identifier};
use std::collections::BTreeMap;

pub struct FileEntry<'a> {
    pub path: &'a str,
}
pub struct FilePreview<'a> {
    pub path: &'a str,
    pub text: Option<&'a str>,
    pub message: &'a str,
}
pub struct RepositoryFilesView<'a> {
    pub attachment_available: bool,
    pub creation_available: bool,
    pub repository_id: [u8; 16],
    pub repository_name: &'a str,
    pub entries: Vec<FileEntry<'a>>,
    pub selected: Option<FilePreview<'a>>,
    pub unavailable: Option<&'a str>,
}

pub(crate) fn content(project: &ProjectSummary<'_>, files: &RepositoryFilesView<'_>) -> String {
    let project_id = identifier(&project.id);
    let base = format!(
        "/projects/{project_id}/repositories/{}/files",
        identifier(&files.repository_id)
    );
    let mut html = super::workspace::navigation(project, ProjectSection::Code);
    html.push_str(r#"<section class="repository-files" aria-labelledby="repository-files-title">"#);
    if project.repositories.len() > 1 {
        html.push_str(r#"<div class="repository-picker" role="group" aria-label="Choose repository"><span>Repository</span>"#);
        for repository in &project.repositories {
            let current = if repository.id == files.repository_id {
                r#" aria-current="true""#
            } else {
                ""
            };
            html.push_str(&format!(r#"<a class="text-button" href="/projects/{project_id}/repositories/{}/files"{current}>"#, identifier(&repository.id)));
            escape_into(&mut html, repository.name);
            html.push_str("</a>");
        }
        html.push_str("</div>");
    }
    html.push_str(r#"<h2 id="repository-files-title">"#);
    escape_into(&mut html, files.repository_name);
    html.push_str(r#" · Files</h2><p class="file-context">Tracked files in the current working tree. Content may include uncommitted changes.</p>"#);
    if let Some(message) = files.unavailable {
        html.push_str("<p role=\"status\">");
        escape_into(&mut html, message);
        html.push_str("</p>");
    } else if files.entries.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No tracked files</h3><p>Files staged with Git will appear here. Untracked files are not shown.</p></div>"#);
    } else {
        html.push_str(r#"<div class="code-browser"><details class="file-tree" open><summary>Files</summary><nav aria-label="Repository files">"#);
        html.push_str(&tree(
            &files.entries,
            files.selected.as_ref().map(|file| file.path),
            &base,
        ));
        html.push_str(r#"</nav></details><div class="file-viewer">"#);
        if let Some(selected) = &files.selected {
            html.push_str(r#"<header class="file-toolbar"><h3 class="file-path">"#);
            escape_into(&mut html, selected.path);
            html.push_str("</h3>");
            if let Some(text) = selected.text {
                html.push_str(&format!(
                    r#"<span>{} lines · {} bytes</span>"#,
                    text.lines().count(),
                    text.len()
                ));
            }
            html.push_str("</header>");
            if let Some(text) = selected.text {
                let is_markdown = selected.path.to_ascii_lowercase().ends_with(".md")
                    || selected.path.to_ascii_lowercase().ends_with(".markdown");
                if is_markdown {
                    html.push_str(r#"<div class="markdown-file"><fieldset class="file-display"><legend class="visually-hidden">File display</legend><input class="visually-hidden" type="radio" name="file-display" id="file-preview-mode" checked><label for="file-preview-mode">Preview</label><input class="visually-hidden" type="radio" name="file-display" id="file-source-mode"><label for="file-source-mode">Source</label></fieldset><article class="markdown-preview" aria-label="Markdown preview">"#);
                    html.push_str(&markdown::render(
                        text,
                        selected.path,
                        &base,
                        &files.entries,
                    ));
                    html.push_str(r#"</article><div class="markdown-source">"#);
                    source(&mut html, text, selected.path);
                    html.push_str("</div></div>");
                } else {
                    source(&mut html, text, selected.path);
                }
                if text.is_empty() {
                    html.push_str(r#"<p class="file-message">This file is empty.</p>"#);
                }
            } else {
                html.push_str(r#"<p class="file-message" role="status">"#);
                escape_into(&mut html, selected.message);
                html.push_str("</p>");
            }
        } else {
            html.push_str(r#"<div class="file-welcome"><h3>Explore this repository</h3><p>Choose a file from the tree to read its contents.</p><p>A README will appear here when one is tracked at the repository root.</p></div>"#);
        }
        html.push_str("</div></div>");
    }
    html.push_str("</section>");
    html.push_str(&super::workspace::repositories(
        project,
        files.attachment_available,
        files.creation_available,
    ));
    html.push_str("<script>");
    html.push_str(include_str!("../../assets/vendor/highlight.min.js"));
    html.push_str("</script><script>");
    html.push_str(include_str!("../../assets/code-browser.js"));
    html.push_str("</script>");
    html
}

#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<&'a str, Folder<'a>>,
    files: BTreeMap<&'a str, &'a str>,
}

fn tree(entries: &[FileEntry<'_>], selected: Option<&str>, base: &str) -> String {
    let mut root = Folder::default();
    for entry in entries {
        let mut folder = &mut root;
        let mut parts = entry.path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                folder.files.insert(part, entry.path);
            } else {
                folder = folder.folders.entry(part).or_default();
            }
        }
    }
    let mut html = String::new();
    render_folder(&mut html, &root, "", selected, base);
    html
}

fn render_folder(
    html: &mut String,
    folder: &Folder<'_>,
    prefix: &str,
    selected: Option<&str>,
    base: &str,
) {
    html.push_str(r#"<ul class="tracked-files">"#);
    for (name, child) in &folder.folders {
        let path = format!("{prefix}{name}/");
        let open = if selected.is_some_and(|selected| selected.starts_with(&path)) {
            " open"
        } else {
            ""
        };
        html.push_str(&format!("<li><details{open}><summary>"));
        html.push_str(&crate::icon("M3 7V4h6l3 3h9v13H3Z"));
        escape_into(html, name);
        html.push_str("</summary>");
        render_folder(html, child, &path, selected, base);
        html.push_str("</details></li>");
    }
    for (name, path) in &folder.files {
        let current = if selected == Some(*path) {
            r#" aria-current="page""#
        } else {
            ""
        };
        html.push_str(&format!(
            r#"<li><a href="{base}/{}"{current} title=""#,
            encode_path(path)
        ));
        escape_into(html, path);
        html.push_str(r#""><span aria-hidden="true">"#);
        html.push_str(&crate::icon("M5 3h9l5 5v13H5Z M14 3v6h5"));
        html.push_str("</span><span>");
        escape_into(html, name);
        html.push_str("</span></a></li>");
    }
    html.push_str("</ul>");
}

fn source(html: &mut String, text: &str, path: &str) {
    html.push_str(r#"<pre class="file-content" tabindex="0" role="region" aria-label="File contents"><span class="line-numbers" aria-hidden="true">"#);
    for line in 1..=text.lines().count().max(1) {
        html.push_str(&format!("{line}\n"));
    }
    html.push_str(r#"</span><code data-source-path=""#);
    escape_into(html, path);
    html.push_str(r#"">"#);
    escape_into(html, text);
    html.push_str("</code></pre>");
}

fn encode_path(path: &str) -> String {
    let mut result = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_links_encode_paths_and_previews_escape_source_in_the_project_shell() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "<Project>",
            repositories: vec![],
        };
        let mut files = RepositoryFilesView {
            attachment_available: true,
            creation_available: false,
            repository_id: [2; 16],
            repository_name: "<Repository>",
            entries: vec![FileEntry {
                path: "src/工具 #?%&.rs",
            }],
            selected: None,
            unavailable: None,
        };
        let html = crate::render_repository_files(&project, &files);
        assert!(html.contains("/files/src/%E5%B7%A5%E5%85%B7%20%23%3F%25%26.rs\""));
        assert!(html.contains("工具 #?%&amp;.rs</span>"));
        assert!(html.contains("&lt;Repository&gt; · Files"));
        assert!(html.contains("/code\" aria-current=\"page\""));
        files.selected = Some(FilePreview {
            path: "<source>",
            text: Some("</code><script>alert('fixture')</script>&"),
            message: "",
        });
        let html = crate::render_repository_files(&project, &files);
        assert!(html.contains("aria-label=\"File contents\""));
        assert!(
            html.contains(
                "&lt;/code&gt;&lt;script&gt;alert(&#39;fixture&#39;)&lt;/script&gt;&amp;"
            )
        );
        assert!(!html.contains("<script>alert("));
        assert!(html.contains("&lt;source&gt;</h3>"));
        files.selected = None;
        files.entries.clear();
        assert!(content(&project, &files).contains("No tracked files"));
        files.unavailable = Some("Storage unavailable");
        let unavailable = content(&project, &files);
        assert!(unavailable.contains("Storage unavailable"));
        assert!(!unavailable.contains("No tracked files"));
    }
}
