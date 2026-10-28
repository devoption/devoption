use super::{DeploymentView, ReleaseView, escaped};

pub(super) fn content(project: &super::ProjectSummary<'_>, view: &DeploymentView) -> String {
    let (token, candidate, health, fingerprint) = view.review.as_ref().expect("review selected");
    let mut html = String::from(
        r#"<header class="deployment-review-heading"><h2 id="deployment-review-title">Review deployment</h2><p id="deployment-action-status" role="status" aria-live="polite">"#,
    );
    html.push_str(&escaped(&view.status));
    html.push_str(r#"</p></header><ol class="deployment-review-stages" aria-label="Delivery stages"><li>Source <span>Captured in release</span></li><li>Build <span>Saved artifact</span></li><li>Release <span>Selected for review</span></li><li aria-current="step">Deploy <span>Review and approve</span></li></ol><div class="deployment-comparison"><section aria-labelledby="deployment-current-title"><p class="deployment-eyebrow">Current selection</p><h3 id="deployment-current-title">"#);
    if let Some(current) = &view.current {
        html.push_str("Selected release</h3>");
        release_summary(&mut html, view, current);
        if let Some((observed, _)) = &view.current_state {
            let state = match observed.as_str() {
                "healthy" => "Healthy on this server",
                "stopped" => "Stopped on this server",
                "failed" => "Health check failed",
                "starting" => "Starting",
                "stopping" => "Stopping",
                "integrity failed" => "Release integrity check failed",
                "recovery failed" => "Recovery failed",
                _ => observed,
            };
            html.push_str(&format!(
                "<p class=\"deployment-current-state\">{}</p>",
                escaped(state)
            ));
        } else {
            html.push_str("<p>Health status unavailable.</p>");
        }
    } else if view.replaces_other_app {
        html.push_str("Another project’s app</h3><p>Deployment will replace the app currently selected on this node.</p>");
    } else {
        html.push_str(
            "No app deployed</h3><p>This will be the first deployment in this app slot.</p>",
        );
    }
    html.push_str(r#"</section><section class="deployment-candidate" aria-labelledby="deployment-candidate-title"><p class="deployment-eyebrow">Ready for review</p><h3 id="deployment-candidate-title">New release</h3>"#);
    release_summary(&mut html, view, candidate);
    html.push_str(if find_release(view, candidate).is_some() {
        "<p>Saved from a successful build.</p>"
    } else {
        "<p>Saved release details unavailable. Refresh before approving.</p>"
    });
    html.push_str(r#"</section></div><section class="deployment-checks" aria-labelledby="deployment-checks-title"><h3 id="deployment-checks-title">Deployment checks</h3><p class="deployment-note">The candidate’s health is checked after approval. Public reachability is checked separately.</p><dl><div><dt>Health expectation</dt><dd>GET <code>/healthz</code> must return HTTP 200 with body <code class="deployment-health-body">"#);
    html.push_str(&escaped(health));
    html.push_str("</code></dd></div><div><dt>App access</dt><dd>");
    if view.bound_elsewhere {
        html.push_str("This node’s app hostname belongs to another project. An operator must reconfigure the binding before deployment.");
    } else if let Some((url, status)) = &view.ingress {
        html.push_str(&format!(r#"<a href="{}" target="_blank" rel="noopener noreferrer">{}</a><p class="deployment-note">{}</p>"#, escaped(url), escaped(url), escaped(status)));
    } else {
        html.push_str("No app hostname configured.<p class=\"deployment-note\">The private endpoint is on this server. See runtime details for its address.</p>");
    }
    html.push_str("</dd></div><div><dt>Project destination</dt><dd>");
    html.push_str(&escaped(project.name));
    if view.independent {
        html.push_str("<p class=\"deployment-note\">This project has its own listener and runtime state. This action does not replace another project’s app.</p>");
    }
    html.push_str("</dd></div><div><dt>Storage access</dt><dd>");
    if view.review_records.is_some() {
        html.push_str("Append validated records only.<p class=\"deployment-note\">This release can append to its operator-selected PostgreSQL collection. It cannot read, update, delete or run SQL. Approval includes this storage access.</p>");
    } else {
        html.push_str("No database access requested.");
    }
    html.push_str("</dd></div><div><dt>HTTPS integration</dt><dd>");
    if let Some(binding) = &view.review_https {
        html.push_str(&escaped(binding));
        html.push_str("<p class=\"deployment-note\">POST JSON only. The host adds this secret as bearer authorization; the app cannot read its value. One call per app request, three seconds, no redirects. Approval includes this access.</p>");
    } else {
        html.push_str("No outbound HTTPS access requested.");
    }
    html.push_str(r#"</dd></div></dl></section><details class="deployment-runtime-details"><summary>Runtime limits, release identity &amp; technical details</summary><dl class="build-identities">"#);
    if let Some(current) = &view.current {
        identity(&mut html, "Selected release", current);
        if let Some((observed, intent)) = &view.current_state {
            identity(&mut html, "Observed app state", observed);
            identity(&mut html, "Desired app state", intent);
        }
        if let Some(release) = find_release(view, current) {
            identity(&mut html, "Selected source commit", &release.commit);
        }
    }
    identity(&mut html, "Candidate release", candidate);
    if let Some(release) = find_release(view, candidate) {
        identity(&mut html, "Candidate source commit", &release.commit);
        identity(&mut html, "Candidate artifact SHA-256", &release.checksum);
    }
    identity(
        &mut html,
        "Private listener on this server",
        &format!("127.0.0.1:{}", view.port),
    );
    identity(
        &mut html,
        "Installed runtime configuration SHA-256",
        fingerprint,
    );
    if let Some(grant) = &view.review_records {
        identity(&mut html, "App storage grant SHA-256", grant);
    }
    html.push_str("</dl><p>WasiHttpV1: Wasmtime 49.0.3. No guest files, environment, sockets or DNS. HTTPS is denied unless the integration above is explicitly bound. One request at a time, 64 MiB guest memory, 10 million fuel units and five seconds per request. Worker: one CPU, 512 MiB RAM, no swap, 64 tasks. Startup health must pass in 30 seconds.</p><p>A healthy approved app resumes after platform restart only when its bound grants and exact secret revision remain available; a stopped app stays stopped.</p></details><section class=\"deployment-approval\" aria-labelledby=\"deployment-impact-title\"><h3 id=\"deployment-impact-title\">");
    if view.replaces_other_app {
        html.push_str("This will replace another project’s app.");
    } else if view.current.is_some() {
        html.push_str("This will replace the selected app.");
    } else {
        html.push_str("This will start the reviewed app.");
    }
    html.push_str("</h3><p>The app may be briefly unavailable. Approval is single-use. This review expires five minutes after preparation. Changed configuration or deployment state requires a new review.</p><div class=\"deployment-review-actions\">");
    if !view.bound_elsewhere && find_release(view, candidate).is_some() {
        html.push_str(&format!(r#"<button class="action-button primary" data-deploy-action="approve" data-value="{}">Approve and deploy</button>"#, escaped(token)));
    } else {
        html.push_str("<p>Refresh Releases and resolve the unavailable release or hostname binding before preparing a new review.</p>");
    }
    html.push_str(r##"<a class="text-button" href="#deployment-saved-releases">Choose another release</a></div></section><footer class="deployment-review-tools"><a class="text-button" href="">Refresh health and logs</a>"##);
    if view.can_stop {
        html.push_str(r#"<button class="text-button" data-deploy-action="stop">Stop app</button>"#);
    }
    html.push_str("<p class=\"deployment-note\">Need an earlier version? Select a saved release and review it for deployment again.</p></footer><noscript><p>JavaScript is required to approve deployment or change the app state.</p></noscript>");
    html
}

fn find_release<'a>(view: &'a DeploymentView, id: &str) -> Option<&'a ReleaseView> {
    view.releases.iter().find(|release| release.id == id)
}
fn shortened(value: &str) -> String {
    escaped(&value.chars().take(12).collect::<String>())
}
fn release_summary(html: &mut String, view: &DeploymentView, id: &str) {
    if let Some(release) = find_release(view, id) {
        html.push_str(&format!(
            "<p>Commit <code>{}</code></p>",
            shortened(&release.commit)
        ));
    }
    html.push_str(&format!(
        "<p class=\"deployment-note\">Release <code>{}</code></p>",
        shortened(id)
    ));
}
fn identity(html: &mut String, label: &str, value: &str) {
    html.push_str(&format!(
        "<div><dt>{}</dt><dd><code>{}</code></dd></div>",
        escaped(label),
        escaped(value)
    ));
}
