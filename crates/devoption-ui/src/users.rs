pub(super) const CONTENT: &str = r#"
<section class="profile-settings" aria-label="Workspace users" id="users-panel">
  <p>Members can work with every project. Only the owner can invite people, approve applications, or change registration.</p>
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
</section>"#;
