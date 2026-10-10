(() => {
  const key = "devoption.appearance";
  let mode = "dark";
  let storageReadable = true;
  try {
    if (localStorage.getItem(key) === "light") mode = "light";
  } catch {
    storageReadable = false;
  }

  // This script runs in the head so the saved mode applies before body rendering.
  document.documentElement.dataset.appearance = mode;
  document.addEventListener("DOMContentLoaded", () => {
    const choice = document.getElementById("appearance-mode");
    const status = document.getElementById("appearance-status");
    choice.value = mode;
    choice.disabled = false;
    status.textContent = storageReadable
      ? "Saved only in this browser when you choose a mode."
      : "Your browser could not read the saved choice. Using dark mode.";

    choice.addEventListener("change", () => {
      mode = choice.value === "light" ? "light" : "dark";
      choice.value = mode;
      document.documentElement.dataset.appearance = mode;
      try {
        localStorage.setItem(key, mode);
        status.textContent = "Saved in this browser.";
      } catch {
        status.textContent = "This choice applies to this page only. Your browser could not save it.";
      }
    });
  });
})();
