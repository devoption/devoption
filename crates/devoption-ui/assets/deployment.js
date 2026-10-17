(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const root = document.getElementById("app-deployment");
    if (!root) return;
    const status = document.getElementById("deployment-action-status");
    const buttons = Array.from(root.querySelectorAll("[data-deploy-action]"));
    let busy = false;
    let generation = 0;
    const request = (path, options) => fetch(path, { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options });
    for (const button of buttons) button.addEventListener("click", async function () {
      if (busy) return;
      const action = button.dataset.deployAction;
      let body = button.dataset.value || "";
      if (action === "prepare") {
        const health = root.querySelector('[data-health="' + body + '"]').value;
        if (!/^[\x20-\x7e]{1,128}$/.test(health)) { status.textContent = "Enter the exact expected health response, for example ok A."; return; }
        body += "\n" + health;
      }
      busy = true;
      const current = ++generation;
      buttons.forEach(b => { b.disabled = true; });
      status.textContent = action === "approve" ? "Starting app and checking health…" : "Submitting release action…";
      try {
        const session = await request("/auth/session", { method: "POST" });
        const csrf = session.headers.get("x-csrf-token");
        if (session.status !== 204 || !csrf || !/^[0-9a-f]{64}$/.test(csrf)) throw new Error("session unavailable");
        if (generation !== current) return;
        const response = await request(root.dataset.base + "/" + action, { method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body });
        if (generation !== current) return;
        if (response.status !== 204) throw new Error("action unavailable");
        window.location.reload();
      } catch {
        if (generation === current) status.textContent = "The result could not be confirmed. Refresh health and logs before retrying. A new deployment review may be required.";
      } finally {
        if (generation === current) { busy = false; buttons.forEach(b => { b.disabled = false; }); }
      }
    });
    window.addEventListener("pagehide", function () { generation++; busy = true; buttons.forEach(b => { b.disabled = true; }); });
    window.addEventListener("pageshow", function (event) { if (event.persisted) window.location.reload(); });
  });
})();
