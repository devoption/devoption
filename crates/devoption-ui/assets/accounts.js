(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const byId = (id) => document.getElementById(id);
    const request = (path, options = {}) => fetch(path, { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options });
    const validToken = (value) => /^[a-f0-9]{64}$/i.test(value || "");
    async function csrf() {
      const response = await request("/auth/session", { method: "POST" });
      const token = response.headers.get("x-csrf-token");
      if (response.status !== 204 || !validToken(token)) throw new Error("Session ended. Sign in again.");
      return token;
    }
    let roleVersion = 0;
    document.addEventListener("devoption-session", async (event) => {
      const current = ++roleVersion;
      const manage = byId("manage-users");
      if (manage) manage.hidden = true;
      if (event.detail !== "signed-in") return;
      try {
        const response = await request("/auth/account");
        if (response.status !== 200) return;
        const { role } = await response.json();
        if (current !== roleVersion || !["Owner", "Member"].includes(role)) return;
        const note = byId("profile-note");
        if (note) note.textContent = `Role: ${role}`;
        const label = byId("profile-role");
        if (label) label.querySelector("strong").textContent = role;
        if (manage) manage.hidden = role !== "Owner";
      } catch { /* Role remains unspecified until checked. */ }
    });
    const panel = byId("users-panel");
    if (panel) {
      let busy = false;
      let ready = false;
      let version = 0;
      const status = byId("users-status");
      const controls = () => panel.querySelectorAll("button, input, select").forEach((control) => { control.disabled = busy || !ready; });
      async function load(current = version) {
        const response = await request("/auth/users");
        if (response.status !== 200) throw new Error("Users could not be loaded. Check your account and refresh.");
        const data = await response.json();
        if (current !== version) return;
        byId("registration-enabled").value = String(data.registration);
        byId("users-list").replaceChildren(...data.users.map((user) => {
          const row = document.createElement("div"); row.className = "user-row";
          const name = document.createElement("strong"); name.textContent = user.email;
          const state = document.createElement("span"); state.textContent = user.status;
          row.append(name, state);
          const actions = user.status === "pending" ? [["approve", "Approve"], ["remove", "Reject"]]
            : user.status === "active" ? [["disable", "Disable"]] : [["remove", "Remove"]];
          for (const [action, text] of actions) {
            const button = document.createElement("button"); button.type = "button"; button.className = "action-button"; button.textContent = text;
            button.setAttribute("aria-label", `${text} ${user.email}`);
            button.addEventListener("click", () => save({ action, email: user.email })); row.append(button);
          }
          return row;
        }));
        ready = true;
      }
      async function refresh() {
        if (busy) return;
        const current = ++version;
        busy = true; controls();
        try { await load(current); if (current !== version) return; status.textContent = byId("users-list").children.length ? "Users loaded." : "No members yet. Invite someone to get started."; }
        catch (error) { if (current !== version) return; ready = false; status.textContent = error.message; }
        finally { if (current === version) { busy = false; controls(); byId("users-reload").disabled = false; } }
      }
      async function save(command) {
        if (busy || !ready) return;
        const current = ++version;
        busy = true; controls(); status.textContent = "Saving…";
        byId("invitation-result").hidden = true; byId("invitation-code").value = "";
        try {
          const response = await request("/auth/users", { method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": await csrf() }, body: JSON.stringify(command) });
          if (response.status !== 200) throw new Error("Change could not be confirmed. Refresh users before retrying. Existing email addresses cannot be invited again; remove an inactive entry first.");
          const data = await response.json();
          if (current !== version) return;
          if (data.invitation && validToken(data.invitation)) { byId("invitation-code").value = data.invitation; byId("invitation-result").hidden = false; byId("invite-email").value = ""; }
          await load(current); if (current === version) status.textContent = "Saved.";
        } catch (error) { if (current !== version) return; ready = false; status.textContent = error.message; }
        finally { if (current === version) { busy = false; controls(); byId("users-reload").disabled = false; } }
      }
      byId("registration-form").addEventListener("submit", (event) => { event.preventDefault(); save({ action: "registration", enabled: byId("registration-enabled").value === "true" }); });
      byId("invite-form").addEventListener("submit", (event) => { event.preventDefault(); save({ action: "invite", email: byId("invite-email").value }); });
      byId("users-reload").addEventListener("click", refresh);
      window.addEventListener("pagehide", () => { ++version; busy = false; byId("invitation-code").value = ""; byId("invitation-result").hidden = true; ready = false; controls(); });
      window.addEventListener("pageshow", (event) => { if (event.persisted) refresh(); });
      refresh();
    }
    const join = byId("join-form");
    if (join) {
      let busy = false;
      const status = byId("join-status");
      const controls = () => join.querySelectorAll("input, button").forEach((control) => { control.disabled = busy; });
      request("/auth/registration").then(async (response) => {
        if (response.status !== 200) throw new Error();
        const data = await response.json();
        byId("registration-note").textContent = data.registration ? "Registration is open. Applications need owner approval before sign-in." : "Registration is closed. An invitation code is required.";
        byId("join-invitation").required = !data.registration;
      }).catch(() => { byId("registration-note").textContent = "Registration availability could not be checked. You can still submit an invitation."; });
      controls();
      status.textContent = "Invited members can sign in after acceptance. Other applications need owner approval.";
      join.addEventListener("submit", async (event) => {
        event.preventDefault(); if (busy) return;
        let password = byId("join-password").value;
        const confirmation = byId("join-confirm").value;
        if (password !== confirmation || Array.from(password).length < 15 || new TextEncoder().encode(password).length > 1024 || /\p{Cc}/u.test(password)) { status.textContent = "Use a matching password of at least 15 characters, up to 1024 UTF-8 bytes, without control characters."; return; }
        const body = { email: byId("join-email").value, password, invitation: byId("join-invitation").value };
        byId("join-password").value = ""; byId("join-confirm").value = "";
        busy = true; controls(); status.textContent = "Submitting…";
        try {
          const challenge = await request("/auth/challenge", { method: "POST" });
          const token = challenge.headers.get("x-csrf-token");
          if (challenge.status !== 204 || !validToken(token)) throw new Error();
          const response = await request("/auth/register", { method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": token }, body: JSON.stringify(body) });
          if (response.status !== 202) throw new Error();
          status.textContent = body.invitation ? "Submission received. You can now try signing in with your email and password. If access is unavailable, ask the owner to check the invitation." : "Application received. If registration is open and this address is eligible, the owner can approve it. Sign in after approval.";
          byId("join-invitation").value = "";
        } catch { status.textContent = "Submission could not be confirmed. Check with the owner before retrying, or wait a minute if you have made several attempts."; }
        finally { password = ""; body.password = ""; busy = false; controls(); }
      });
    }
  });
})();
