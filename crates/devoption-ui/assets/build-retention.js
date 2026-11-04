(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const form = document.getElementById("retention-form");
    if (!form) return;
    const input = document.getElementById("retention-count");
    const status = document.getElementById("retention-status");
    const submit = form.querySelector('button[type="submit"]');
    let busy = false;
    let departed = false;
    form.addEventListener("submit", async function (event) {
      event.preventDefault();
      if (busy || departed || !form.reportValidity()) return;
      const count = input.value;
      if (!/^[0-9]{1,4}$/.test(count) || Number(count) < 1 || Number(count) > 1000 || !/^[a-f0-9]{32}$/.test(form.dataset.project)) return;
      busy = true; input.disabled = true; submit.disabled = true;
      status.textContent = "Saving policy and checking eligible builds…";
      try {
        const options = { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer" };
        const session = await fetch("/auth/session", { ...options, method: "POST" });
        const csrf = session.headers.get("x-csrf-token");
        if (departed) return;
        if (session.status !== 204 || !/^[a-f0-9]{64}$/.test(csrf || "")) throw new Error("session unavailable");
        const response = await fetch(`/projects/${form.dataset.project}/retention`, {
          ...options, method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body: count,
        });
        if (departed) return;
        if (response.status === 204) { window.location.reload(); return; }
        status.textContent = "The saved policy or cleanup result could not be confirmed. Reload Settings to inspect the result before retrying.";
      } catch {
        if (!departed) status.textContent = "The saved policy or cleanup result could not be confirmed. Reload Settings to inspect the result before retrying.";
      }
    });
    window.addEventListener("pagehide", () => { departed = true; input.disabled = true; submit.disabled = true; });
    window.addEventListener("pageshow", event => { if (event.persisted) window.location.reload(); });
  });
})();
