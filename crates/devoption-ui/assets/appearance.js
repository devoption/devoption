(() => {
  const settings = [
    { id: "appearance-mode", key: "devoption.appearance", attribute: "appearance", label: "Mode", values: ["dark", "light", "system"] },
    { id: "appearance-accent", key: "devoption.accent", attribute: "accent", label: "Accent", values: ["purple", "blue", "green"] },
  ];
  let storageReadable = true;
  const readSettings = () => {
    storageReadable = true;
    for (const setting of settings) {
      let value = setting.values[0];
      try {
        const saved = localStorage.getItem(setting.key);
        if (setting.values.includes(saved)) value = saved;
      } catch {
        storageReadable = false;
      }
      setting.value = value;
    }
  };
  // Apply each saved choice independently, before body rendering on every page.
  readSettings();
  const deviceScheme = matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    for (const setting of settings) {
      const value = setting.attribute === "appearance" && setting.value === "system"
        ? (deviceScheme.matches ? "dark" : "light")
        : setting.value;
      document.documentElement.dataset[setting.attribute] = value;
    }
  };
  apply();
  deviceScheme.addEventListener("change", apply);
  let syncControls = () => {};
  // A history-restored document may predate a preference change on another page.
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) {
      readSettings();
      apply();
      syncControls();
    }
  });
  document.addEventListener("DOMContentLoaded", () => {
    const status = document.getElementById("appearance-status");
    if (!status) return; // Only the Appearance page has controls.
    const updateName = () => {
      document.getElementById("appearance-name").textContent = `Nord ${document.documentElement.dataset.accent}`;
    };
    syncControls = () => {
      updateName();
      for (const setting of settings) {
        document.getElementById(setting.id).value = setting.value;
      }
      status.textContent = storageReadable
        ? "Changes are saved only in this browser."
        : "Some saved choices could not be read. Defaults are in use for those choices.";
    };
    syncControls();

    for (const setting of settings) {
      const choice = document.getElementById(setting.id);
      choice.disabled = false;
      choice.addEventListener("change", () => {
        const value = setting.values.includes(choice.value) ? choice.value : setting.values[0];
        choice.value = value;
        setting.value = value;
        apply();
        updateName();
        try {
          localStorage.setItem(setting.key, value);
          status.textContent = `${setting.label} saved in this browser.`;
        } catch {
          status.textContent = `${setting.label} changed for this page only. Your browser could not save it.`;
        }
      });
    }
  });
})();
