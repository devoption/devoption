(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const byId = (id) => document.getElementById(id);
    if (!byId("profile-state")) return;
    const form = byId("profile-form");
    const input = byId("display-name");
    const save = byId("profile-save");
    const reload = byId("profile-reload");
    const status = byId("profile-save-status");
    let verified = false;
    let loaded = false;
    let pending = false;
    let version = 0;
    const valid = (name) => typeof name === "string" && new TextEncoder().encode(name).length <= 128
      && name.trim() === name && !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(name);
    const message = (text) => { if (status) status.textContent = text; };
    const controls = () => {
      if (!form) return;
      input.disabled = !verified || !loaded || pending;
      save.disabled = input.disabled;
      save.textContent = pending ? "Saving…" : "Save name";
      form.setAttribute("aria-busy", String(pending));
      reload.disabled = !verified || pending;
      byId("profile-signin").hidden = verified;
      byId("profile-role").hidden = !verified;
    };
    function show(name) {
      byId("profile-state").textContent = name || "Account";
      byId("profile-title").textContent = name || "Account";
      const words = name.match(/[\p{L}\p{N}]+/gu) || [];
      const letters = words.length ? [Array.from(words[0])[0], ...(words.length > 1 ? [Array.from(words.at(-1))[0]] : [])] : [];
      byId("profile-initials").textContent = Array.from(letters.join("").toLocaleUpperCase()).slice(0, 2).join("");
      byId("profile-initials").hidden = !letters.length;
      byId("profile-avatar-icon").hidden = Boolean(letters.length);
    }
    const request = (url, options = {}) => fetch(url, {
      credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options,
    });
    async function load() {
      if (!verified || pending) return;
      const current = ++version;
      loaded = false;
      if (reload) reload.hidden = true;
      message("Loading your profile…"); controls();
      try {
        const response = await request("/account/profile");
        if (response.status !== 200) throw new Error("Profile unavailable");
        const name = await response.text();
        if (current !== version) return;
        if (!valid(name)) throw new Error("Invalid profile");
        show(name);
        if (input) input.value = name;
        loaded = true; message("");
      } catch {
        if (current !== version) return;
        message("Your saved name could not be loaded. Reload to retry, or check your account.");
        if (reload) reload.hidden = false;
        if (form) byId("profile-signin").hidden = false;
      } finally {
        if (current === version) {
          controls();
          if (!loaded && form) byId("profile-signin").hidden = false;
        }
      }
    }
    document.addEventListener("devoption-session", function (event) {
      ++version; pending = false; loaded = false;
      verified = event.detail === "signed-in";
      show("");
      if (verified) load();
      else {
        byId("profile-state").textContent = event.detail === "unknown" ? "Check account" : "Signed out";
        message("Check your account before editing your profile.");
        if (reload) reload.hidden = true;
        controls();
      }
    });
    if (reload) reload.addEventListener("click", load);
    if (form) form.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (!verified || !loaded || pending) return;
      const name = input.value;
      if (!valid(name)) {
        message("Use at most 128 UTF-8 bytes, without leading or trailing spaces, control characters or direction overrides.");
        input.focus(); return;
      }
      const current = ++version;
      pending = true; message("Saving your name…"); controls();
      try {
        const session = await request("/auth/session", { method: "POST" });
        if (current !== version) return;
        const csrf = session.headers.get("x-csrf-token");
        if (session.status !== 204 || !/^[a-f0-9]{64}$/i.test(csrf || "")) {
          verified = false;
          throw new Error("Session unavailable");
        }
        const response = await request("/account/profile", { method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body: name });
        if (current !== version) return;
        if (response.status === 401) verified = false;
        if (response.status !== 204) throw new Error("Save unavailable");
        show(name); message(name ? "Name saved." : "Name cleared. Your account uses “Account.”");
        reload.hidden = true;
      } catch {
        if (current !== version) return;
        message(verified ? "The save could not be confirmed. Reload the saved name before retrying." : "Your session could not be confirmed. Check your account to continue.");
        loaded = false; reload.hidden = !verified;
      } finally {
        if (current === version) { pending = false; controls(); }
      }
    });
    window.addEventListener("pagehide", function () { ++version; verified = false; pending = false; loaded = false; controls(); });
    if (document.body.dataset.ownerConfigured !== "true") {
      message("Owner setup is required before editing a profile."); controls();
    }
  });
}());
