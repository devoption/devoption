const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");

const assets = join(__dirname, "../crates/devoption-ui/assets");
const script = readFileSync(join(assets, "appearance.js"), "utf8");
const key = "devoption.appearance";
const accentKey = "devoption.accent";
const themeKey = "devoption.theme";
const css = readFileSync(join(assets, "theme.css"), "utf8");
const blocks = Object.fromEntries([...css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]+)\}/g)].map((match) => [
  match[1].trim(), Object.fromEntries([...match[2].matchAll(/--color-([\w-]+):\s*([^;]+);/g)].map((color) => color.slice(1))),
]));
function paletteFor(mode, accent, theme = "nord") {
  const light = ':root[data-appearance="light"]';
  const themeSelector = `:root[data-theme="${theme}"]`;
  const themeLight = `${themeSelector}[data-appearance="light"]`;
  return {
    ...blocks[":root"],
    ...(mode === "light" ? blocks[light] : {}),
    ...blocks[`:root[data-accent="${accent}"]`],
    ...(mode === "light" ? blocks[`${light}[data-accent="${accent}"]`] : {}),
    ...blocks[themeSelector],
    ...blocks[`${themeSelector}[data-accent="${accent}"]`],
    ...(mode === "light" ? blocks[themeLight] : {}),
    ...(mode === "light" ? blocks[`${themeLight}[data-accent="${accent}"]`] : {}),
  };
}
function colorValue(palette, name) {
  const value = palette[name];
  const reference = /^var\(--color-([\w-]+)\)$/.exec(value);
  return reference ? colorValue(palette, reference[1]) : value;
}

// Run the shipped script against storage faults and a minimal event-driven DOM.
// Real browser checks cover native select/popover behavior and rendered layout.
function page(saved = new Map(), faults = {}, deviceDark = false, hasControls = true, paletteOverride = {}) {
  const choice = Object.assign(new EventTarget(), { value: "dark", disabled: true });
  const accent = Object.assign(new EventTarget(), { value: "purple", disabled: true });
  const theme = Object.assign(new EventTarget(), { value: "nord", disabled: true });
  const themeNote = { hidden: true };
  const status = { textContent: "Enable JavaScript to change appearance." };
  const primary = Object.assign(new EventTarget(), { value: "#b48ead", disabled: true });
  const reset = Object.assign(new EventTarget(), { disabled: true });
  const warning = { textContent: "", hidden: true };
  const hex = { textContent: "#b48ead" };
  const deviceScheme = Object.assign(new EventTarget(), { matches: deviceDark });
  const writes = [];
  const window = new EventTarget();
  const inline = new Map();
  const document = Object.assign(new EventTarget(), {
    documentElement: {
      dataset: {},
      style: { setProperty: (name, value) => inline.set(name, value), removeProperty: (name) => inline.delete(name) },
    },
    getElementById: (id) => hasControls ? ({
      "appearance-mode": choice, "appearance-accent": accent,
      "appearance-theme": theme, "appearance-theme-note": themeNote, "appearance-status": status,
      "appearance-primary": primary, "appearance-reset": reset,
      "appearance-contrast": warning, "appearance-hex": hex,
    })[id] : null,
  });
  const computedColor = (name) => colorValue({
    ...paletteFor(document.documentElement.dataset.appearance, document.documentElement.dataset.accent, document.documentElement.dataset.theme),
    ...paletteOverride,
    ...Object.fromEntries([...inline].map(([key, value]) => [key.replace("--color-", ""), value])),
  }, name);
  runInNewContext(script, {
    document,
    window,
    getComputedStyle: () => ({ getPropertyValue: (name) => computedColor(name.replace("--color-", "")) }),
    matchMedia(query) {
      assert.equal(query, "(prefers-color-scheme: dark)");
      return deviceScheme;
    },
    get localStorage() {
      if (faults.access) throw new Error("Storage blocked");
      return {
        getItem(name) {
          if (faults.read === true || faults.read === name) throw new Error("Read blocked");
          return saved.get(name) ?? null;
        },
        setItem(name, value) {
          writes.push([name, value]);
          if (faults.write === true || faults.write === name) throw new Error("Quota exceeded");
          saved.set(name, value);
        },
      };
    },
  });
  return {
    root: document.documentElement,
    choice,
    accent,
    theme, themeNote,
    status,
    writes,
    primary, reset, warning, hex, inline, computedColor,
    chooseColor(value) {
      primary.value = value;
      primary.dispatchEvent(new Event("change"));
    },
    resetAccent: () => reset.dispatchEvent(new Event("click")),
    restore() {
      window.dispatchEvent(Object.assign(new Event("pageshow"), { persisted: true }));
    },
    changeDevice(dark) {
      deviceScheme.matches = dark;
      deviceScheme.dispatchEvent(new Event("change"));
    },
    ready: () => document.dispatchEvent(new Event("DOMContentLoaded")),
    choose(value) {
      choice.value = value;
      choice.dispatchEvent(new Event("change"));
    },
    chooseAccent(value) {
      accent.value = value;
      accent.dispatchEvent(new Event("change"));
    },
    chooseTheme(value) {
      theme.value = value;
      theme.dispatchEvent(new Event("change"));
    },
  };
}

test("a fresh browser starts dark-purple without writing preferences", () => {
  const saved = new Map();
  const current = page(saved);
  assert.equal(current.root.dataset.appearance, "dark");
  assert.equal(current.root.dataset.accent, "purple");
  assert.equal(current.choice.disabled, true);
  assert.equal(current.accent.disabled, true);
  current.ready();
  assert.equal(current.choice.disabled, false);
  assert.equal(current.choice.value, "dark");
  assert.equal(current.accent.disabled, false);
  assert.equal(current.accent.value, "purple");
  assert.equal(current.theme.value, "nord");
  assert.equal(current.theme.disabled, false);
  assert.equal(current.themeNote.hidden, true);
  assert.equal(saved.size, 0);
});

test("pages without controls still apply preferences, follow System, and restore history", () => {
  const saved = new Map([[key, "system"], [accentKey, "blue"]]);
  const current = page(saved, {}, false, false);
  current.ready();
  assert.equal(current.root.dataset.appearance, "light");
  assert.equal(current.root.dataset.accent, "blue");
  current.changeDevice(true);
  assert.equal(current.root.dataset.appearance, "dark");
  const settings = page(saved);
  settings.ready();
  settings.choose("light");
  settings.chooseAccent("green");
  current.restore();
  assert.equal(current.root.dataset.appearance, "light");
  assert.equal(current.root.dataset.accent, "green");
  current.changeDevice(false);
  current.changeDevice(true);
  assert.equal(current.root.dataset.appearance, "light");
  assert.equal(current.writes.length, 0);
});

test("history restores Appearance controls and feedback without adding listeners or saving", () => {
  const saved = new Map([[key, "dark"], [accentKey, "purple"]]);
  const current = page(saved);
  current.ready();
  saved.set(key, "system");
  saved.set(accentKey, "green");
  current.restore();
  assert.equal(current.choice.value, "system");
  assert.equal(current.accent.value, "green");
  assert.equal(current.root.dataset.appearance, "light");
  assert.equal(current.writes.length, 0);
  current.restore();
  current.chooseAccent("blue");
  assert.equal(current.writes.length, 1);

  const faults = {};
  const blocked = page(saved, faults);
  blocked.ready();
  faults.read = key;
  blocked.restore();
  assert.equal(blocked.choice.value, "dark");
  assert.equal(blocked.accent.value, "blue");
  assert.match(blocked.status.textContent, /could not be read/);
});

test("all six combinations persist independently and apply before content is ready", () => {
  const saved = new Map([["unrelated", "preserve me"]]);
  for (const accent of ["purple", "blue", "green"]) {
    for (const mode of ["light", "dark"]) {
      const current = page(saved);
      current.ready();
      const priorAccent = current.root.dataset.accent;
      current.choose(mode);
      assert.equal(current.root.dataset.appearance, mode);
      assert.equal(current.root.dataset.accent, priorAccent);
      assert.equal(saved.get(key), mode);
      assert.equal(current.status.textContent, "Mode saved in this browser.");
      current.chooseAccent(accent);
      assert.equal(current.root.dataset.accent, accent);
      assert.equal(current.root.dataset.appearance, mode);
      assert.equal(saved.get(accentKey), accent);
      assert.equal(current.status.textContent, "Accent saved in this browser.");
      const reloaded = page(saved);
      assert.equal(reloaded.root.dataset.appearance, mode);
      assert.equal(reloaded.root.dataset.accent, accent);
      reloaded.ready();
      assert.equal(reloaded.choice.value, mode);
      assert.equal(reloaded.accent.value, accent);
    }
  }
  assert.equal(saved.get("unrelated"), "preserve me");
  assert.equal(saved.size, 3);
});

test("an existing mode preference is preserved when no accent has been saved", () => {
  const saved = new Map([[key, "light"]]);
  const current = page(saved);
  current.ready();
  assert.equal(current.choice.value, "light");
  assert.equal(current.accent.value, "purple");
  assert.equal(saved.size, 1);
});

test("unknown or malformed values fall back without discarding the other valid choice", () => {
  for (const value of ["", "auto", "SYSTEM", "LIGHT", "null", '{"mode":"light"}', "<script>"]) {
    const current = page(new Map([[key, value], [accentKey, "green"]]));
    current.ready();
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.choice.value, "dark");
    assert.equal(current.accent.value, "green");
    const invalidAccent = page(new Map([[key, "light"], [accentKey, value]]));
    invalidAccent.ready();
    assert.equal(invalidAccent.choice.value, "light");
    assert.equal(invalidAccent.root.dataset.accent, "purple");
    assert.equal(invalidAccent.accent.value, "purple");
  }
});

test("unavailable storage does not prevent rendering or page-local switching", () => {
  for (const faults of [{ access: true }, { read: true, write: true }]) {
    const current = page(new Map(), faults);
    current.ready();
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.root.dataset.accent, "purple");
    assert.match(current.status.textContent, /could not be read/);
    current.choose("light");
    assert.equal(current.root.dataset.appearance, "light");
    assert.match(current.status.textContent, /page only.*could not save/);
    current.chooseAccent("blue");
    assert.equal(current.root.dataset.accent, "blue");
    assert.match(current.status.textContent, /Accent changed for this page only.*could not save/);
    const reloaded = page(new Map(), faults);
    assert.equal(reloaded.root.dataset.appearance, "dark");
    assert.equal(reloaded.root.dataset.accent, "purple");
  }
});

test("one failed read leaves the other stored preference intact", () => {
  const saved = new Map([[key, "light"], [accentKey, "green"]]);
  const modeBlocked = page(saved, { read: key });
  assert.equal(modeBlocked.root.dataset.appearance, "dark");
  assert.equal(modeBlocked.root.dataset.accent, "green");
  const accentBlocked = page(saved, { read: accentKey });
  assert.equal(accentBlocked.root.dataset.appearance, "light");
  assert.equal(accentBlocked.root.dataset.accent, "purple");
});

test("a failed write keeps the current page usable and preserves the previous choice", () => {
  const saved = new Map([[key, "light"], [accentKey, "green"]]);
  const current = page(saved, { write: true });
  current.ready();
  current.choose("dark");
  assert.equal(current.root.dataset.appearance, "dark");
  assert.equal(current.choice.value, "dark");
  assert.match(current.status.textContent, /could not save/);
  current.chooseAccent("blue");
  assert.equal(current.accent.value, "blue");
  assert.equal(current.root.dataset.accent, "blue");
  assert.match(current.status.textContent, /could not save/);
  assert.equal(page(saved).root.dataset.appearance, "light");
  assert.equal(page(saved).root.dataset.accent, "green");
});

test("saved System resolves either device scheme before rendering and keeps every accent", () => {
  for (const deviceDark of [false, true]) {
    for (const accent of ["purple", "blue", "green"]) {
      const saved = new Map([[key, "system"], [accentKey, accent]]);
      const current = page(saved, {}, deviceDark);
      assert.equal(current.root.dataset.appearance, deviceDark ? "dark" : "light");
      assert.equal(current.root.dataset.accent, accent);
      current.ready();
      assert.equal(current.choice.value, "system");
      assert.equal(current.accent.value, accent);
      current.changeDevice(!deviceDark);
      assert.equal(current.root.dataset.appearance, deviceDark ? "light" : "dark");
      assert.equal(current.choice.value, "system");
      assert.equal(current.root.dataset.accent, accent);
      assert.equal(saved.get(key), "system");
      assert.equal(current.writes.length, 0);
    }
  }
});

test("System persists as a choice and explicit modes stop following device changes", () => {
  const saved = new Map([[accentKey, "blue"]]);
  const current = page(saved);
  current.ready();
  current.choose("system");
  assert.equal(saved.get(key), "system");
  for (const dark of [true, false]) {
    current.changeDevice(dark);
    assert.equal(current.root.dataset.appearance, dark ? "dark" : "light");
    assert.equal(current.choice.value, "system");
    const reloaded = page(saved, {}, dark);
    reloaded.ready();
    assert.equal(reloaded.root.dataset.appearance, dark ? "dark" : "light");
    assert.equal(reloaded.choice.value, "system");
  }
  assert.equal(current.writes.length, 1);
  for (const mode of ["light", "dark"]) {
    current.choose(mode);
    for (const dark of [true, false]) {
      current.changeDevice(dark);
      assert.equal(current.root.dataset.appearance, mode);
      assert.equal(current.choice.value, mode);
      assert.equal(current.root.dataset.accent, "blue");
      assert.equal(saved.get(key), mode);
    }
    const reloaded = page(saved, {}, mode === "light");
    assert.equal(reloaded.root.dataset.appearance, mode);
  }
  current.choose("system");
  current.changeDevice(true);
  current.chooseAccent("green");
  current.changeDevice(false);
  assert.equal(current.root.dataset.appearance, "light");
  assert.equal(current.root.dataset.accent, "green");
  assert.equal(current.choice.value, "system");
  assert.equal(saved.get(key), "system");
  assert.equal(current.writes.length, 5);
});

test("unavailable storage still allows page-local System mode without changing the default", () => {
  for (const faults of [{ access: true }, { read: true, write: true }]) {
    const current = page(new Map(), faults);
    current.ready();
    current.changeDevice(true);
    current.changeDevice(false);
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.choice.value, "dark");
    current.choose("system");
    assert.equal(current.root.dataset.appearance, "light");
    assert.match(current.status.textContent, /page only.*could not save/);
    current.changeDevice(true);
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.choice.value, "system");
    assert.equal(current.root.dataset.accent, "purple");
    assert.equal(page(new Map(), faults).root.dataset.appearance, "dark");
  }
});

test("both themes retain text and interactive contrast in every mode and preset", () => {
  const luminance = (hex) => hex.slice(1).match(/../g).map((byte) => {
    const channel = parseInt(byte, 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  const contrast = (first, second) => {
    const values = [luminance(first), luminance(second)].sort((a, b) => b - a);
    return (values[0] + 0.05) / (values[1] + 0.05);
  };
  for (const theme of ["nord", "dracula"]) {
    for (const mode of ["dark", "light"]) {
      for (const accent of ["purple", "blue", "green", "custom"]) {
        const palette = paletteFor(mode, accent, theme);
        for (const surface of ["background", "sidebar", "surface", "selected"]) {
          const context = `${theme} ${mode} ${accent} on ${surface}`;
          for (const text of ["text", "muted"]) {
            assert.ok(contrast(palette[text], palette[surface]) >= 4.5, `${text}: ${context}`);
          }
          // Arbitrary custom primaries have no contrast guarantee; focus still must.
          for (const role of accent === "custom" ? ["focus"] : ["primary", "focus"]) {
            assert.ok(contrast(colorValue(palette, role), palette[surface]) >= 3, `${role}: ${context}`);
          }
        }
      }
    }
  }
});

test("custom colors persist atomically and apply before rendering on every route", () => {
  const saved = new Map([[key, "system"]]);
  const current = page(saved);
  current.ready();
  assert.equal(current.primary.disabled, false);
  assert.equal(current.reset.disabled, false);
  current.chooseColor("#12ABef");
  assert.equal(current.accent.value, "custom");
  assert.equal(current.primary.value, "#12abef");
  assert.equal(current.hex.textContent, "#12abef");
  assert.equal(saved.get(accentKey), "#12abef");
  assert.deepEqual(current.writes, [[accentKey, "#12abef"]]);
  for (const hasControls of [true, false]) {
    const reloaded = page(saved, {}, true, hasControls);
    assert.equal(reloaded.inline.get("--color-primary"), "#12abef");
    assert.equal(reloaded.root.dataset.accent, "custom");
    reloaded.ready();
    assert.equal(reloaded.root.dataset.appearance, "dark");
    saved.set(accentKey, "blue");
    reloaded.restore();
    assert.equal(reloaded.inline.size, 0);
    saved.set(accentKey, "#12abef");
    reloaded.restore();
    assert.equal(reloaded.inline.get("--color-primary"), "#12abef");
  }
});

test("invalid stored colors fall back independently and invalid picker values do not save", () => {
  for (const value of ["custom", "#abc", "#12345678", "#gggggg", " #ffffff", "#ffffff ", "#ffffff\n", "#ffffff\r", "#ffffff\u2028", "red", "rgb(0,0,0)", "url(x)", "#000000;display:none"]) {
    const saved = new Map([[key, "light"], [accentKey, value]]);
    const current = page(saved);
    assert.equal(current.inline.size, 0);
    current.ready();
    assert.equal(current.choice.value, "light");
    assert.equal(current.accent.value, "purple");
    assert.equal(current.warning.hidden, true);
    current.chooseColor(value);
    assert.equal(current.inline.size, 0);
    assert.equal(current.writes.length, 0);
    assert.match(current.status.textContent, /six-digit hex/);
    assert.equal(saved.get(accentKey), value);
  }
});

test("custom colors survive mode changes and System updates the measured warning", () => {
  const saved = new Map([[key, "system"], [accentKey, "#ffffff"]]);
  const current = page(saved, {}, true);
  current.ready();
  assert.match(current.warning.textContent, /^Lowest accent contrast:.*dark mode/);
  current.changeDevice(false);
  assert.match(current.warning.textContent, /^Low contrast:.*light mode/);
  assert.equal(current.primary.value, "#ffffff");
  assert.equal(current.writes.length, 0);
  for (const mode of ["dark", "light"]) {
    current.choose(mode);
    current.changeDevice(mode === "light");
    assert.equal(current.root.dataset.appearance, mode);
    assert.equal(current.primary.value, "#ffffff");
    assert.equal(current.computedColor("focus"), mode === "dark" ? "#b48ead" : "#7e5a79");
    const reloaded = page(saved);
    reloaded.ready();
    assert.equal(reloaded.primary.value, "#ffffff");
    assert.equal(reloaded.choice.value, mode);
  }
});

test("presets and Reset to Purple remove the override and preserve the current mode", () => {
  for (const mode of ["light", "dark", "system"]) {
    const saved = new Map([[key, mode], [accentKey, "#abcdef"]]);
    const current = page(saved);
    current.ready();
    for (const preset of ["purple", "blue", "green"]) {
      current.chooseAccent(preset);
      assert.equal(current.inline.size, 0);
      assert.equal(current.warning.hidden, true);
      assert.equal(current.warning.textContent, "");
      assert.equal(saved.get(accentKey), preset);
      const before = current.primary.value;
      current.chooseAccent("custom");
      assert.equal(current.inline.get("--color-primary"), before);
      assert.equal(saved.get(accentKey), before);
    }
    current.resetAccent();
    assert.equal(current.inline.size, 0);
    assert.equal(current.accent.value, "purple");
    assert.equal(current.choice.value, mode);
    assert.equal(saved.get(key), mode);
    const reloaded = page(saved);
    reloaded.ready();
    assert.equal(reloaded.accent.value, "purple");
    assert.equal(reloaded.primary.value, mode === "dark" ? "#b48ead" : "#7e5a79");
  }
});

test("custom save and reset failures leave prior storage intact and explain page-local changes", () => {
  const saved = new Map([[key, "light"], [accentKey, "green"]]);
  const faults = { write: accentKey };
  const current = page(saved, faults);
  current.ready();
  current.chooseColor("#abcdef");
  assert.equal(current.primary.value, "#abcdef");
  assert.equal(current.choice.value, "light");
  assert.equal(saved.get(accentKey), "green");
  assert.match(current.status.textContent, /page only.*could not save/);
  faults.write = false;
  current.chooseColor("#123456");
  faults.write = accentKey;
  current.resetAccent();
  assert.equal(current.accent.value, "purple");
  assert.equal(current.inline.size, 0);
  assert.equal(saved.get(accentKey), "#123456");
  assert.match(current.status.textContent, /page only.*could not save/);
  const reloaded = page(saved, { read: accentKey });
  reloaded.ready();
  assert.equal(reloaded.accent.value, "purple");
  assert.equal(reloaded.choice.value, "light");
  assert.match(reloaded.status.textContent, /could not be read/);
  const blocked = page(saved, { access: true });
  blocked.ready();
  blocked.chooseColor("#ffffff");
  assert.equal(blocked.primary.value, "#ffffff");
  assert.match(blocked.status.textContent, /page only.*could not save/);
});

test("contrast uses known sRGB ratios, the weakest surface, and an unrounded 3:1 threshold", () => {
  for (const [foreground, background, ratio, low] of [
    ["#ffffff", "#000000", "21.00", false],
    ["#000000", "#ffffff", "21.00", false],
    ["#ffffff", "#ffffff", "1.00", true],
    ["#ff0000", "#000000", "5.25", false],
    ["#0a0a0a", "#000000", "1.06", true],
    ["#949494", "#ffffff", "3.03", false],
    ["#959595", "#ffffff", "2.99", true],
  ]) {
    const palette = Object.fromEntries(["background", "sidebar", "surface", "selected"].map((name) => [name, background]));
    const current = page(new Map([[accentKey, foreground]]), {}, false, true, palette);
    current.ready();
    assert.ok(current.warning.textContent.includes(`${ratio}:1`), current.warning.textContent);
    assert.equal(current.warning.textContent.startsWith("Low contrast:"), low);
    assert.match(current.warning.textContent, /checks accents?(\/background contrast)? only/);
    assert.equal(current.warning.hidden, false);
  }
  for (const surface of ["background", "sidebar", "surface", "selected"]) {
    const current = page(new Map([[accentKey, "#ffffff"]]), {}, false, true, { [surface]: "#ffffff" });
    current.ready();
    assert.match(current.warning.textContent, new RegExp(`Low contrast: 1.00:1 on ${surface}`));
  }
});

test("missing or invalid themes preserve legacy preferences without migration writes", () => {
  for (const theme of [null, "", "Dracula", "unknown", "dracula\n", "#282a36"]) {
    for (const mode of ["dark", "light", "system"]) {
      for (const accent of ["purple", "blue", "green", "#abcdef"]) {
        const saved = new Map([[key, mode], [accentKey, accent]]);
        if (theme !== null) saved.set(themeKey, theme);
        const before = new Map(saved);
        const current = page(saved);
        assert.equal(current.root.dataset.theme, "nord");
        current.ready();
        assert.equal(current.theme.value, "nord");
        assert.equal(current.choice.value, mode);
        assert.equal(current.accent.value, accent.startsWith("#") ? "custom" : accent);
        assert.deepEqual(saved, before);
        assert.equal(current.writes.length, 0);
      }
    }
  }
});

test("theme switching persists independently, preserves accents, and reset keeps theme and mode", () => {
  const presets = {
    nord: { dark: ["#b48ead", "#81a1c1", "#a3be8c"], light: ["#7e5a79", "#4b6f93", "#4f7044"] },
    dracula: { dark: ["#bd93f9", "#8be9fd", "#50fa7b"], light: ["#7043a0", "#176e87", "#247237"] },
  };
  for (const mode of ["dark", "light", "system"]) {
    for (const deviceDark of [true, false]) {
      for (const accent of ["purple", "blue", "green", "#abcdef"]) {
        const saved = new Map([[key, mode], [accentKey, accent]]);
        const current = page(saved, {}, deviceDark);
        current.ready();
        for (const theme of ["dracula", "nord", "dracula"]) {
          current.chooseTheme(theme);
          const resolved = mode === "system" ? (deviceDark ? "dark" : "light") : mode;
          const expected = accent.startsWith("#") ? accent : presets[theme][resolved][["purple", "blue", "green"].indexOf(accent)];
          assert.equal(current.primary.value, expected);
          assert.equal(current.choice.value, mode);
          assert.equal(saved.get(accentKey), accent);
          assert.equal(saved.get(key), mode);
          assert.equal(saved.get(themeKey), theme);
          assert.ok(current.writes.every(([key]) => key === themeKey));
          const reloaded = page(saved, {}, deviceDark);
          assert.equal(reloaded.root.dataset.theme, theme);
          reloaded.ready();
          assert.equal(reloaded.primary.value, expected);
        }
        current.resetAccent();
        assert.equal(saved.get(themeKey), "dracula");
        assert.equal(current.choice.value, mode);
        assert.equal(current.accent.value, "purple");
        assert.equal(current.inline.size, 0);
      }
    }
  }
});

test("theme storage faults are isolated and history refreshes pages with or without controls", () => {
  const saved = new Map([[key, "system"], [accentKey, "#abcdef"], [themeKey, "dracula"]]);
  const unreadable = page(saved, { read: themeKey });
  unreadable.ready();
  assert.equal(unreadable.theme.value, "nord");
  assert.equal(unreadable.choice.value, "system");
  assert.equal(unreadable.primary.value, "#abcdef");
  assert.match(unreadable.status.textContent, /could not be read/);
  for (const faults of [{ write: themeKey }, { access: true }]) {
    const current = page(saved, faults);
    current.ready();
    current.chooseTheme("nord");
    assert.equal(current.theme.value, "nord");
    assert.equal(saved.get(themeKey), "dracula");
    assert.match(current.status.textContent, /Theme changed for this page only/);
  }
  for (const hasControls of [true, false]) {
    const current = page(saved, {}, false, hasControls);
    assert.equal(current.root.dataset.theme, "dracula");
    current.ready();
    saved.set(themeKey, "nord");
    current.restore();
    assert.equal(current.root.dataset.theme, "nord");
    saved.set(themeKey, "dracula");
    current.restore();
    current.changeDevice(true);
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.computedColor("background"), "#282a36");
    assert.equal(current.computedColor("primary"), "#abcdef");
    assert.equal(current.writes.length, 0);
    if (hasControls) assert.equal(current.theme.value, "dracula");
  }
});

test("theme and System changes recalculate custom contrast and identify adapted Dracula light", () => {
  const current = page(new Map([[key, "system"], [accentKey, "#282a36"]]), {}, true);
  current.ready();
  const nordWarning = current.warning.textContent;
  current.chooseTheme("dracula");
  assert.match(current.warning.textContent, /Low contrast: 1.00:1 on background in dark mode/);
  assert.notEqual(current.warning.textContent, nordWarning);
  assert.equal(current.themeNote.hidden, true);
  current.changeDevice(false);
  assert.equal(current.primary.value, "#282a36");
  assert.match(current.warning.textContent, /^Lowest accent contrast:.*light mode/);
  assert.equal(current.themeNote.hidden, false);
  assert.equal(current.computedColor("focus"), "#7043a0");
  current.chooseColor("#f8f8f2");
  assert.match(current.warning.textContent, /Low contrast: 1.00:1 on background in light mode/);
  current.chooseTheme("nord");
  assert.equal(current.themeNote.hidden, true);
  assert.equal(current.primary.value, "#f8f8f2");
  assert.notEqual(current.warning.textContent.includes("1.00:1"), true);
  current.chooseTheme("dracula");
  current.choose("dark");
  current.changeDevice(false);
  assert.equal(current.root.dataset.appearance, "dark");
  assert.equal(current.themeNote.hidden, true);
  assert.equal(current.computedColor("focus"), "#bd93f9");
});
