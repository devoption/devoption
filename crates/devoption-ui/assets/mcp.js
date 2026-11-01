/* Consent and revocation use the existing browser session and explicit CSRF. */
document.addEventListener("DOMContentLoaded", async () => {
  const status = document.getElementById("mcp-status");
  const forms = [...document.querySelectorAll("#mcp-consent, .mcp-revoke")];
  if (!status || !forms.length) return;
  let csrf;
  try {
    const response = await fetch("/auth/session", {method: "POST", credentials: "same-origin", cache: "no-store"});
    if (!response.ok || !(csrf = response.headers.get("x-csrf-token"))) throw new Error();
    status.textContent = "Review access before continuing.";
    for (const form of forms) {
      for (const button of form.querySelectorAll("button")) button.disabled = false;
      form.addEventListener("submit", async event => {
        event.preventDefault();
        for (const button of form.querySelectorAll("button")) button.disabled = true;
        try {
          const consent = form.id === "mcp-consent";
          const body = new URLSearchParams(new FormData(form));
          if (consent) body.set("decision", event.submitter.value);
          const response = await fetch(consent ? `/oauth/authorize${location.search}` : "/mcp/grants", {
            method: "POST", credentials: "same-origin", cache: "no-store",
            headers: {"Content-Type": "application/x-www-form-urlencoded", "x-csrf-token": csrf}, body
          });
          if (!response.ok) throw new Error();
          if (consent) location.assign((await response.json()).redirect);
          else {form.remove(); status.textContent = "Client access revoked.";}
        } catch {
          status.textContent = "Access could not be updated. Reload and try again.";
        }
      });
    }
  } catch {
    status.textContent = "Sign in, then return here and reload to manage access.";
  }
});
