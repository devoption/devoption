use super::{ProjectSection, ProjectSummary, escape_into, identifier};

pub struct BuildAttemptView {
    pub id: [u8; 16],
    pub repository: String,
    pub commit: String,
    pub source: String,
    pub state: &'static str,
    pub explanation: &'static str,
}
pub enum HistoryAvailability {
    Available,
    Unconfigured,
    Unavailable,
}
pub enum BuildLogView {
    Missing,
    Unavailable,
    Saved {
        stdout: String,
        stderr: String,
        bytes: usize,
        truncated: bool,
        limit_reached: bool,
    },
}
pub struct BuildHistoryView {
    pub attempts: Vec<BuildAttemptView>,
    pub availability: HistoryAvailability,
    pub selected: Option<[u8; 16]>,
    pub log: BuildLogView,
    pub source_available: bool,
}
pub(crate) fn content(project: &ProjectSummary<'_>, view: &BuildHistoryView) -> String {
    let mut html = super::workspace::navigation(project, ProjectSection::Builds);
    let base = format!("/projects/{}/builds", identifier(&project.id));
    html.push_str("<section class=\"build-history\" aria-labelledby=\"history-title\">");
    if let Some(id) = view.selected {
        html.push_str(&format!(
            "<a class=\"text-button\" href=\"{base}\">← Back to Builds</a>"
        ));
        html.push_str("<h2 id=\"history-title\">Build attempt</h2>");
        if let Some(attempt) = view.attempts.iter().find(|a| a.id == id) {
            html.push_str("<div class=\"build-attempt-heading\"><h3>");
            escape_into(&mut html, &attempt.repository);
            html.push_str("</h3><span class=\"build-state\">");
            escape_into(&mut html, attempt.state);
            html.push_str("</span></div><p class=\"preparation-note\">");
            escape_into(&mut html, attempt.explanation);
            html.push_str("</p><details class=\"build-source-details\"><summary>Source and attempt details</summary><dl class=\"build-identities\">");
            for (label, value) in [
                ("Attempt", identifier(&id)),
                ("Commit", attempt.commit.clone()),
                ("Source snapshot SHA-256", attempt.source.clone()),
            ] {
                html.push_str("<div><dt>");
                escape_into(&mut html, label);
                html.push_str("</dt><dd><code>");
                escape_into(&mut html, &value);
                html.push_str("</code></dd></div>");
            }
            html.push_str("</dl></details><h3>Saved output</h3><p class=\"preparation-note\">This is the last saved checkpoint, not a live stream. Output alone does not prove completion. No artifact download is available.</p>");
            match &view.log {
                BuildLogView::Missing => {
                    html.push_str("<p>No output checkpoint was saved for this attempt.</p>")
                }
                BuildLogView::Unavailable => html.push_str(
                    "<p role=\"status\">Saved output could not be loaded. Reload to try again.</p>",
                ),
                BuildLogView::Saved {
                    stdout,
                    stderr,
                    bytes,
                    truncated,
                    limit_reached,
                } => {
                    html.push_str(&format!(
                        "<p>{bytes} bytes retained across stdout and stderr.</p>"
                    ));
                    if *truncated {
                        html.push_str("<p class=\"build-output-notice\">Output was truncated. Bytes beyond the combined limit were discarded.</p>");
                    } else if *limit_reached {
                        html.push_str("<p class=\"build-output-notice\">The output limit was reached. No discarded bytes were recorded.</p>");
                    }
                    for (label, text) in [("Standard output", stdout), ("Standard error", stderr)] {
                        html.push_str("<h4>");
                        escape_into(&mut html, label);
                        html.push_str("</h4>");
                        if text.is_empty() {
                            html.push_str(
                                "<p class=\"preparation-note\">No bytes saved for this stream.</p>",
                            );
                        } else {
                            html.push_str("<pre class=\"build-log\" tabindex=\"0\" aria-label=\"");
                            escape_into(&mut html, label);
                            html.push_str("\"><code>");
                            escape_into(&mut html, text);
                            html.push_str("</code></pre>");
                        }
                    }
                }
            }
        }
        html.push_str(&format!(
            "<a class=\"text-button\" href=\"{base}/attempts/{}\">Reload saved attempt</a>",
            identifier(&id)
        ));
    } else {
        html.push_str(
            "<div class=\"section-heading\"><h2 id=\"history-title\">Saved attempts</h2>",
        );
        html.push_str(&format!(
            "<a class=\"text-button\" href=\"{base}\">Refresh history</a></div>"
        ));
        match view.availability {
            HistoryAvailability::Unconfigured => {
                html.push_str("<p>Build history is not configured for this server.</p>")
            }
            HistoryAvailability::Unavailable => html.push_str(
                "<p role=\"status\">Build history could not be loaded. Reload to try again.</p>",
            ),
            HistoryAvailability::Available if view.attempts.is_empty() => {
                html.push_str("<p>No saved build attempts for this project.</p>")
            }
            HistoryAvailability::Available => {
                html.push_str("<p class=\"preparation-note\">Saved records, not live process status. Open an attempt to inspect its source and last output checkpoint.</p><ul class=\"build-attempts\" aria-label=\"Saved build attempts\">");
                for attempt in view.attempts.iter().rev() {
                    html.push_str(&format!("<li><a href=\"{base}/attempts/{}\"><div class=\"build-attempt-heading\"><h3>", identifier(&attempt.id)));
                    escape_into(&mut html, &attempt.repository);
                    html.push_str("</h3><span class=\"build-state\">");
                    escape_into(&mut html, attempt.state);
                    html.push_str("</span></div><p>Commit <code>");
                    escape_into(&mut html, &attempt.commit);
                    html.push_str(
                        "</code></p><span class=\"text-button\">View attempt →</span></a></li>",
                    );
                }
                html.push_str("</ul>");
            }
        }
    }
    html.push_str("</section>");
    if view.selected.is_none() {
        html.push_str(&super::builds::list(project, view.source_available));
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_renders_distinct_storage_states_and_escapes_output() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "<project>",
            repositories: vec![],
        };
        let mut view = BuildHistoryView {
            attempts: vec![],
            availability: HistoryAvailability::Available,
            selected: None,
            log: BuildLogView::Missing,
            source_available: false,
        };
        assert!(content(&project, &view).contains("No saved build attempts"));
        view.availability = HistoryAvailability::Unconfigured;
        assert!(content(&project, &view).contains("not configured"));
        view.availability = HistoryAvailability::Unavailable;
        assert!(content(&project, &view).contains("could not be loaded"));
        view.availability = HistoryAvailability::Available;
        view.attempts.push(BuildAttemptView {
            id: [2; 16],
            repository: "<repo>".into(),
            commit: "<commit>".into(),
            source: "<source>".into(),
            state: "Interrupted",
            explanation: "No resume.",
        });
        view.selected = Some([2; 16]);
        view.log = BuildLogView::Saved {
            stdout: "<script>bad()</script>&quot;".into(),
            stderr: String::new(),
            bytes: 40,
            truncated: false,
            limit_reached: true,
        };
        let html = crate::render_build_history(&project, &view);
        for escaped in [
            "&lt;repo&gt;",
            "&lt;commit&gt;",
            "&lt;source&gt;",
            "&lt;script&gt;bad()&lt;/script&gt;&amp;quot;",
        ] {
            assert!(html.contains(escaped));
        }
        assert!(!html.contains("<script>bad()"));
        assert!(html.contains("No discarded bytes"));
        assert!(html.contains("No bytes saved for this stream"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(!html.contains(">Run build<"));
    }
}
