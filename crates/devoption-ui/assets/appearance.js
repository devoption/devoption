(() => {
  const settings = [
    { id: "appearance-mode", key: "devoption.appearance", attribute: "appearance", label: "Mode", values: ["dark", "light"] },
    { id: "appearance-accent", key: "devoption.accent", attribute: "accent", label: "Accent", values: ["purple", "blue", "green"] },
  ];
  let storageReadable = true;
  // Apply each saved choice independently, before body rendering.
  for (const setting of settings) {
    let value = setting.values[0];
    try {
      const saved = localStorage.getItem(setting.key);
      if (setting.values.includes(saved)) value = saved;
    } catch {
      storageReadable = false;
    }
    document.documentElement.dataset[setting.attribute] = value;
  }
  document.addEventListener("DOMContentLoaded", () => {
    const status = document.getElementById("appearance-status");
    const updateName = () => {
      document.getElementById("appearance-name").textContent = `Nord ${document.documentElement.dataset.accent}`;
    };
    updateName();
    status.textContent = storageReadable
      ? "Changes are saved only in this browser."
      : "Some saved choices could not be read. Defaults are in use for those choices.";

    for (const setting of settings) {
      const choice = document.getElementById(setting.id);
      choice.value = document.documentElement.dataset[setting.attribute];
      choice.disabled = false;
      choice.addEventListener("change", () => {
        const value = setting.values.includes(choice.value) ? choice.value : setting.values[0];
        choice.value = value;
        document.documentElement.dataset[setting.attribute] = value;
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
