(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const byId = (id) => document.getElementById(id);
    const request = (path, options = {}) => fetch(path, { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options });
    const token = (response) => {
      const value = response.headers.get("x-csrf-token");
      if (!/^[a-f0-9]{64}$/i.test(value || "")) throw new Error("Session unavailable.");
      return value;
    };
    const button = byId("saml-signin");
    let methods = null;
    let signedIn = false;
    let sessionKnown = false;
    let signingIn = false;
    function showMethods() {
      if (!button || !methods) return;
      button.hidden = !methods.saml || signedIn;
      button.disabled = signingIn || signedIn || !sessionKnown;
      byId("local-auth-controls").hidden = !methods.local_auth;
      const help = document.querySelector(".auth-help");
      if (help) help.hidden = !methods.local_auth;
      if (!methods.local_auth && !signedIn) {
        byId("page-message").textContent = "Use your organization's single sign-on to access this workspace.";
        byId("auth-status").textContent = "Local password authentication is disabled.";
      }
    }
    if (button) {
      request("/auth/methods").then(async (response) => {
        if (response.status !== 200) throw new Error();
        const value = await response.json();
        if (typeof value.saml !== "boolean" || typeof value.local_auth !== "boolean") throw new Error();
        methods = value; showMethods();
      }).catch(() => { byId("saml-signin-status").hidden = false; byId("saml-signin-status").textContent = "Sign-in methods could not be loaded. Reload to retry."; });
      button.addEventListener("click", async () => {
        if (signingIn || signedIn) return;
        signingIn = true; showMethods();
        const status = byId("saml-signin-status"); status.hidden = false; status.textContent = "Opening single sign-on…";
        try {
          const challenge = await request("/auth/challenge", { method: "POST" });
          if (challenge.status !== 204) throw new Error();
          const response = await request("/auth/saml/start", { method: "POST", headers: { "x-csrf-token": token(challenge) } });
          if (response.status !== 200) throw new Error();
          const result = await response.json();
          const url = new URL(result.redirect);
          if (url.protocol !== "https:") throw new Error();
          window.location.assign(url.href);
        } catch { status.textContent = "SSO could not be started. Try again later or contact your administrator."; signingIn = false; showMethods(); }
      });
    }
    const panel = byId("saml-panel");
    let ready = false;
    let busy = false;
    let revision = null;
    let generation = 0;
    const fields = ["enabled", "local_auth", "idp_entity_id", "idp_metadata", "group_attribute", "required_group"];
    const ids = ["saml-enabled", "saml-local", "saml-entity", "saml-metadata", "saml-group-attribute", "saml-required-group"];
    const status = byId("saml-status");
    function controls() {
      if (panel) {
        panel.querySelectorAll("button, input, select, textarea").forEach((el) => { el.disabled = busy || !ready; });
        byId("saml-reload").disabled = busy || !signedIn;
      }
    }
    async function load() {
      if (!panel || busy || !signedIn) return;
      const current = ++generation;
      busy = true; ready = false; controls(); status.textContent = "Loading authentication settings…";
      try {
        const response = await request("/auth/saml/settings");
        if (response.status !== 200) throw new Error();
        const data = await response.json();
        if (current !== generation) return;
        const c = data.configuration;
        revision = c.revision;
        fields.forEach((field, i) => { byId(ids[i]).value = String(c[field]); });
        byId("saml-local").querySelector('option[value="false"]').disabled = !c.verified || !c.enabled;
        byId("saml-verification").textContent = c.verified ? "Successful SSO sign-in verified for this saved configuration. Local authentication can now be disabled." : "Keep local authentication enabled. Save, then complete a successful SSO sign-in before disabling it.";
        byId("saml-endpoints").textContent = data.origin ? `Entity ID: ${data.origin}/auth/saml/metadata · ACS URL: ${data.origin}/auth/saml/acs` : "SAML requires the server's explicit HTTPS public origin. Configure it before enabling SSO.";
        ready = true; status.textContent = "Authentication settings loaded.";
      } catch { if (current === generation) status.textContent = "Authentication settings could not be loaded. Check your account and reload this page."; }
      finally { if (current === generation) { busy = false; controls(); } }
    }
    async function save(command) {
      if (!ready || busy) return;
      const current = ++generation;
      busy = true; controls(); status.textContent = "Saving authentication…";
      try {
        const session = await request("/auth/session", { method: "POST" });
        if (session.status !== 204) throw new Error();
        const response = await request("/auth/saml/settings", { method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": token(session) }, body: JSON.stringify(command) });
        if (current !== generation) return;
        if (response.status !== 204) throw new Error();
        ready = false;
        status.textContent = "Saved. SSO sessions have been revoked. Sign in again if needed, then reload this page to check saved settings.";
      } catch { if (current === generation) { ready = false; status.textContent = "Save could not be confirmed. Reload this page to check saved state, configuration validity and SSO verification before retrying."; } }
      finally { if (current === generation) { busy = false; controls(); } }
    }
    if (panel) {
      ["saml-enabled", "saml-entity", "saml-metadata", "saml-group-attribute", "saml-required-group"].forEach(id => byId(id).addEventListener("input", () => {
        byId("saml-local").value = "true";
        byId("saml-local").querySelector('option[value="false"]').disabled = true;
        byId("saml-verification").textContent = "Save this changed configuration with local authentication enabled, then verify SSO again.";
      }));
      byId("saml-reload").addEventListener("click", load);
      byId("saml-revoke").addEventListener("click", () => save({ action: "revoke" }));
      byId("saml-form").addEventListener("submit", (event) => {
        event.preventDefault();
        const command = { action: "save", revision };
        fields.forEach((field, i) => { const value = byId(ids[i]).value; command[field] = ["enabled", "local_auth"].includes(field) ? value === "true" : value; });
        save(command);
      });
    }
    document.addEventListener("devoption-session", (event) => {
      signedIn = event.detail === "signed-in"; sessionKnown = event.detail !== "unknown"; showMethods();
      if (panel) { ++generation; busy = false; ready = false; controls(); if (signedIn) load(); else status.textContent = "Sign in as an administrator to manage authentication."; }
    });
  });
})();
