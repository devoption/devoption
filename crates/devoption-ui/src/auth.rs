const SIGN_IN: &str = r#"
        <section class="auth" aria-label="Account access">
          <form id="signin-form" action="/auth/sign-in" method="post">
            <label for="auth-email">Email address <span>(leave blank for owner)</span></label>
            <input id="auth-email" name="email" type="email" autocomplete="username" maxlength="254" disabled>
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
        <p class="auth-help"><a href="/join">Create account or accept an invitation</a>.</p>"#;

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
            "Enter your email and password, or leave email blank to sign in as owner.",
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
        <div class="auth-content"><p class="auth-eyebrow">Workspace access</p>
          <h1 id="page-title" tabindex="-1">{title}</h1>
          <p id="page-message">{message}</p>
          {content}
        </div>
      </main>
    </body>"##
        ),
    )
}

pub(super) fn render_join() -> String {
    let page = render(true);
    let start = page
        .find("          <form id=\"signin-form\"")
        .expect("sign-in form");
    let end = page[start..]
        .find("        </section>")
        .expect("section end")
        + start;
    let content = r#"<form id="join-form">
      <p id="registration-note">Checking registration…</p>
      <label for="join-email">Email address</label><input id="join-email" type="email" autocomplete="username" maxlength="254" required>
      <label for="join-password">Password</label><input id="join-password" type="password" autocomplete="new-password" maxlength="1024" required aria-describedby="join-password-help">
      <p id="join-password-help">At least 15 characters. Use a unique passphrase.</p>
      <label for="join-confirm">Confirm password</label><input id="join-confirm" type="password" autocomplete="new-password" maxlength="1024" required>
      <label for="join-invitation">Invitation code (if invited)</label><input id="join-invitation" type="text" autocomplete="off" maxlength="64" spellcheck="false" required>
      <button class="action-button primary" type="submit" disabled>Create account</button>
      <p id="join-status" role="status">JavaScript is required. Registration applications require owner approval.</p>
    </form>"#;
    let mut page = format!("{}{}{}", &page[..start], content, &page[end..]);
    page = page
        .replace(
            "<h1 id=\"page-title\" tabindex=\"-1\">Sign in</h1>",
            "<h1 id=\"page-title\" tabindex=\"-1\">Create account</h1>",
        )
        .replace(
            "<title>Sign in · DevOption</title>",
            "<title>Create account · DevOption</title>",
        )
        .replace(
            "Enter your email and password, or leave email blank to sign in as owner.",
            "Join this shared workspace through an invitation or owner approval.",
        )
        .replace(
            "<a href=\"/join\">Create account or accept an invitation</a>.",
            "<a href=\"/sign-in\">Back to sign in</a>.",
        );
    page
}
