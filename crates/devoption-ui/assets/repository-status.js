(function () {
  "use strict";
  document.addEventListener("DOMContentLoaded", () => {
    const views = Array.from(document.querySelectorAll("[data-repository-status-view]")).map((view) => ({
      view, refresh: view.querySelector("[data-status-refresh]"),
      fields: view.querySelector("[data-status-fields]"), note: view.querySelector("[data-status-note]"),
      branch: view.querySelector("[data-status-branch]"), commit: view.querySelector("[data-status-commit]"),
      changes: view.querySelector("[data-status-changes]"), version: 0, busy: false, requested: false,
    }));
    async function read(item) {
      if (item.busy) return;
      const current = ++item.version;
      item.requested = true;
      item.refresh.textContent = "Refresh status";
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
        const changes = fields.slice(2).map((value, index) => {
          const count = Number(value);
          const label = index === 2 && count === 1 ? "untracked entry" : labels[index];
          return count ? `${count} ${label}` : "";
        }).filter(Boolean);
        item.changes.textContent = changes.length ? changes.join(" · ") : "Clean";
        const checkedAt = new Date().toLocaleString(undefined, {dateStyle: "medium", timeStyle: "long"});
        item.fields.hidden = false;
        item.note.textContent = `Last checked ${checkedAt} (browser time).`;
      } catch {
        if (current === item.version) item.note.textContent = "Repository status could not be read. Check your session and checkout, then refresh.";
      } finally {
        if (current === item.version) { item.busy = false; item.refresh.disabled = false; }
      }
    }
    views.forEach(item => {
      item.refresh.disabled = false;
      item.note.textContent = "Not checked yet.";
      item.refresh.addEventListener("click", () => read(item));
    });
    window.addEventListener("pagehide", () => views.forEach(item => {
      ++item.version; item.busy = false; item.fields.hidden = true;
      item.refresh.disabled = true; item.note.textContent = item.requested ? "Status will refresh when you return." : "Not checked yet.";
    }));
    window.addEventListener("pageshow", event => {
      if (event.persisted) views.forEach(item => { item.refresh.disabled = false; if (item.requested) read(item); });
    });
  });
})();
