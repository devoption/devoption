//! Explicit delegated access consent and per-account revocation.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn page(title: &str, content: &str) -> String {
    crate::document(
        title,
        &format!(
            r##"<body class="auth-page" data-owner-configured="true">
    <a class="skip-link" href="#main-content">Skip to MCP access</a>
    <header class="auth-brand"><div class="auth-wordmark" role="img" aria-label="DevOption">{}</div><div class="auth-introduction"><p class="auth-eyebrow">Connected tools</p><p class="auth-statement">Your workspace.<br>Your permission.</p><p>Choose which clients can read your platform.</p></div></header>
    <main id="main-content" class="auth-main" tabindex="-1" aria-labelledby="page-title"><div class="auth-content"><p class="auth-eyebrow">MCP access</p><h1 id="page-title">{title}</h1>{content}<p id="mcp-status" role="status">Enable JavaScript to manage access.</p><p class="auth-help"><a href="/mcp/grants">Manage client access</a> · <a href="/">Open workspace</a></p></div></main>
    <script src="/mcp/consent.js" defer></script></body>"##,
            crate::LOGO
        ),
    )
}
pub fn render_mcp_consent(client: Option<&str>) -> String {
    let content=match client {
        Some(name)=>format!(r#"<p><strong>{}</strong> is requesting read access to this workspace.</p><ul><li>Projects and attached repository identities</li><li>Saved build checkpoints and runtime observations</li><li>Platform configuration availability</li></ul><p>Access covers the shared workspace you can currently view. It expires after 15 minutes and can be revoked at any time.</p><p>Build, deployment and approval actions are unavailable.</p><form id="mcp-consent"><button class="action-button primary" name="decision" value="allow" type="submit" disabled>Allow read access</button> <button class="action-button" name="decision" value="deny" type="submit" disabled>Deny</button></form>"#,escape(name)),
        None=>r#"<p>Sign in to review this client's request. After signing in, return here and reload this page.</p><a class="action-button primary" href="/sign-in" target="_blank" rel="noopener noreferrer">Sign in</a>"#.into(),
    };
    page("Connect a client", &content)
}
pub fn render_mcp_grants(grants: &[(String, String)]) -> String {
    let mut content="<p>These clients can read your shared workspace. Revoking access takes effect on subsequent requests.</p>".to_owned();
    if grants.is_empty() {
        content.push_str("<p>No active client access.</p>");
    }
    for (id, name) in grants {
        content.push_str(&format!(r#"<form class="mcp-revoke"><p><strong>{}</strong></p><input type="hidden" name="grant_id" value="{}"><button class="action-button" type="submit" disabled>Revoke access</button></form>"#,escape(name),escape(id)));
    }
    page("Client access", &content)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_names_are_inert_and_consent_is_explicit() {
        let html = render_mcp_consent(Some("<script>unsafe</script>"));
        assert!(html.contains("&lt;script&gt;unsafe&lt;/script&gt;"));
        assert!(html.contains("Allow read access"));
        assert!(!render_mcp_consent(None).contains("Allow read access"));
    }
}
