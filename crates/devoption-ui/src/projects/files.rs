use super::{ProjectSection, ProjectSummary, escape_into, identifier};

pub struct FileEntry<'a> {
    pub path: &'a str,
}
pub struct FilePreview<'a> {
    pub path: &'a str,
    pub text: Option<&'a str>,
    pub message: &'a str,
}
pub struct RepositoryFilesView<'a> {
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
    html.push_str(&format!(r#"<section class="repository-files" aria-labelledby="repository-files-title"><a class="text-button" href="/projects/{project_id}/code">← Back to repositories</a><h2 id="repository-files-title">"#));
    escape_into(&mut html, files.repository_name);
    html.push_str(r#" · Files</h2><p class="file-context">Tracked files in the current working tree. Content may include uncommitted changes.</p>"#);
    if let Some(message) = files.unavailable {
        html.push_str("<p role=\"status\">");
        escape_into(&mut html, message);
        html.push_str("</p>");
    } else if let Some(selected) = &files.selected {
        html.push_str(&format!(
            r#"<a class="text-button" href="{base}">← All tracked files</a><h3 class="file-path">"#
        ));
        escape_into(&mut html, selected.path);
        html.push_str("</h3>");
        if let Some(text) = selected.text {
            html.push_str(r#"<pre class="file-content" tabindex="0" role="region" aria-label="File contents"><code>"#);
            escape_into(&mut html, text);
            html.push_str("</code></pre>");
            if text.is_empty() {
                html.push_str("<p>This file is empty.</p>");
            }
        } else {
            html.push_str("<p role=\"status\">");
            escape_into(&mut html, selected.message);
            html.push_str("</p>");
        }
    } else if files.entries.is_empty() {
        html.push_str(r#"<div class="empty-state"><h3>No tracked files</h3><p>Files staged with Git will appear here. Untracked files are not shown.</p></div>"#);
    } else {
        html.push_str(r#"<ul class="tracked-files" aria-label="Tracked files">"#);
        for file in &files.entries {
            html.push_str(&format!(
                r#"<li><a href="{base}/{}"><span aria-hidden="true">↳</span><span>"#,
                encode_path(file.path)
            ));
            escape_into(&mut html, file.path);
            html.push_str("</span></a></li>");
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
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
        assert!(html.contains("src/工具 #?%&amp;.rs</span>"));
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
