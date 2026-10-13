pub(super) const CONTENT: &str = r#"
<section class="profile-settings" aria-label="Your profile">
  <p id="profile-role" class="profile-role" hidden>Role <strong>Owner</strong></p>
  <form id="profile-form" aria-describedby="display-name-help profile-save-status">
    <label for="display-name">Display name</label>
    <input id="display-name" name="display-name" type="text" autocomplete="name" maxlength="128" aria-describedby="display-name-help profile-save-status" disabled>
    <p id="display-name-help">Up to 128 UTF-8 bytes. Leave blank to use “Account.” This name does not change how you sign in.</p>
    <button id="profile-save" class="action-button primary" type="submit" disabled>Save name</button>
    <p id="profile-save-status" role="status">Enable JavaScript to edit your profile.</p>
    <button id="profile-reload" class="action-button" type="button" hidden disabled>Reload saved name</button>
    <a id="profile-signin" href="/sign-in" hidden>Check account</a>
  </form>
</section>"#;
