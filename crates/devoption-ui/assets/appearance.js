(() => {
  const isColor = (value) => typeof value === "string" && value.length === 7 && /^#[\da-f]{6}$/i.test(value);
  const settings = [
    { id: "appearance-mode", key: "devoption.appearance", attribute: "appearance", label: "Mode", values: ["dark", "light", "system"] },
    { id: "appearance-accent", key: "devoption.accent", attribute: "accent", label: "Accent", values: ["purple", "blue", "green"] },
    { id: "appearance-theme", key: "devoption.theme", attribute: "theme", label: "Theme", values: ["nord", "dracula", "tokyo-night"] },
  ];
  const [, accent, theme] = settings;
  const normalize = (setting, value) => {
    if (setting.attribute === "accent" && isColor(value)) return value.toLowerCase();
    return setting.values.includes(value) ? value : setting.values[0];
  };
  let storageReadable = true;
  const readSettings = () => {
    storageReadable = true;
    for (const setting of settings) {
      setting.value = setting.values[0];
      try {
        setting.value = normalize(setting, localStorage.getItem(setting.key));
      } catch {
        storageReadable = false;
      }
    }
  };
  // Apply saved choices before rendering; accent is one preset name or hex color.
  readSettings();
  const root = document.documentElement;
  const deviceScheme = matchMedia("(prefers-color-scheme: dark)");
  let syncControls = () => {};
  let showStorageStatus = () => {};
  const apply = () => {
    for (const setting of settings) {
      let value = setting.value;
      if (setting.attribute === "appearance" && value === "system") {
        value = deviceScheme.matches ? "dark" : "light";
      }
      root.dataset[setting.attribute] = isColor(value) ? "custom" : value;
    }
    if (isColor(accent.value)) {
      root.style.setProperty("--color-primary", accent.value);
    } else {
      root.style.removeProperty("--color-primary");
    }
    syncControls();
  };
  apply();
  deviceScheme.addEventListener("change", apply);
  // A history-restored document may predate a preference change on another page.
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) {
      readSettings();
      apply();
      showStorageStatus();
    }
  });

  // WCAG relative luminance for opaque sRGB colors, not an accessibility audit.
  const luminance = (hex) => hex.slice(1).match(/../g).map((byte) => {
    const channel = parseInt(byte, 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  const contrast = (first, second) => {
    const a = luminance(first);
    const b = luminance(second);
    return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
  };
  document.addEventListener("DOMContentLoaded", () => {
    const status = document.getElementById("appearance-status");
    if (!status) return; // Only the Appearance page has controls.
    const primary = document.getElementById("appearance-primary");
    const reset = document.getElementById("appearance-reset");
    const warning = document.getElementById("appearance-contrast");
    syncControls = () => {
      const custom = isColor(accent.value);
      for (const setting of settings) {
        document.getElementById(setting.id).value = isColor(setting.value) ? "custom" : setting.value;
      }
      const palette = getComputedStyle(root);
      const color = (name) => palette.getPropertyValue(`--color-${name}`).trim();
      primary.value = color("primary");
      document.getElementById("appearance-hex").textContent = primary.value;
      document.getElementById("appearance-theme-note").hidden =
        theme.value !== "dracula" || root.dataset.appearance !== "light";
      warning.hidden = !custom;
      if (custom) {
        const lowest = ["background", "sidebar", "surface", "selected"]
          .map((name) => ({ name, ratio: contrast(primary.value, color(name)) }))
          .reduce((a, b) => a.ratio < b.ratio ? a : b);
        // Compare the unrounded ratio; round down for consistent warning text.
        const ratio = (Math.floor(lowest.ratio * 100) / 100).toFixed(2);
        warning.textContent = lowest.ratio < 3
          ? `Low contrast: ${ratio}:1 on ${lowest.name} in ${root.dataset.appearance} mode. Aim for 3:1 for accents; choose another color or a preset. This checks accents only.`
          : `Lowest accent contrast: ${ratio}:1 in ${root.dataset.appearance} mode. This checks accent/background contrast only.`;
      } else {
        warning.textContent = "";
      }
    };
    showStorageStatus = () => {
      status.textContent = storageReadable
        ? "Changes are saved only in this browser."
        : "Some saved choices could not be read. Defaults are in use for those choices.";
    };
    syncControls();
    showStorageStatus();
    const change = (setting, value) => {
      setting.value = normalize(setting, value);
      apply();
      try {
        localStorage.setItem(setting.key, setting.value);
        status.textContent = `${setting.label} saved in this browser.`;
      } catch {
        status.textContent = `${setting.label} changed for this page only. Your browser could not save it.`;
      }
    };
    for (const setting of settings) {
      const choice = document.getElementById(setting.id);
      choice.disabled = false;
      choice.addEventListener("change", () => {
        change(setting, choice.value === "custom" ? primary.value : choice.value);
      });
    }
    primary.disabled = false;
    reset.disabled = false;
    primary.addEventListener("change", () => {
      if (!isColor(primary.value)) {
        syncControls();
        status.textContent = "Choose a six-digit hex color, such as #b48ead.";
        return;
      }
      change(accent, primary.value);
    });
    reset.addEventListener("click", () => change(accent, "purple"));
  });
})();
