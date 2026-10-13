const SIGN_IN: &str = r#"
        <section class="auth" aria-label="Account access">
          <form id="signin-form" action="/auth/sign-in" method="post">
            <label for="auth-password">Password</label>
            <input id="auth-password" name="password" type="password" autocomplete="current-password" required maxlength="1024" aria-describedby="auth-status" disabled>
            <button id="auth-submit" class="action-button primary" type="submit" disabled>Sign in</button>
          </form>
          <div id="auth-signed-in" hidden>
            <a class="action-button primary" href="/">Open workspace <span aria-hidden="true">→</span></a>
            <button class="action-button" type="button" data-signout disabled>Sign out</button>
          </div>
          <a id="auth-recheck" class="action-button primary" href="/sign-in" hidden>Check account</a>
          <p id="auth-status" class="auth-feedback" role="status" aria-atomic="true">Enable JavaScript to sign in.</p>
        </section>
        <p class="auth-help">Access is limited to this server’s owner.</p>"#;

const SETUP_REQUIRED: &str = r#"
        <section class="auth-setup" aria-label="Owner setup required">
          <h2>Start in a terminal</h2>
          <p>Create the first owner from a local or SSH terminal, then restart DevOption.</p>
          <p>Run <code>devoption setup --help</code> for setup instructions.</p>
        </section>
        <p class="auth-help">Browser account creation is not available.</p>"#;

pub(super) fn render(account_available: bool) -> String {
    let (title, message, content) = if account_available {
        (
            "Sign in",
            "Use the owner password for this server.",
            SIGN_IN,
        )
    } else {
        (
            "Set up your workspace",
            "Owner setup is required before you can sign in.",
            SETUP_REQUIRED,
        )
    };
    let logo = crate::LOGO;
    crate::document(
        title,
        &format!(
            r##"<body class="auth-page" data-owner-configured="{account_available}">
      <a class="skip-link" href="#main-content">Skip to account access</a>
      <header class="auth-brand">
        <div class="auth-wordmark" role="img" aria-label="DevOption">{logo}</div>
        <div class="auth-introduction"><p class="auth-eyebrow">Software factory</p><p class="auth-statement">Your projects.<br>One place.</p><p>One place for your projects and connected repositories.</p></div>
      </header>
      <main id="main-content" class="auth-main" tabindex="-1" aria-labelledby="page-title">
        <div class="auth-content"><p class="auth-eyebrow">Owner access</p>
          <h1 id="page-title" tabindex="-1">{title}</h1>
          <p id="page-message">{message}</p>
          {content}
        </div>
      </main>
    </body>"##
        ),
    )
}
