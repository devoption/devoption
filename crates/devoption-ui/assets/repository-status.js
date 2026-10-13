(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const views = Array.from(document.querySelectorAll("[data-repository-status-view]")).map((view) => ({
      view, refresh: view.querySelector("[data-status-refresh]"),
      fields: view.querySelector("[data-status-fields]"), note: view.querySelector("[data-status-note]"),
      branch: view.querySelector("[data-status-branch]"), commit: view.querySelector("[data-status-commit]"),
      changes: view.querySelector("[data-status-changes]"), version: 0, busy: false, loaded: false,
    }));
    async function read(item) {
      if (item.busy) return;
      const current = ++item.version;
      item.busy = true; item.refresh.disabled = true; item.fields.hidden = true;
      item.note.textContent = "Reading repository status…";
      try {
        const response = await fetch(item.view.dataset.statusUrl, {
          method: "GET", credentials: "same-origin", cache: "no-store", redirect: "error", referrerPolicy: "no-referrer",
        });
        if (current !== item.version) return;
        if (response.status !== 200) {
          const code = response.headers.get("x-devoption-error");
          item.note.textContent = code === "changed" ? "This checkout changed. Check its attachment before refreshing."
            : code === "unsupported" ? "Status is unavailable for this checkout layout or configuration."
            : code === "limited" ? "Status exceeded its read limit. This checkout needs a smaller inspection."
            : "Repository status could not be read. Check your session and checkout, then refresh.";
          return;
        }
        const fields = (await response.text()).split("\n");
        if (current !== item.version) return;
        if (fields.length !== 6 || fields[0].length > 4096 || /[\u0000-\u001f\u007f-\u009f]/.test(fields[0])
          || !/^(?:[a-f0-9]{40})?$/.test(fields[1]) || (!fields[0] && !fields[1])
          || fields.slice(2).some(value => !/^\d{1,4}$/.test(value) || Number(value) > 1024)) throw new Error("Invalid status");
        item.branch.textContent = fields[0] || "Detached HEAD";
        item.commit.textContent = fields[1] || "No commits yet";
        const labels = ["staged", "modified", "untracked entries", "conflicted"];
        const changes = fields.slice(2).map((value, index) => Number(value) ? `${Number(value)} ${labels[index]}` : "").filter(Boolean);
        item.changes.textContent = changes.length ? changes.join(" · ") : "Clean";
        item.fields.hidden = false; item.loaded = true;
        item.note.textContent = "Refresh to read the checkout again.";
      } catch {
        if (current === item.version) item.note.textContent = "Repository status could not be read. Check your session and checkout, then refresh.";
      } finally {
        if (current === item.version) { item.busy = false; item.refresh.disabled = false; }
      }
    }
    views.forEach(item => {
      item.refresh.disabled = false;
      item.note.textContent = "Open to read the current checkout.";
      item.view.addEventListener("toggle", () => { if (item.view.open && !item.loaded) read(item); });
      item.refresh.addEventListener("click", () => read(item));
      if (item.view.open) read(item);
    });
    window.addEventListener("pagehide", () => views.forEach(item => {
      ++item.version; item.busy = false; item.loaded = false; item.fields.hidden = true;
      item.refresh.disabled = true; item.note.textContent = "Status will refresh when opened.";
    }));
    window.addEventListener("pageshow", event => {
      if (event.persisted) views.forEach(item => { item.refresh.disabled = false; if (item.view.open) read(item); });
    });
  });
})();
