(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const root = document.getElementById("build-execution");
    if (!root) return;
    const status = document.getElementById("build-action-status");
    const buttons = Array.from(root.querySelectorAll("[data-build-action]"));
    let busy = false;
    let generation = 0;
    const request = (path, options) => fetch(path, { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer", ...options });
    for (const button of buttons) button.addEventListener("click", async function () {
      if (busy) return;
      busy = true;
      const current = ++generation;
      buttons.forEach(b => { b.disabled = true; });
      const action = button.dataset.buildAction;
      status.textContent = action === "prepare" ? "Verifying committed source and prepared inputs…" : "Submitting build action…";
      try {
        const session = await request("/auth/session", { method: "POST" });
        const csrf = session.headers.get("x-csrf-token");
        if (session.status !== 204 || !csrf || !/^[0-9a-f]{64}$/.test(csrf)) throw new Error("session unavailable");
        if (generation !== current) return;
        const response = await request(root.dataset.base + "/" + action, {
          method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf },
          body: action === "approve" ? button.dataset.token : "",
        });
        if (generation !== current) return;
        if (![202, 204].includes(response.status)) throw new Error("action unavailable");
        if (action === "prepare") window.location.reload();
        else window.location.assign(root.dataset.base.replace(/\/[0-9a-f]{32}$/, ""));
      } catch {
        if (generation !== current) return;
        status.textContent = "The result could not be confirmed. Open build history before retrying; refresh this page for a new review if inputs changed.";
        // Never automatically retry approval after an ambiguous response.
      } finally {
        if (generation === current) { busy = false; buttons.forEach(b => { b.disabled = false; }); }
      }
    });
    window.addEventListener("pagehide", function () { generation++; busy = true; buttons.forEach(b => { b.disabled = true; }); });
    window.addEventListener("pageshow", function (event) { if (event.persisted) window.location.reload(); });
  });
})();
