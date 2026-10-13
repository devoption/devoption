(() => {
  document.addEventListener("DOMContentLoaded", () => {
    const triggers = Array.from(document.querySelectorAll("[data-open-dialog]"));
    for (const trigger of triggers) {
      const dialog = document.getElementById(trigger.dataset.openDialog);
      if (!dialog || typeof dialog.showModal !== "function") continue;
      trigger.disabled = false;
      trigger.addEventListener("click", () => {
        if (dialog.open) return;
        dialog.showModal();
        dialog.dispatchEvent(new Event("dialogopened"));
        (dialog.querySelector("input:not(:disabled)") || dialog.querySelector("h2")).focus();
      });
      dialog.querySelectorAll("[data-close-dialog]").forEach(button => {
        button.addEventListener("click", () => dialog.close());
      });
      dialog.addEventListener("close", () => trigger.focus());
    }
    // Do not restore a modal over an account state that has not been rechecked.
    window.addEventListener("pagehide", () => {
      document.querySelectorAll("dialog[open]").forEach(dialog => dialog.close());
    });
  });
})();
