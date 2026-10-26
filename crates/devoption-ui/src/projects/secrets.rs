use super::{ProjectSection, ProjectSummary, escape_into, identifier};
pub struct SecretMetadataView {
    pub name: String,
    pub revision: u64,
    pub updated: u64,
}
pub struct SecretEventView {
    pub name: String,
    pub action: String,
    pub revision: u64,
    pub updated: u64,
}
pub struct EnvironmentSecretsView {
    pub environment: String,
    pub label: String,
    pub configured: bool,
    pub unavailable: bool,
    pub entries: Vec<SecretMetadataView>,
    pub events: Vec<SecretEventView>,
}
pub(crate) fn content(project: &ProjectSummary<'_>, view: &EnvironmentSecretsView) -> String {
    let environment = match view.environment.as_str() {
        "development" | "preview" | "production" => view.environment.as_str(),
        _ => "development",
    };
    let id = identifier(&project.id);
    let base = format!("/projects/{id}/environments");
    let mut html = super::workspace::navigation(project, ProjectSection::Settings);
    html.push_str(&format!(r#"<section class="environment-secrets" id="environment-secrets" data-base="{base}/{}" aria-labelledby="environment-title"><p><a href="/projects/{id}/settings">Project settings</a> / Environments</p><h2 id="environment-title">Environment secrets</h2><p>Keep separate values for development, preview and production.</p><nav class="environment-tabs" aria-label="Environments">"#,environment));
    for (segment, label) in [
        ("development", "Development"),
        ("preview", "Preview"),
        ("production", "Production"),
    ] {
        let current = if environment == segment {
            r#" aria-current="page""#
        } else {
            ""
        };
        html.push_str(&format!(
            r#"<a href="{base}/{segment}"{current}>{label}</a>"#
        ));
    }
    html.push_str("</nav><h3>");
    escape_into(&mut html, &view.label);
    html.push_str(" secrets</h3>");
    html.push_str(r#"<p class="secrets-scope-note">Values are encrypted and cannot be viewed after saving. Runtime access requires an operator-configured HTTPS binding and a reviewed deployment. Builds never receive these values.</p>"#);
    if !view.configured {
        html.push_str(r#"<p role="status">Secret storage is not enabled on this installation. An operator must configure a protected installation key before you can save values.</p></section>"#);
        return html;
    }
    if view.unavailable {
        html.push_str(r#"<p role="alert">Secrets could not be loaded. No values were changed. Reload to check storage before trying again.</p></section>"#);
        return html;
    }
    html.push_str(r#"<p id="secrets-status" role="status" aria-live="polite"></p><div class="secrets-layout"><div><h4>Saved secrets</h4>"#);
    if view.entries.is_empty() {
        html.push_str("<p>No secrets in this environment yet.</p>")
    } else {
        html.push_str(r#"<ul class="secret-list" aria-label="Saved secrets">"#);
        for entry in &view.entries {
            let mut name = String::new();
            escape_into(&mut name, &entry.name);
            html.push_str(&format!(r#"<li><div class="secret-metadata"><strong><code>{name}</code></strong><p>Revision {} · <time data-secret-time="{}">Update time recorded</time></p></div><details><summary>Replace value for {name}</summary><form data-secret-form="save" data-name="{name}" data-revision="{}"><fieldset disabled><label>New value for {name}<input type="password" name="value" autocomplete="new-password" maxlength="8192" required></label><p>Saving replaces the current value. It remains hidden.</p><button class="action-button" type="submit">Replace secret</button></fieldset></form></details><details><summary>Remove {name}</summary><form data-secret-form="remove" data-name="{name}" data-revision="{}"><fieldset disabled><label><input type="checkbox" required> Remove {name} from this environment</label><p>You will need to enter a value again to restore this secret.</p><button class="action-button" type="submit">Remove secret</button></fieldset></form></details></li>"#,entry.revision,entry.updated,entry.revision,entry.revision));
        }
        html.push_str("</ul>");
    }
    html.push_str(r#"</div><aside class="secret-add"><h4>Add a secret</h4><form data-secret-form="save" data-revision="0"><fieldset disabled><label for="secret-name">Name</label><input id="secret-name" name="name" placeholder="SERVICE_API_KEY" pattern="[A-Z_][A-Z0-9_]{0,63}" maxlength="64" autocomplete="off" required><label for="secret-value">Value</label><input id="secret-value" type="password" name="value" autocomplete="new-password" maxlength="8192" required><p>Up to 64 secrets per environment. To change an existing secret, use its Replace control.</p><button class="action-button primary" type="submit">Save secret</button></fieldset></form></aside></div>"#);
    if !view.events.is_empty() {
        html.push_str(r#"<details class="secret-activity"><summary>Recent changes</summary><ol>"#);
        for event in view.events.iter().rev() {
            html.push_str("<li><code>");
            escape_into(&mut html, &event.name);
            html.push_str("</code> ");
            escape_into(&mut html, &event.action);
            html.push_str(&format!(
                r#" · revision {} · <time data-secret-time="{}">Time recorded</time></li>"#,
                event.revision, event.updated
            ));
        }
        html.push_str("</ol><p>The most recent 100 changes in this environment are retained. Values are never included.</p></details>");
    }
    html.push_str(
        "<noscript>JavaScript is required to save, replace or remove secrets.</noscript></section>",
    );
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_is_escaped_and_no_stored_value_can_be_supplied_to_the_view() {
        let project = ProjectSummary {
            id: [1; 16],
            name: "Fixture",
            repositories: vec![],
        };
        let view = EnvironmentSecretsView {
            environment: "\" onmouseover=\"bad".into(),
            label: "<script>bad</script>".into(),
            configured: true,
            unavailable: false,
            entries: vec![SecretMetadataView {
                name: "<script>bad</script>".into(),
                revision: 3,
                updated: 42,
            }],
            events: vec![],
        };
        let html = content(&project, &view);
        assert!(!html.contains("<script>bad"));
        assert!(!html.contains("onmouseover"));
        assert!(html.contains("&lt;script&gt;bad&lt;/script&gt;"));
        assert!(html.contains("type=\"password\""));
        assert!(!html.contains("value=\""));
        assert!(html.contains("Runtime access requires an operator-configured HTTPS binding"));
    }
}
