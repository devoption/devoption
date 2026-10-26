mod review;

use super::{ProjectSection, ProjectSummary, escape_into, identifier};
#[derive(Default)]
pub struct DeploymentView {
    pub enabled: bool,
    pub ingress: Option<(String, String)>,
    pub bound_elsewhere: bool,
    pub status: String,
    pub port: u16,
    pub current: Option<String>,
    pub current_state: Option<(String, String)>,
    pub replaces_other_app: bool,
    pub can_stop: bool,
    pub log: String,
    pub releases: Vec<ReleaseView>,
    pub builds: Vec<(String, String)>,
    pub review: Option<(String, String, String, String)>,
    pub review_records: Option<String>,
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
    let reviewing = view.enabled && view.review.is_some();
    if reviewing {
        html.push_str(&format!(r#"<section class="deployment-review" id="app-deployment" data-base="{base}" aria-labelledby="deployment-review-title">"#));
        html.push_str(&review::content(view));
    } else {
        html.push_str(&format!(r#"<section class="build-preparation" id="app-deployment" data-base="{base}"><h2>Run your web app</h2><p id="deployment-action-status" role="status">{}</p>"#, escaped(&view.status)));
    }
    if view.enabled {
        if !reviewing {
            if view.bound_elsewhere {
                html.push_str("<p>This node's app hostname belongs to another project. An operator must reconfigure the binding before deploying this project.</p>");
            }
            if let Some((url, status)) = &view.ingress {
                html.push_str(&format!(r#"<h3>App hostname</h3><p><a href="{}" target="_blank" rel="noopener noreferrer">Open app: {}</a></p><p>{}</p><p>Hostname changes are operator-managed. <a href="https://github.com/devoption/devoption/blob/main/docs/app-hostnames.md" target="_blank" rel="noopener noreferrer">App hostname setup</a>.</p>"#,escaped(url),escaped(url),escaped(status)));
            }
            if let Some(current) = &view.current {
                html.push_str(&format!("<dl class=\"build-identities\"><div><dt>Selected release</dt><dd><code>{}</code></dd></div><div><dt>Private app endpoint on this node</dt><dd><code>http://127.0.0.1:{}</code></dd></div></dl>",escaped(current),view.port));
            }
            html.push_str(r#"<p>One app runs on this node. Updates can have downtime.</p><a class="text-button" href="">Refresh health and logs</a> "#);
            if view.can_stop {
                html.push_str(
                    r#"<button class="text-button" data-deploy-action="stop">Stop app</button>"#,
                );
            }
        } else {
            html.push_str(
                r#"<div class="deployment-saved-releases" id="deployment-saved-releases">"#,
            );
        }
        html.push_str("<h3>Saved releases</h3><p>Select a release and the exact response expected from its health check. To return to an earlier version, review and deploy that saved release again.</p>");
        if view.releases.is_empty() {
            html.push_str("<p>No saved releases yet. Promote a successful build below.</p>");
        }
        for release in &view.releases {
            let id = escaped(&release.id);
            html.push_str(&format!(r#"<details class="build-source-details"><summary>Commit {}</summary><dl class="build-identities"><div><dt>Release</dt><dd><code>{id}</code></dd></div><div><dt>Artifact SHA-256</dt><dd><code>{}</code></dd></div><div><dt>Build</dt><dd><a href="/projects/{}/builds/attempts/{}">Open build and logs</a></dd></div></dl>"#,escaped(&release.commit),escaped(&release.checksum),identifier(&project.id),escaped(&release.job)));
            if !view.bound_elsewhere {
                html.push_str(&format!(r#"<label>Expected health response <input data-health="{id}" type="text" maxlength="128" placeholder="ok A" autocomplete="off"></label> <button class="action-button" data-deploy-action="prepare" data-value="{id}">Review deployment</button>"#));
            }
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
        if reviewing {
            html.push_str("</div>");
        }
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
        view.bound_elsewhere = true;
        assert!(!content(&project, &view).contains("data-deploy-action=\"prepare\""));
        view.bound_elsewhere = false;
        view.ingress = Some((
            "https://app.example.test".into(),
            "<certificate failed>".into(),
        ));
        assert!(content(&project, &view).contains("&lt;certificate failed&gt;"));
        view.review = Some((
            "ab".repeat(16),
            "ab".repeat(32),
            "<health>".into(),
            "ef".repeat(32),
        ));
        let page = content(&project, &view);
        assert!(page.contains("Approve and deploy"));
        assert!(page.contains("&lt;health&gt;"));
        view.review_records = Some("<grant>".into());
        let page = content(&project, &view);
        assert!(page.contains("Approval includes this storage access."));
        assert!(page.contains("&lt;grant&gt;"));
    }

    fn sample_release(id: &str, commit: &str) -> ReleaseView {
        ReleaseView {
            id: id.repeat(32),
            commit: format!("{commit}{}", "0".repeat(40 - commit.len())),
            job: "56".repeat(16),
            checksum: "78".repeat(32),
            provenance: "Synthetic saved release; not executed by this fixture.".into(),
        }
    }

    #[test]
    fn review_distinguishes_first_other_stopped_and_missing_releases() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "App",
            repositories: vec![],
        };
        let mut view = DeploymentView {
            enabled: true,
            review: Some((
                "12".repeat(16),
                "ab".repeat(32),
                " ok  app ".into(),
                "34".repeat(32),
            )),
            releases: vec![sample_release("ab", "8c42e1a")],
            ..Default::default()
        };
        let page = content(&project, &view);
        assert!(page.contains("No app deployed"));
        assert!(page.contains("This will start the reviewed app."));
        assert!(page.contains("No database access requested."));
        assert!(page.contains("No app hostname configured."));
        assert!(page.contains("deployment-health-body\"> ok  app </code>"));
        assert!(page.contains("data-value=\"12121212121212121212121212121212\""));
        assert!(!page.contains("<details class=\"deployment-runtime-details\" open"));
        view.replaces_other_app = true;
        assert!(content(&project, &view).contains("This will replace another project’s app."));
        view.replaces_other_app = false;
        view.current = Some("ab".repeat(32));
        view.current_state = Some(("stopped".into(), "stopped".into()));
        let stopped = content(&project, &view);
        assert!(stopped.contains("Stopped on this server"));
        assert!(!stopped.contains("Healthy on this server"));
        assert!(!stopped.contains("first deployment"));
        view.current_state = Some(("failed".into(), "running".into()));
        assert!(content(&project, &view).contains("Health check failed"));
        view.bound_elsewhere = true;
        assert!(!content(&project, &view).contains("data-deploy-action=\"approve\""));
        view.bound_elsewhere = false;
        view.releases.clear();
        let missing = content(&project, &view);
        assert!(!missing.contains("Saved from a successful build"));
        assert!(!missing.contains("data-deploy-action=\"approve\""));
    }

    #[test]
    #[ignore = "explicit synthetic review screenshot fixture"]
    fn manual_records_review_fixture() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "Example application",
            repositories: vec![],
        };
        let mut view = DeploymentView {
            enabled: true,
            port: 15000,
            status: "Healthy saved app; reviewing a storage-enabled candidate.".into(),
            ingress: Some((
                "https://app.example.test".into(),
                "Synthetic screenshot fixture; no external requests or deployment actions.".into(),
            )),
            current: Some("ab".repeat(32)),
            current_state: Some(("healthy".into(), "running".into())),
            can_stop: true,
            review: Some((
                "cd".repeat(16),
                "ef".repeat(32),
                "ok records".into(),
                "12".repeat(32),
            )),
            review_records: Some("34".repeat(32)),
            ..Default::default()
        };
        view.releases = vec![
            sample_release("ab", "74a9c20"),
            sample_release("ef", "8c42e1a"),
        ];
        let path = std::env::var_os("DEVOPTION_REVIEW_CAPTURE_HTML").expect("explicit output path");
        std::fs::write(path, crate::render_deployment(&project, &view)).unwrap();
        if let Some(directory) = std::env::var_os("DEVOPTION_DEPLOYMENT_CAPTURE_DIR") {
            let directory = std::path::Path::new(&directory);
            let write = |name: &str, view: &DeploymentView| {
                std::fs::write(
                    directory.join(name),
                    crate::render_deployment(&project, view),
                )
                .unwrap();
            };
            let review = view.review.take();
            write("release.html", &view);
            let ingress = view.ingress.take();
            view.ingress = Some(("https://app.example.test".into(), "Certificate verification failed. Ask the operator to inspect the hostname and trust configuration. Synthetic presentation fixture.".into()));
            write("hostname.html", &view);
            view.ingress = ingress;
            view.review = review;
            view.review_records = None;
            view.review.as_mut().unwrap().2 = "ok sample-site".into();
            view.status =
                "Healthy on this server. Review the saved candidate before approval.".into();
            write("stateless.html", &view);
            view.current_state = Some(("stopped".into(), "stopped".into()));
            view.status = "Selected app is stopped.".into();
            write("stopped.html", &view);
            view.current_state = Some(("failed".into(), "running".into()));
            view.status = "Selected app failed its health check.".into();
            write("failed.html", &view);
            view.current = None;
            view.current_state = None;
            view.can_stop = false;
            view.ingress = None;
            view.status = "No app deployed yet.".into();
            write("first.html", &view);
            view.replaces_other_app = true;
            view.status = "Another project occupies this node. Deployment will replace it.".into();
            write("other-project.html", &view);
            view.bound_elsewhere = true;
            view.status = "Another project owns this node’s app hostname.".into();
            write("bound-elsewhere.html", &view);
            view.bound_elsewhere = false;
            view.releases.clear();
            view.status = "Saved candidate metadata unavailable. Refresh Releases.".into();
            write("missing-release.html", &view);
            view.enabled = false;
            view.review = None;
            view.status =
                "Install and explicitly enable the Linux app runtime to promote and deploy builds."
                    .into();
            write("unavailable.html", &view);
        }
    }
}
