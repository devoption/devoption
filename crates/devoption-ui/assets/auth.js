(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const byId = (id) => document.getElementById(id);
    const form = byId("signin-form");
    const password = byId("auth-password");
    const submit = byId("auth-submit");
    const signedIn = byId("auth-signed-in");
    const status = byId("auth-status") || byId("profile-status");
    const signouts = Array.from(document.querySelectorAll("[data-signout]"));
    let csrf = null;
    let busy = false;
    let version = 0;
    const token = (response) => {
      const value = response.headers.get("x-csrf-token");
      return /^[a-f0-9]{64}$/i.test(value || "") ? value : null;
    };
    const message = (text) => { status.textContent = text; };
    const controls = () => {
      if (password) password.disabled = busy || Boolean(csrf);
      if (submit) submit.disabled = busy || Boolean(csrf);
      signouts.forEach((button) => { button.disabled = busy || !csrf; });
    };
    const state = (value) => {
      csrf = value;
      if (form) form.hidden = Boolean(csrf);
      if (signedIn) signedIn.hidden = !csrf;
      signouts.forEach((button) => { button.hidden = !csrf; });
      byId("profile-state").textContent = csrf ? "Owner" : "Signed out";
      byId("profile-note").textContent = csrf ? "Signed in on this server." : "Sign in to your local account.";
      byId("profile-account").textContent = csrf ? "Account" : "Sign in";
      if (form) {
        byId("page-title").textContent = csrf ? "Account" : "Sign in";
        byId("page-message").textContent = csrf ? "You’re signed in on this server." : "Use the owner password for this server.";
        document.title = (csrf ? "Account" : "Sign in") + " · DevOption";
      }
      controls();
    };
    const request = (path, options = {}) => fetch(path, {
      method: "POST", credentials: "same-origin", cache: "no-store",
      redirect: "error", referrerPolicy: "no-referrer", ...options,
    });
    const failure = (response) => {
      if (response.status === 429 || response.status === 503) {
        const seconds = Number(response.headers.get("retry-after"));
        return Number.isInteger(seconds) && seconds > 0 && seconds <= 300
          ? `Please wait ${seconds} seconds before trying again.`
          : "DevOption is temporarily unavailable. Try again shortly.";
      }
      if (response.status === 403) return "This sign-in attempt expired or changed. Please try again.";
      return "The request could not be completed. Please try again.";
    };
    async function recover() {
      if (busy) return;
      const current = ++version;
      busy = true;
      controls();
      message("Checking your session…");
      try {
        const response = await request("/auth/session");
        if (current !== version) return;
        if (response.status === 204 && token(response)) {
          state(token(response));
          message("");
        } else if (response.status === 401) {
          state(null);
          message("");
        } else {
          state(null);
          message("Could not check your session. Reload or try signing in again.");
        }
      } catch {
        if (current === version) {
          state(null);
          message("Could not reach DevOption. Reload or try again.");
        }
      } finally {
        if (current === version) { busy = false; controls(); }
      }
    }
    if (document.body.dataset.ownerConfigured !== "true") {
      state(null);
      byId("profile-state").textContent = "Local preview";
      byId("profile-note").textContent = "Owner setup is required.";
      return;
    }
    if (form) form.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (busy) return;
      let value = password.value;
      password.value = "";
      password.removeAttribute("aria-invalid");
      if (!value || new TextEncoder().encode(value).length > 1024) {
        message("Enter a password of up to 1024 UTF-8 bytes.");
        password.setAttribute("aria-invalid", "true");
        password.focus();
        return;
      }
      const current = ++version;
      busy = true;
      controls();
      message("Signing in…");
      try {
        const challenge = await request("/auth/challenge");
        if (current !== version) return;
        if (challenge.status !== 204 || !token(challenge)) {
          message(failure(challenge));
          return;
        }
        const body = new URLSearchParams({ password: value });
        value = "";
        const response = await request("/auth/sign-in", {
          headers: { "content-type": "application/x-www-form-urlencoded", "x-csrf-token": token(challenge) }, body,
        });
        if (current !== version) return;
        if (response.status === 204 && token(response)) {
          // Confirm that the browser accepted its Secure cookie before reporting
          // success. Never weaken cookie attributes for an unsupported browser.
          const session = await request("/auth/session");
          if (current !== version) return;
          if (session.status === 204 && token(session)) {
            state(token(session));
            message("Signed in.");
            byId("page-title").focus();
          } else {
            state(null);
            message("Sign-in could not be retained. Check that this browser accepts secure localhost cookies.");
          }
        } else {
          message(response.status === 401 ? "Password not recognized. Try again." : failure(response));
          password.setAttribute("aria-invalid", "true");
        }
      } catch {
        if (current === version) message("Could not reach DevOption. Please try again.");
      } finally {
        value = "";
        if (current === version) {
          busy = false;
          controls();
          if (!csrf) password.focus();
        }
      }
    });
    signouts.forEach((button) => button.addEventListener("click", async function () {
      if (busy || !csrf) return;
      const current = ++version;
      busy = true;
      controls();
      message("Signing out…");
      try {
        const response = await request("/auth/sign-out", { headers: { "x-csrf-token": csrf } });
        if (current !== version) return;
        if (response.status === 204) {
          state(null);
          message("Signed out.");
          if (!password) window.location.assign("/sign-in");
        } else if (response.status === 401) {
          // Do not retry a stale logout against a newer session automatically.
          state(null);
          message("Your session changed or ended. Reload to check your account.");
        } else message(failure(response));
      } catch {
        if (current === version) message("Could not sign out. Check your connection and try again.");
      } finally {
        if (current === version) {
          busy = false;
          controls();
          if (password && !csrf) password.focus();
        }
      }
    }));
    window.addEventListener("pagehide", () => {
      ++version;
      csrf = null;
      busy = false;
      if (password) password.value = "";
      controls();
    });
    window.addEventListener("pageshow", (event) => { if (event.persisted) recover(); });
    recover();
  });
})();
