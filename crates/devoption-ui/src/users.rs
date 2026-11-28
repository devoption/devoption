pub(super) const CONTENT: &str = r#"
<section class="profile-settings" aria-label="Workspace users" id="users-panel">
  <p>Members can work with every project. The owner and SSO administrators can manage accounts and authentication.</p>
  <form id="registration-form">
    <label for="registration-enabled">Registration</label>
    <select id="registration-enabled" disabled><option value="false">Closed — invitations only</option><option value="true">Open — owner approval required</option></select>
    <button class="action-button" type="submit" disabled>Save registration</button>
  </form>
  <h2>Invite a member</h2>
  <form id="invite-form">
    <label for="invite-email">Email address</label>
    <input id="invite-email" name="email" type="email" autocomplete="email" maxlength="254" required disabled>
    <p>Share the invitation code privately with this person. It expires after 24 hours and can be used once.</p>
    <button class="action-button primary" type="submit" disabled>Create invitation</button>
  </form>
  <div id="invitation-result" hidden>
    <label for="invitation-code">Invitation code — shown once</label>
    <textarea id="invitation-code" rows="3" readonly></textarea>
    <p>Ask the recipient to open <a href="/join">Create account</a> on this server, enter the invited email address, and paste this code. Copy it before leaving this page.</p>
  </div>
  <h2>Members and applications</h2>
  <p id="users-status" role="status">Checking account…</p>
  <div id="users-list"></div>
  <button id="users-reload" class="action-button" type="button">Refresh users</button>
</section>
<section class="profile-settings" id="saml-panel" aria-label="SAML single sign-on">
  <h2>SAML single sign-on</h2>
  <p>Assign application access at your identity provider. Every admitted SSO user has full administration access here.</p>
  <p id="saml-endpoints">Loading service provider details…</p>
  <form id="saml-form">
    <label for="saml-enabled">SAML sign-in</label>
    <select id="saml-enabled" disabled><option value="false">Disabled</option><option value="true">Enabled</option></select>
    <label for="saml-entity">Identity provider Entity ID</label>
    <input id="saml-entity" type="text" maxlength="2048" spellcheck="false" disabled>
    <label for="saml-metadata">Identity provider metadata XML</label>
    <textarea id="saml-metadata" rows="6" maxlength="65536" spellcheck="false" aria-describedby="saml-metadata-help" disabled></textarea>
    <p id="saml-metadata-help">Import metadata from your identity provider's administrator console. This pins its signing certificate; metadata is never fetched from a URL. Use signed responses and persistent, never-reused Name IDs.</p>
    <label for="saml-group-attribute">Group attribute (optional)</label>
    <input id="saml-group-attribute" type="text" maxlength="2048" spellcheck="false" disabled>
    <label for="saml-required-group">Required group value (optional)</label>
    <input id="saml-required-group" type="text" maxlength="2048" spellcheck="false" disabled>
    <p>Set both group fields to require an exact value in the signed assertion, or leave both blank to rely on provider application assignment.</p>
    <label for="saml-local">Local password sign-in and registration</label>
    <select id="saml-local" aria-describedby="saml-verification" disabled><option value="true">Enabled</option><option value="false">Disabled</option></select>
    <p id="saml-verification">Keep local authentication enabled until SSO has been verified.</p>
    <p>SSO sessions expire after 15 minutes. Provider access changes do not end existing sessions immediately. Saving authentication settings revokes all SSO sessions; local sessions keep their existing expiry.</p>
    <button class="action-button primary" type="submit" disabled>Save authentication</button>
  </form>
  <button id="saml-reload" class="action-button" type="button" disabled>Reload authentication</button>
  <button id="saml-revoke" class="action-button" type="button" disabled>Revoke all SSO sessions</button>
  <p id="saml-status" role="status">Enable JavaScript to configure SSO.</p>
</section>"#;
