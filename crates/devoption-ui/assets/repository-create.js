(() => {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const byId = id => document.getElementById(id);
    const form = byId("repository-create-form");
    if (!form) return;
    const name = byId("repository-create-name");
    const submit = byId("repository-create-submit");
    const status = byId("repository-create-status");
    const reload = byId("repository-create-reload");
    const signin = byId("repository-create-signin");
    const project = form.dataset.project;
    if (!/^[a-f0-9]{32}$/i.test(project || "")) return;
    let verified = false, pending = false, blocked = false, inspectNeeded = false, version = 0;
    const message = text => { status.textContent = text; };
    const controls = () => {
      name.disabled = submit.disabled = !verified || pending || blocked;
      submit.textContent = pending ? "Creating…" : "Create repository";
      form.setAttribute("aria-busy", String(pending));
      signin.hidden = verified;
      reload.hidden = !(blocked || inspectNeeded);
    };
    const request = (url, options = {}) => fetch(url, {method: "POST", credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options});
    document.addEventListener("devoption-session", event => {
      ++version;
      if (pending) blocked = true;
      pending = false;
      verified = event.detail === "signed-in";
      message(blocked ? "Reload Code to inspect saved state before another creation." : verified ? "" : "Check your account before creating a repository.");
      controls();
    });
    form.addEventListener("submit", async event => {
      event.preventDefault();
      if (!verified || pending || blocked) return;
      const value = name.value;
      if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value)) {
        message("Use 1–64 letters, numbers, hyphens or underscores, starting with a letter or number.");
        name.focus(); return;
      }
      const current = ++version;
      pending = true; inspectNeeded = false; message("Creating your repository…"); controls();
      try {
        const session = await request("/auth/session");
        if (current !== version) return;
        const csrf = session.headers.get("x-csrf-token");
        if (session.status !== 204 || !/^[a-f0-9]{64}$/i.test(csrf || "")) {
          verified = false; message("Your session could not be confirmed. Check your account to continue."); return;
        }
        const response = await request(`/projects/${project}/repositories/create`, {headers: {"content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf}, body: value});
        if (current !== version) return;
        const destination = response.headers.get("location") || "";
        if (response.status === 204 && new RegExp(`^/projects/${project}/repositories/[a-f0-9]{32}/files$`, "i").test(destination)) {
          blocked = true; message("Repository created. Opening Code…"); window.history.replaceState(null, "", destination); window.location.reload();
        } else if (response.status === 400 || response.status === 413) {
          message("The repository name was rejected. Check the name and try again.");
        } else if (response.status === 409) {
          message("That path already exists or the catalog is full. Existing files were not changed. Choose another name or reload Code to inspect saved state.");
          inspectNeeded = true;
        } else if (response.status === 401) {
          verified = false; message("Your session ended. Check your account to continue.");
        } else {
          blocked = true; message("Creation could not be confirmed. Reload Code and inspect repository storage before retrying.");
        }
      } catch {
        if (current === version) { blocked = true; message("Creation could not be confirmed. Reload Code and inspect repository storage before retrying."); }
      } finally {
        if (current === version) { pending = false; controls(); }
      }
    });
    window.addEventListener("pagehide", () => {
      ++version;
      if (pending) blocked = true;
      pending = false; verified = false; controls();
    });
  });
})();
