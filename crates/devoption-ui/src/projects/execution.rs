use super::{ProjectSection, ProjectSummary, escape_into, identifier};
pub struct BuildExecutionView {
    pub repository: [u8; 16],
    pub token: Option<String>,
    pub review: Vec<(String, String)>,
    pub running: bool,
    pub available: bool,
    pub message: String,
}
pub(crate) fn content(project: &ProjectSummary<'_>, view: &BuildExecutionView) -> String {
    let base = format!(
        "/projects/{}/builds/{}",
        identifier(&project.id),
        identifier(&view.repository)
    );
    let mut html = super::workspace::navigation(project, ProjectSection::Builds);
    html.push_str(&format!(r#"<section class="build-preparation" id="build-execution" data-base="{base}"><h2>Build web app</h2><p>Build the committed app into a Wasm component. This does not deploy or run the app.</p><p id="build-action-status" role="status">"#));
    escape_into(&mut html, &view.message);
    html.push_str("</p>");
    if view.running {
        html.push_str(r#"<button class="action-button" data-build-action="cancel">Cancel build</button><p>Cancellation is complete only when the saved attempt says Cancelled.</p>"#);
    } else if let Some(token) = &view.token {
        html.push_str("<h3>Review this build</h3><dl class=\"build-identities\">");
        for (index, (label, value)) in view.review.iter().enumerate() {
            if index == 5 {
                html.push_str("</dl><details class=\"build-source-details\"><summary>Input identities and environment</summary><dl class=\"build-identities\">");
            }
            html.push_str("<div><dt>");
            escape_into(&mut html, label);
            html.push_str("</dt><dd><pre>");
            escape_into(&mut html, value);
            html.push_str("</pre></dd></div>");
        }
        html.push_str("</dl>");
        if view.review.len() > 5 {
            html.push_str("</details>");
        }
        html.push_str("<p>Only build trusted source and dependencies. Approval is single-use and expires after five minutes. Changed source or prepared inputs require a new review.</p><button class=\"action-button\" data-build-action=\"approve\" data-token=\"");
        escape_into(&mut html, token);
        html.push_str("\">Approve and build</button> <button class=\"text-button\" data-build-action=\"prepare\">Prepare new review</button>");
    } else if view.available {
        html.push_str(r#"<button class="action-button" data-build-action="prepare">Prepare build review</button>"#);
    }
    html.push_str(&format!(r#"<p><a class="text-button" href="/projects/{}/builds">Open build history and logs</a> · <a class="text-button" href="{base}">Check source</a></p></section>"#,identifier(&project.id)));
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_escapes_inputs_and_separates_approval_from_preparation() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "<project>",
            repositories: vec![],
        };
        let mut view = BuildExecutionView {
            repository: [2; 16],
            token: None,
            review: vec![],
            running: false,
            available: false,
            message: "<unavailable>".into(),
        };
        let unavailable = content(&project, &view);
        assert!(unavailable.contains("&lt;unavailable&gt;"));
        assert!(!unavailable.contains("data-build-action"));
        view.available = true;
        assert!(content(&project, &view).contains("Prepare build review"));
        view.token = Some("ab".repeat(16));
        view.review = vec![("<input>".into(), "<script>source</script>".into()); 6];
        let review = content(&project, &view);
        assert!(review.contains("&lt;script&gt;source&lt;/script&gt;"));
        assert!(review.contains("Input identities and environment"));
        assert!(review.contains("Approve and build"));
        assert!(review.contains("Prepare new review"));
        view.running = true;
        let active = content(&project, &view);
        assert!(active.contains("Cancel build"));
        assert!(!active.contains("data-token"));
    }
}
