use super::{ProjectSection, ProjectSummary, escape_into, identifier};
#[derive(Default)]
pub struct DeploymentView {
    pub enabled: bool,
    pub status: String,
    pub port: u16,
    pub current: Option<String>,
    pub can_stop: bool,
    pub log: String,
    pub releases: Vec<ReleaseView>,
    pub builds: Vec<(String, String)>,
    pub review: Option<(String, String, String, String)>,
}
pub struct ReleaseView {
    pub provenance: String,
    pub id: String,
    pub commit: String,
    pub job: String,
    pub checksum: String,
}
fn escaped(value: &str) -> String {
    let mut html = String::new();
    escape_into(&mut html, value);
    html
}
pub(crate) fn content(project: &ProjectSummary<'_>, view: &DeploymentView) -> String {
    let base = format!("/projects/{}/releases", identifier(&project.id));
    let mut html = super::workspace::navigation(project, ProjectSection::Releases);
    html.push_str(&format!(r#"<section class="build-preparation" id="app-deployment" data-base="{base}"><h2>Run your web app</h2><p id="deployment-action-status" role="status">{}</p>"#, escaped(&view.status)));
    if view.enabled {
        if let Some(current) = &view.current {
            html.push_str(&format!("<dl class=\"build-identities\"><div><dt>Selected release</dt><dd><code>{}</code></dd></div><div><dt>Private app endpoint on this node</dt><dd><code>http://127.0.0.1:{}</code></dd></div></dl>",escaped(current),view.port));
        }
        html.push_str(r#"<p>One app runs on this node. Updates can have downtime. Public hostname routing is not configured here.</p><a class="text-button" href="">Refresh health and logs</a> "#);
        if view.can_stop {
            html.push_str(
                r#"<button class="text-button" data-deploy-action="stop">Stop app</button>"#,
            );
        }
        if let Some((token, release, health, fingerprint)) = &view.review {
            html.push_str(&format!(r#"<h3>Review deployment</h3><dl class="build-identities"><div><dt>Release</dt><dd><code>{}</code></dd></div><div><dt>Health check</dt><dd><code>GET /healthz → 200 {}</code></dd></div><div><dt>Private listener</dt><dd><code>127.0.0.1:{}</code></dd></div><div><dt>Installed runtime configuration SHA-256</dt><dd><code>{}</code></dd></div></dl><p>WasiHttpV1: Wasmtime 49.0.3. No guest files, environment or outbound network. One request at a time, 64 MiB guest memory, 10 million fuel units and five seconds per request. Worker: one CPU, 512 MiB RAM, no swap, 64 tasks. Startup health must pass in 30 seconds.</p><p>This replaces the current app. A healthy approved app resumes after platform restart; a stopped app stays stopped. Approval is single-use and expires in five minutes.</p><button class="action-button" data-deploy-action="approve" data-value="{}">Approve and deploy</button>"#, escaped(release),escaped(health),view.port,escaped(fingerprint),escaped(token)));
        }
        html.push_str("<h3>Saved releases</h3><p>Select a release and the exact response expected from its health check. To return to an earlier version, review and deploy that saved release again.</p>");
        if view.releases.is_empty() {
            html.push_str("<p>No saved releases yet. Promote a successful build below.</p>");
        }
        for release in &view.releases {
            let id = escaped(&release.id);
            html.push_str(&format!(r#"<details class="build-source-details"><summary>Commit {}</summary><dl class="build-identities"><div><dt>Release</dt><dd><code>{id}</code></dd></div><div><dt>Artifact SHA-256</dt><dd><code>{}</code></dd></div><div><dt>Build</dt><dd><a href="/projects/{}/builds/attempts/{}">Open build and logs</a></dd></div></dl><label>Expected health response <input data-health="{id}" type="text" maxlength="128" placeholder="ok A" autocomplete="off"></label> <button class="action-button" data-deploy-action="prepare" data-value="{id}">Review deployment</button>"#,escaped(&release.commit),escaped(&release.checksum),identifier(&project.id),escaped(&release.job)));
            html.push_str(
                "<details class=\"build-source-details\"><summary>Build provenance</summary><pre>",
            );
            escape_into(&mut html, &release.provenance);
            html.push_str("</pre></details></details>");
        }
        html.push_str("<h3>Successful builds</h3><p>Promotion preserves the component and its build provenance as a local release. It does not run the app. Builds made before this deployment feature need rebuilding to supply complete provenance.</p>");
        if view.builds.is_empty() {
            html.push_str("<p>No successful builds available.</p>");
        }
        for (job, commit) in &view.builds {
            html.push_str(&format!(r#"<p><code>{}</code> <button class="text-button" data-deploy-action="promote" data-value="{}">Save release</button></p>"#,escaped(commit),escaped(job)));
        }
        html.push_str("<details class=\"build-source-details\"><summary>App logs (latest saved output)</summary><pre>");
        escape_into(
            &mut html,
            if view.log.is_empty() {
                "No guest output recorded."
            } else {
                &view.log
            },
        );
        html.push_str("</pre></details>");
    }
    html.push_str("</section>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_review_and_logs_are_escaped_and_approval_is_separate() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "App",
            repositories: vec![],
        };
        let mut view = DeploymentView::default();
        assert!(!content(&project, &view).contains("data-deploy-action"));
        view.enabled = true;
        view.log = "<script>private</script>".into();
        view.releases.push(ReleaseView {
            id: "ab".repeat(32),
            commit: "<commit>".into(),
            job: "cd".repeat(16),
            checksum: "ef".repeat(32),
            provenance: "<manifest>".into(),
        });
        let page = content(&project, &view);
        assert!(page.contains("&lt;script&gt;private&lt;/script&gt;"));
        assert!(page.contains("&lt;manifest&gt;"));
        assert!(!page.contains("Approve and deploy"));
        view.review = Some((
            "ab".repeat(16),
            "cd".repeat(32),
            "<health>".into(),
            "ef".repeat(32),
        ));
        let page = content(&project, &view);
        assert!(page.contains("Approve and deploy"));
        assert!(page.contains("&lt;health&gt;"));
    }
}
