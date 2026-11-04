use super::*;

#[derive(Default)]
pub struct RetentionView {
    pub count: u16,
    pub configured: bool,
    pub unavailable: bool,
    pub status_unavailable: bool,
    pub retained: Option<usize>,
    pub protected: usize,
    pub removed: usize,
    pub reclaimed_bytes: u64,
    pub failed: bool,
}

pub(crate) fn content(project: &ProjectSummary<'_>, view: &RetentionView) -> String {
    let mut html =
        workspace::content_with_creation(project, false, false, ProjectSection::Settings);
    let id = identifier(&project.id);
    html.push_str(r#"<section class="project-environment-settings" aria-labelledby="retention-title"><h2 id="retention-title">Build retention</h2><p id="retention-hint">Keep the newest completed build attempts for this project, including failed and cancelled attempts. Saving a lower limit immediately cleans up eligible older artifacts, staged source, compiler output and receipts.</p><p id="retention-protection">Queued and running builds, active promotions, saved releases and their deployment or recovery evidence are protected. Attempts with unconfirmed process cleanup are also kept. Protected exceptions can exceed your limit. Compact audit metadata remains after cleanup.</p>"#);
    if view.unavailable {
        html.push_str("<p role=\"status\">Retention storage is unavailable. No cleanup result can be confirmed.</p>");
    } else if !view.configured {
        html.push_str("<p>Build history is not configured. The default is 10 completed attempts per project; saving a policy requires build history storage.</p>");
    } else {
        html.push_str(&format!(r#"<form id="retention-form" class="project-form" data-project="{id}"><label for="retention-count">Completed attempts to keep</label><input id="retention-count" name="count" type="number" min="1" max="1000" step="1" required value="{}" aria-describedby="retention-hint retention-protection retention-status"><button class="action-button" type="submit">Save retention and clean up</button><p id="retention-status" role="status" aria-live="polite"></p></form><noscript>JavaScript is required to save build retention.</noscript>"#,view.count));
        if view.status_unavailable {
            html.push_str("<p role=\"status\">The policy is saved, but the cleanup result is unavailable. No reclaimed space can be confirmed. Inspect storage and save again to retry.</p>");
        } else if let Some(retained) = view.retained {
            let protection = if view.failed {
                "protection or removal verification incomplete".to_owned()
            } else {
                format!("{} protected exceptions", view.protected)
            };
            html.push_str(&format!("<p>Last cleanup: {retained} retained attempts; {protection}. {} attempts removed; {} bytes reclaimed by successful removals.</p>", view.removed, view.reclaimed_bytes));
        } else {
            html.push_str("<p>No cleanup result has been recorded yet.</p>");
        }
        if view.failed {
            html.push_str("<p role=\"status\">Cleanup is incomplete. Some build files or references could not be verified or removed. Retained evidence is preserved; inspect storage and retry by saving the policy again. No space is claimed for failed removals.</p>");
        }
    }
    html.push_str("</section>");
    html
}
