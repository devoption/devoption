(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const byId = (id) => document.getElementById(id);
    const form = byId("signin-form");
    const password = byId("auth-password");
    const emailInput = byId("auth-email");
    const submit = byId("auth-submit");
    const signedIn = byId("auth-signed-in");
    const status = byId("auth-status") || byId("profile-status");
    const signouts = Array.from(document.querySelectorAll("[data-signout]"));
    const projectForm = byId("project-form");
    const projectName = byId("project-name");
    const projectStatus = byId("project-status");
    const renameProject = projectForm?.dataset.project;
    const renaming = renameProject !== undefined;
    let projectPending = false;
    const repositoryForms = Array.from(document.querySelectorAll(".repository-form")).map((form) => ({
      form, path: form.querySelector("[data-repository-path]"),
      submit: form.querySelector("[data-repository-submit]"), status: form.querySelector("[data-repository-status]"),
      choice: form.querySelector("[data-repository-choice]"), refresh: form.querySelector("[data-repository-refresh]"),
      optionsStatus: form.querySelector("[data-repository-options-status]"),
      optionsLoaded: false, optionsLoading: false, optionsAvailable: false, optionsVersion: 0,
      beforeSessionFailure: null, pending: false,
    }));
    // null: confirmed signed out; undefined: session check failed; string: verified.
    let csrf = null;
    let busy = false;
    let version = 0;
    let hadSession = false;
    let operation = "";
    const token = (response) => {
      const value = response.headers.get("x-csrf-token");
      return /^[a-f0-9]{64}$/i.test(value || "") ? value : null;
    };
    const text = (id, value) => { const element = byId(id); if (element) element.textContent = value; };
    const message = (value) => { if (status) status.textContent = value; };
    const controls = () => {
      if (password) password.disabled = busy || csrf !== null;
      if (emailInput) emailInput.disabled = busy || csrf !== null;
      if (submit) {
        submit.disabled = busy || csrf !== null;
        submit.textContent = busy ? operation : "Sign in";
        form.setAttribute("aria-busy", String(busy));
      }
      signouts.forEach((button) => { button.disabled = busy || !csrf; });
      repositoryForms.forEach((repository) => {
        repository.path.disabled = busy || !csrf;
        repository.submit.disabled = busy || !csrf;
        if (repository.choice) {
          repository.choice.disabled = busy || !csrf || repository.optionsLoading || !repository.optionsLoaded || !repository.optionsAvailable;
          repository.refresh.disabled = busy || !csrf || repository.optionsLoading;
        }
      });
      if (projectForm) {
        projectName.disabled = busy || !csrf;
        byId("project-submit").disabled = busy || !csrf;
      }
    };
    const state = (value) => {
      csrf = value;
      if (csrf) hadSession = true;
      const unchecked = csrf === undefined;
      if (!unchecked) repositoryForms.forEach((repository) => {
        if (repository.beforeSessionFailure !== null) {
          repository.status.textContent = repository.beforeSessionFailure;
          repository.beforeSessionFailure = null;
        }
      });
      if (form) {
        form.hidden = csrf !== null;
        byId("auth-recheck").hidden = !unchecked;
      }
      if (signedIn) signedIn.hidden = !csrf;
      signouts.forEach((button) => { button.hidden = !csrf; });
      text("profile-state", unchecked ? "Check account" : csrf ? "Account" : "Signed out");
      text("profile-note", unchecked ? "Check your current session before signing in again." : csrf ? "Signed in" : "Sign in to your account.");
      text("profile-account", unchecked ? "Check account" : csrf ? "Profile" : "Sign in");
      if (form) {
        const title = unchecked ? "Check account" : csrf ? "Account" : "Sign in";
        byId("page-title").textContent = title;
        byId("page-message").textContent = unchecked ? "Check your current session before signing in again." : csrf ? "You’re signed in on this server." : "Enter your email and password, or leave email blank to sign in as owner.";
        document.title = title + " · DevOption";
      }
      const account = byId("profile-account");
      if (account) {
        account.setAttribute("href", csrf ? "/settings/profile" : "/sign-in");
        if (csrf && window.location.pathname === "/settings/profile") account.setAttribute("aria-current", "page");
        else account.removeAttribute("aria-current");
      }
      document.dispatchEvent(new CustomEvent("devoption-session", { detail: csrf ? "signed-in" : unchecked ? "unknown" : "signed-out" }));
      controls();
    };
    const uncheckedSession = () => {
      state(undefined);
      message("Your session could not be checked. Use Check account to retry.");
      repositoryForms.forEach((repository) => {
        if (repository.beforeSessionFailure === null) repository.beforeSessionFailure = repository.status.textContent;
        repository.status.textContent = "Your session could not be checked. Open Check account to retry.";
      });
    };
    const request = (path, options = {}) => fetch(path, {
      method: "POST", credentials: "same-origin", cache: "no-store",
      redirect: "error", referrerPolicy: "no-referrer", ...options,
    });
    async function loadCheckouts(repository) {
      if (!csrf || busy || repository.optionsLoading) return;
      const current = ++repository.optionsVersion;
      repository.optionsLoading = true;
      repository.optionsStatus.textContent = "Loading checkouts…";
      controls();
      try {
        const response = await request("/repository-options", { method: "GET" });
        if (response.status !== 200) throw new Error("Checkouts unavailable");
        const text = await response.text();
        if (current !== repository.optionsVersion || !csrf) return;
        const paths = text ? text.split("\n") : [];
        if (paths.length > 64 || paths.some((path) => !path || /[\/\u0000-\u001f\u007f-\u009f]/.test(path) || path === "." || path === ".." || new TextEncoder().encode(path).length > 128)) throw new Error("Invalid checkout list");
        const placeholder = document.createElement("option");
        placeholder.value = "";
        placeholder.textContent = paths.length ? "Choose a checkout" : "No checkouts found";
        repository.choice.replaceChildren(placeholder, ...paths.map((path) => {
          const option = document.createElement("option");
          option.value = path; option.textContent = path; return option;
        }));
        repository.optionsLoaded = true;
        repository.optionsAvailable = paths.length > 0;
        repository.optionsStatus.textContent = response.headers.get("x-devoption-list-limited") === "true"
          ? "Only the first 64 entries were checked. Enter a path if your checkout is not listed."
          : paths.length ? "Select a checkout to fill its path below."
            : "No supported checkouts found directly in the trusted root. Enter a nested path, or add a checkout and refresh.";
      } catch {
        if (current === repository.optionsVersion && csrf) {
          repository.optionsLoaded = false;
          repository.optionsStatus.textContent = "Checkouts could not be loaded. Refresh to retry, or enter a path below.";
        }
      } finally {
        if (current === repository.optionsVersion) { repository.optionsLoading = false; controls(); }
      }
    }
    function loadOpenCheckouts() {
      repositoryForms.forEach((repository) => {
        if (repository.choice && !repository.optionsLoaded && repository.form.closest("dialog").open) loadCheckouts(repository);
      });
    }
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
      operation = "Checking…";
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
          message(hadSession ? "Your session ended. Sign in again to continue." : "");
        } else {
          uncheckedSession();
        }
      } catch {
        if (current === version) {
          uncheckedSession();
        }
      } finally {
        if (current === version) { busy = false; controls(); loadOpenCheckouts(); }
      }
    }
    if (document.body.dataset.ownerConfigured !== "true") {
      state(null);
      text("profile-state", "Account");
      text("profile-note", "Owner setup is required.");
      return;
    }
    if (form) form.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (busy || csrf !== null) return;
      const email = emailInput?.value || "";
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
      operation = "Signing in…";
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
        const response = await request(email ? "/auth/user-sign-in" : "/auth/sign-in", {
          headers: { "content-type": email ? "text/plain;charset=UTF-8" : "application/x-www-form-urlencoded", "x-csrf-token": token(challenge) },
          body: email ? JSON.stringify({ email, password: body.get("password") }) : body,
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
              // A fixed destination cannot carry a caller-controlled return URL.
              window.location.replace("/");
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
      if (renaming && !/^[a-f0-9]{32}$/.test(renameProject)) {
        projectStatus.textContent = "Project unavailable. Reload the project list.";
        return;
      }
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
      projectStatus.textContent = renaming ? "Saving name…" : "Creating project…";
      projectPending = true;
      const uncertain = renaming ? "The name change could not be confirmed. Reload this page before trying again." : "Creation could not be confirmed. Reload the project list before trying again.";
      let saved = false;
      try {
        const response = await request(renaming ? `/projects/${renameProject}/name` : "/projects", {
          headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body: name,
        });
        if (current !== version) return;
        if (response.status === 204) {
          saved = true;
          if (renaming) {
            projectStatus.textContent = "Name saved. Reloading project…";
            window.location.reload();
            return;
          }
          const location = response.headers.get("location");
          const destination = /^\/projects\/[0-9a-f]{32}$/.test(location || "") ? location : "/projects";
          projectStatus.textContent = destination === "/projects" ? "Project saved. Opening Projects…" : "Project saved. Opening project…";
          window.location.assign(destination);
          return;
        }
        if (response.status === 401) {
          state(null);
          projectStatus.textContent = renaming ? "Your session ended. Sign in again to rename this project." : "Your session ended. Sign in again to create a project.";
        } else if (response.status === 400 || response.status === 413) {
          projectStatus.textContent = "Check the project name: use up to 128 UTF-8 bytes without surrounding spaces or control characters.";
          projectName.setAttribute("aria-invalid", "true");
        } else if (response.status === 409 && !renaming) {
          projectStatus.textContent = "This catalog has reached its limit of 64 projects.";
        } else if (response.status === 404 && renaming) {
          projectStatus.textContent = "Project unavailable. Return to the project list.";
        } else {
          projectStatus.textContent = uncertain;
        }
      } catch {
        if (current === version) projectStatus.textContent = uncertain;
      } finally {
        if (current === version) { projectPending = saved; busy = saved; controls(); }
      }
    });
    repositoryForms.forEach((repository) => {
      if (!repository.choice) return;
      repository.refresh.addEventListener("click", () => loadCheckouts(repository));
      repository.form.closest("dialog").addEventListener("dialogopened", loadOpenCheckouts);
      repository.choice.addEventListener("change", () => {
        if (!csrf || busy || repository.choice.disabled || !repository.choice.value) return;
        repository.path.value = repository.choice.value;
        repository.path.removeAttribute("aria-invalid");
      });
      repository.path.addEventListener("input", () => { repository.choice.value = ""; });
    });
    repositoryForms.forEach((repository) => repository.form.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (busy || !csrf) return;
      const project = repository.form.dataset.project;
      const path = repository.path.value;
      repository.path.removeAttribute("aria-invalid");
      const parts = path.split("/");
      if (!/^[a-f0-9]{32}$/i.test(project || "")) {
        repository.status.textContent = "Project unavailable. Reload the project list.";
        return;
      }
      if (!path || new TextEncoder().encode(path).length > 4096 || parts.length > 64 || parts.some((part) => !part || part === "." || part === "..") || /[\u0000-\u001f\u007f-\u009f]/.test(path)) {
        repository.status.textContent = "Enter a relative checkout path of up to 4096 UTF-8 bytes and 64 parts, without empty parts, . or .., or control characters.";
        repository.path.setAttribute("aria-invalid", "true");
        repository.path.focus();
        return;
      }
      const current = ++version;
      busy = true;
      controls();
      repository.status.textContent = "Attaching repository…";
      repository.pending = true;
      let saved = false;
      const uncertain = "Attachment could not be confirmed. Reload the project list before trying again.";
      try {
        const response = await request(`/projects/${project}/repositories`, {
          headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body: path,
        });
        if (current !== version) return;
        if (response.status === 204) {
          saved = true;
          repository.status.textContent = "Repository attached. Reloading…";
          window.location.reload();
          return;
        }
        const code = response.headers.get("x-devoption-error");
        if (response.status === 401) {
          state(null);
          repository.status.textContent = "Your session ended. Sign in again to attach a repository.";
        } else if (response.status === 400 || response.status === 413) {
          repository.status.textContent = "Check the relative path. It must name a supported local checkout inside the server’s trusted root.";
          repository.path.setAttribute("aria-invalid", "true");
        } else if (response.status === 409) {
          repository.status.textContent = code === "already-associated"
            ? "This repository is already attached to this project. Its default checkout was not changed."
            : code === "checkout-changed"
              ? "The checkout or trusted root changed. Nothing was attached. Reload the project list and ask the server administrator to check the location."
              : code === "catalog-full"
                ? "The project catalog has reached its repository or storage limit."
                : "The attachment conflicts with saved state. Reload the project list before trying again.";
        } else if (response.status === 404) {
          repository.status.textContent = "Project unavailable. Reload the project list.";
        } else if (response.status === 503 && code === "root-unconfigured") {
          repository.status.textContent = "Repository attachment is not configured on this server. Ask the server administrator to select a trusted root.";
        } else if (response.status === 503 && code === "unavailable") {
          repository.status.textContent = "Repository storage is unavailable. The attachment could not be confirmed. Reload the project list before trying again.";
        } else if (response.status === 403) {
          repository.status.textContent = "The request was denied. Reload the project list before trying again.";
        } else {
          repository.status.textContent = uncertain;
        }
      } catch {
        if (current === version) repository.status.textContent = uncertain;
      } finally {
        if (current === version) { repository.pending = saved; busy = saved; controls(); }
      }
    }));
    signouts.forEach((button) => button.addEventListener("click", async function () {
      if (busy || !csrf) return;
      const current = ++version;
      busy = true;
      operation = "Signing out…";
      controls();
      message("Signing out…");
      try {
        const response = await request("/auth/sign-out", { headers: { "x-csrf-token": csrf } });
        if (current !== version) return;
        if (response.status === 204) {
          state(null);
          hadSession = false;
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
      if (projectPending) {
        projectStatus.textContent = "Reload this page to check whether the project was saved before trying again.";
        projectPending = false;
      }
      repositoryForms.forEach((repository) => {
        ++repository.optionsVersion;
        repository.optionsLoading = false;
        repository.optionsLoaded = false;
        if (repository.optionsStatus) repository.optionsStatus.textContent = "Checkouts will refresh when your session is checked.";
        if (repository.pending) {
          repository.status.textContent = "Reload the project list to check whether the attachment completed before trying again.";
          repository.pending = false;
        }
      });
      controls();
    });
    window.addEventListener("pageshow", (event) => { if (event.persisted) recover(); });
    recover();
  });
})();
