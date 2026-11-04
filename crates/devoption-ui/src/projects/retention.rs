use super::*;

#[derive(Default)]
pub struct RetentionView {
    pub count: u16,
    pub configured: bool,
    pub unavailable: bool,
    pub status_unavailable: bool,
    pub retained: Option<usize>,
    pub protected: Option<usize>,
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
            let protection = match view.protected {
                Some(count) => format!("{count} protected exceptions"),
                None => "protected count unavailable".to_owned(),
            };
            html.push_str(&format!("<p>Last cleanup: {retained} retained attempts; {protection}. {} attempts removed; {} bytes reclaimed by successful removals.</p>", view.removed, view.reclaimed_bytes));
        } else if view.failed {
            html.push_str(
                "<p>Retained and protected counts could not be verified for this cleanup.</p>",
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_cleanup_keeps_verified_counts_and_distinguishes_unavailable_evidence() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "Fixture",
            repositories: vec![],
        };
        let mut view = RetentionView {
            count: 1,
            configured: true,
            retained: Some(12),
            protected: Some(3),
            removed: 1,
            reclaimed_bytes: 24_576,
            failed: true,
            ..Default::default()
        };
        let html = content(&project, &view);
        assert!(html.contains("12 retained attempts; 3 protected exceptions"));
        assert!(html.contains("Cleanup is incomplete"));
        view.protected = None;
        let html = content(&project, &view);
        assert!(html.contains("protected count unavailable"));
        assert!(!html.contains("0 protected exceptions"));
        view.retained = None;
        assert!(content(&project, &view).contains("counts could not be verified"));
        view.status_unavailable = true;
        let html = content(&project, &view);
        assert!(html.contains("The policy is saved, but the cleanup result is unavailable"));
        assert!(html.contains("value=\"1\""));
        assert!(!html.contains("24576 bytes reclaimed"));
    }
}
