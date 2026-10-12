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
    const projectForm = byId("project-form");
    const projectName = byId("project-name");
    const projectStatus = byId("project-status");
    // null: confirmed signed out; undefined: session check failed; string: verified.
    let csrf = null;
    let busy = false;
    let version = 0;
    const token = (response) => {
      const value = response.headers.get("x-csrf-token");
      return /^[a-f0-9]{64}$/i.test(value || "") ? value : null;
    };
    const message = (text) => { status.textContent = text; };
    const controls = () => {
      if (password) password.disabled = busy || csrf !== null;
      if (submit) submit.disabled = busy || csrf !== null;
      signouts.forEach((button) => { button.disabled = busy || !csrf; });
      if (projectForm) {
        projectName.disabled = busy || !csrf;
        byId("project-submit").disabled = busy || !csrf;
      }
    };
    const state = (value) => {
      csrf = value;
      const unchecked = csrf === undefined;
      if (form) {
        form.hidden = csrf !== null;
        byId("auth-recheck").hidden = !unchecked;
      }
      if (signedIn) signedIn.hidden = !csrf;
      signouts.forEach((button) => { button.hidden = !csrf; });
      byId("profile-state").textContent = unchecked ? "Check account" : csrf ? "Owner" : "Signed out";
      byId("profile-note").textContent = unchecked ? "Check your current session before signing in again." : csrf ? "Signed in on this server." : "Sign in to your local account.";
      byId("profile-account").textContent = unchecked ? "Check account" : csrf ? "Account" : "Sign in";
      if (form) {
        const title = unchecked ? "Check account" : csrf ? "Account" : "Sign in";
        byId("page-title").textContent = title;
        byId("page-message").textContent = unchecked ? "Check your current session before signing in again." : csrf ? "You’re signed in on this server." : "Use the owner password for this server.";
        document.title = title + " · DevOption";
      }
      controls();
    };
    const uncheckedSession = () => {
      state(undefined);
      message("Your session could not be checked. Use Check account to retry.");
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
          uncheckedSession();
        }
      } catch {
        if (current === version) {
          uncheckedSession();
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
      if (busy || csrf !== null) return;
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
        if (response.status === 204) {
          // Confirm that the browser accepted its Secure cookie before reporting
          // success. Never weaken cookie attributes for an unsupported browser.
          try {
            const session = await request("/auth/session");
            if (current !== version) return;
            if (session.status === 204 && token(session)) {
              state(token(session));
              message("Signed in.");
              byId("page-title").focus();
            } else if (session.status === 401) {
              state(null);
              message("Sign-in could not be retained. Check that this browser accepts secure localhost cookies.");
            } else uncheckedSession();
          } catch {
            if (current === version) uncheckedSession();
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
          if (csrf === undefined) byId("auth-recheck").focus();
          else if (csrf === null) password.focus();
        }
      }
    });
    if (projectForm) projectForm.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (busy || !csrf) return;
      const name = projectName.value;
      projectName.removeAttribute("aria-invalid");
      if (!name || name.trim() !== name || new TextEncoder().encode(name).length > 128 || /[\u0000-\u001f\u007f-\u009f]/.test(name)) {
        projectStatus.textContent = "Enter a name of up to 128 UTF-8 bytes, without leading or trailing spaces or control characters.";
        projectName.setAttribute("aria-invalid", "true");
        projectName.focus();
        return;
      }
      const current = ++version;
      busy = true;
      controls();
      projectStatus.textContent = "Creating project…";
      let saved = false;
      try {
        const response = await request("/projects", {
          headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body: name,
        });
        if (current !== version) return;
        if (response.status === 204) {
          saved = true;
          projectStatus.textContent = "Project saved. Loading the list…";
          window.location.assign("/projects");
          return;
        }
        if (response.status === 401) {
          state(null);
          projectStatus.textContent = "Your session ended. Sign in again to create a project.";
        } else if (response.status === 400 || response.status === 413) {
          projectStatus.textContent = "Check the project name: use up to 128 UTF-8 bytes without surrounding spaces or control characters.";
          projectName.setAttribute("aria-invalid", "true");
        } else if (response.status === 409) {
          projectStatus.textContent = "This catalog has reached its limit of 64 projects.";
        } else {
          projectStatus.textContent = "Creation could not be confirmed. Reload the project list before trying again.";
        }
      } catch {
        if (current === version) projectStatus.textContent = "Creation could not be confirmed. Reload the project list before trying again.";
      } finally {
        if (current === version) { busy = saved; controls(); }
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
