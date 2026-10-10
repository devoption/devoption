const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");

const assets = join(__dirname, "../crates/devoption-ui/assets");
const script = readFileSync(join(assets, "appearance.js"), "utf8");
const key = "devoption.appearance";

// Run the shipped script against storage faults and a minimal event-driven DOM.
// Real browser checks cover native select/popover behavior and rendered layout.
function page(saved = new Map(), faults = {}) {
  const choice = Object.assign(new EventTarget(), { value: "dark", disabled: true });
  const status = { textContent: "Enable JavaScript to change appearance." };
  const document = Object.assign(new EventTarget(), {
    documentElement: { dataset: {} },
    getElementById: (id) => ({ "appearance-mode": choice, "appearance-status": status })[id],
  });
  runInNewContext(script, {
    document,
    get localStorage() {
      if (faults.access) throw new Error("Storage blocked");
      return {
        getItem(name) {
          if (faults.read) throw new Error("Read blocked");
          return saved.get(name) ?? null;
        },
        setItem(name, value) {
          if (faults.write) throw new Error("Quota exceeded");
          saved.set(name, value);
        },
      };
    },
  });
  return {
    root: document.documentElement,
    choice,
    status,
    ready: () => document.dispatchEvent(new Event("DOMContentLoaded")),
    choose(value) {
      choice.value = value;
      choice.dispatchEvent(new Event("change"));
    },
  };
}

test("a fresh browser starts dark without writing a preference", () => {
  const saved = new Map();
  const current = page(saved);
  assert.equal(current.root.dataset.appearance, "dark");
  assert.equal(current.choice.disabled, true);
  current.ready();
  assert.equal(current.choice.disabled, false);
  assert.equal(current.choice.value, "dark");
  assert.equal(saved.size, 0);
});

test("both choices survive a new page and apply before content is ready", () => {
  const saved = new Map([["unrelated", "preserve me"]]);
  for (const mode of ["light", "dark"]) {
    const current = page(saved);
    current.ready();
    current.choose(mode);
    assert.equal(current.root.dataset.appearance, mode);
    assert.equal(saved.get(key), mode);
    assert.equal(current.status.textContent, "Saved in this browser.");
    const reloaded = page(saved);
    assert.equal(reloaded.root.dataset.appearance, mode);
    reloaded.ready();
    assert.equal(reloaded.choice.value, mode);
  }
  assert.equal(saved.get("unrelated"), "preserve me");
  assert.equal(saved.size, 2);
});

test("unknown or malformed stored values safely use dark", () => {
  for (const value of ["", "system", "LIGHT", "null", '{"mode":"light"}', "<script>"]) {
    const current = page(new Map([[key, value]]));
    current.ready();
    assert.equal(current.root.dataset.appearance, "dark");
    assert.equal(current.choice.value, "dark");
  }
});

test("unavailable storage does not prevent rendering or page-local switching", () => {
  for (const faults of [{ access: true }, { read: true, write: true }]) {
    const current = page(new Map(), faults);
    current.ready();
    assert.equal(current.root.dataset.appearance, "dark");
    assert.match(current.status.textContent, /could not read/);
    current.choose("light");
    assert.equal(current.root.dataset.appearance, "light");
    assert.match(current.status.textContent, /page only.*could not save/);
    assert.equal(page(new Map(), faults).root.dataset.appearance, "dark");
  }
});

test("a failed write keeps the current page usable and preserves the previous choice", () => {
  const saved = new Map([[key, "light"]]);
  const current = page(saved, { write: true });
  current.ready();
  current.choose("dark");
  assert.equal(current.root.dataset.appearance, "dark");
  assert.equal(current.choice.value, "dark");
  assert.match(current.status.textContent, /could not save/);
  assert.equal(page(saved).root.dataset.appearance, "light");
});

test("text and interactive accents retain contrast on both palettes' surfaces", () => {
  const css = readFileSync(join(assets, "theme.css"), "utf8");
  const colors = (block) => Object.fromEntries(
    [...block.matchAll(/--color-([\w-]+):\s*(#[\da-f]{6});/g)].map((match) => match.slice(1)),
  );
  const luminance = (hex) => hex.slice(1).match(/../g).map((byte) => {
    const channel = parseInt(byte, 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  const contrast = (first, second) => {
    const values = [luminance(first), luminance(second)].sort((a, b) => b - a);
    return (values[0] + 0.05) / (values[1] + 0.05);
  };
  const dark = colors(css.match(/:root\s*\{([^}]+)\}/)[1]);
  const light = { ...dark, ...colors(css.match(/:root\[data-appearance="light"\]\s*\{([^}]+)\}/)[1]) };
  for (const palette of [dark, light]) {
    for (const surface of ["background", "sidebar", "surface", "selected"]) {
      for (const text of ["text", "muted"]) {
        assert.ok(contrast(palette[text], palette[surface]) >= 4.5, `${text} on ${surface}`);
      }
      assert.ok(contrast(palette.primary, palette[surface]) >= 3, `accent on ${surface}`);
    }
  }
});
