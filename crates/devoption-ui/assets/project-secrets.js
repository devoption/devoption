(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", function () {
    const root = document.getElementById("environment-secrets");
    if (!root) return;
    const status = document.getElementById("secrets-status");
    const forms = Array.from(root.querySelectorAll("[data-secret-form]"));
    let busy = false;
    let departed = false;
    const clear = () => root.querySelectorAll('input[type="password"]').forEach(input => { input.value = ""; });
    const disable = () => forms.forEach(form => { form.querySelector("fieldset").disabled = true; });
    root.querySelectorAll("[data-secret-time]").forEach(time => {
      const date = new Date(Number(time.dataset.secretTime) * 1000);
      if (!Number.isNaN(date.getTime())) { time.textContent = date.toLocaleString(); time.dateTime = date.toISOString(); }
    });
    forms.forEach(form => {
      form.querySelector("fieldset").disabled = false;
      form.addEventListener("submit", async function (event) {
        event.preventDefault();
        if (busy || departed || !form.reportValidity()) return;
        const name = form.dataset.name || form.elements.namedItem("name").value;
        const revision = Number(form.dataset.revision);
        if (!/^[A-Z_][A-Z0-9_]{0,63}$/.test(name) || !Number.isSafeInteger(revision)) return;
        const input = form.elements.namedItem("value");
        let value = input ? input.value : "";
        if (input && (new TextEncoder().encode(value).length > 8192 || !value.length)) {
          status.textContent = "Enter a value of at most 8,192 UTF-8 bytes."; return;
        }
        let body = form.dataset.secretForm === "remove" ? String(revision) : JSON.stringify({ value, revision });
        value = "";
        clear(); busy = true; disable();
        status.textContent = "Saving secret change…";
        try {
          const options = { credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer" };
          const session = await fetch("/auth/session", { ...options, method: "POST" });
          const csrf = session.headers.get("x-csrf-token");
          if (departed) return;
          if (session.status !== 204 || !csrf || !/^[0-9a-f]{64}$/.test(csrf)) throw new Error("session unavailable");
          const response = await fetch(root.dataset.base + "/secrets/" + encodeURIComponent(name) + (form.dataset.secretForm === "remove" ? "/remove" : ""), {
            ...options, method: "POST", headers: { "content-type": "text/plain;charset=UTF-8", "x-csrf-token": csrf }, body,
          });
          body = "";
          if (departed) return;
          if (response.status === 204) { window.location.reload(); return; }
          status.textContent = response.status === 409
            ? "This secret changed, or the name already exists. Reload before trying again."
            : response.status === 400 || response.status === 413
              ? "The change was rejected. Reload, then check the name and value size."
              : "The result could not be confirmed. Reload to check saved state before trying again.";
        } catch {
          if (!departed) status.textContent = "The result could not be confirmed. Reload to check saved state before trying again.";
        } finally {
          body = ""; clear();
          // A lost response may have committed. Never retry against stale revisions.
          if (!departed) {
            const reload = document.createElement("button"); reload.type = "button"; reload.className = "text-button secrets-reload"; reload.textContent = "Reload secrets";
            reload.addEventListener("click", () => window.location.reload());
            status.appendChild(reload);
          }
        }
      });
    });
    window.addEventListener("pagehide", () => { departed = true; clear(); disable(); });
    window.addEventListener("pageshow", event => { if (event.persisted) { clear(); window.location.reload(); } });
  });
})();
