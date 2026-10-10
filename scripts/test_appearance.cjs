const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");

const assets = join(__dirname, "../crates/devoption-ui/assets");
const script = readFileSync(join(assets, "appearance.js"), "utf8");
const key = "devoption.appearance";
const accentKey = "devoption.accent";

// Run the shipped script against storage faults and a minimal event-driven DOM.
// Real browser checks cover native select/popover behavior and rendered layout.
function page(saved = new Map(), faults = {}) {
  const choice = Object.assign(new EventTarget(), { value: "dark", disabled: true });
  const accent = Object.assign(new EventTarget(), { value: "purple", disabled: true });
  const name = { textContent: "Nord purple" };
  const status = { textContent: "Enable JavaScript to change appearance." };
  const document = Object.assign(new EventTarget(), {
    documentElement: { dataset: {} },
    getElementById: (id) => ({
      "appearance-mode": choice, "appearance-accent": accent,
      "appearance-name": name, "appearance-status": status,
    })[id],
  });
  runInNewContext(script, {
    document,
    get localStorage() {
      if (faults.access) throw new Error("Storage blocked");
      return {
        getItem(name) {
          if (faults.read === true || faults.read === name) throw new Error("Read blocked");
          return saved.get(name) ?? null;
        },
        setItem(name, value) {
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
    name,
    status,
    ready: () => document.dispatchEvent(new Event("DOMContentLoaded")),
    choose(value) {
      choice.value = value;
      choice.dispatchEvent(new Event("change"));
    },
    chooseAccent(value) {
      accent.value = value;
      accent.dispatchEvent(new Event("change"));
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
  assert.equal(current.name.textContent, "Nord purple");
  assert.equal(saved.size, 0);
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
      assert.equal(current.name.textContent, `Nord ${accent}`);
      assert.equal(current.status.textContent, "Accent saved in this browser.");
      const reloaded = page(saved);
      assert.equal(reloaded.root.dataset.appearance, mode);
      assert.equal(reloaded.root.dataset.accent, accent);
      reloaded.ready();
      assert.equal(reloaded.choice.value, mode);
      assert.equal(reloaded.accent.value, accent);
      assert.equal(reloaded.name.textContent, `Nord ${accent}`);
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
  for (const value of ["", "system", "LIGHT", "null", '{"mode":"light"}', "<script>"]) {
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
    assert.equal(invalidAccent.name.textContent, "Nord purple");
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
    assert.equal(current.name.textContent, "Nord blue");
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
  assert.equal(current.name.textContent, "Nord blue");
  assert.match(current.status.textContent, /could not save/);
  assert.equal(page(saved).root.dataset.appearance, "light");
  assert.equal(page(saved).root.dataset.accent, "green");
});

test("text and interactive accents retain contrast across all six combinations", () => {
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
  const blocks = Object.fromEntries(
    [...css.matchAll(/([^{}]+)\{([^{}]+)\}/g)].map((match) => [match[1].trim(), colors(match[2])]),
  );
  for (const mode of ["dark", "light"]) {
    for (const accent of ["purple", "blue", "green"]) {
      const modeSelector = ':root[data-appearance="light"]';
      const accentSelector = `:root[data-accent="${accent}"]`;
      const combinedSelector = `${modeSelector}[data-accent="${accent}"]`;
      const palette = {
        ...blocks[":root"],
        ...(mode === "light" ? blocks[modeSelector] : {}),
        ...(accent !== "purple" ? blocks[accentSelector] : {}),
        ...(mode === "light" && accent !== "purple" ? blocks[combinedSelector] : {}),
      };
      if (accent !== "purple") {
        assert.ok(blocks[accentSelector]);
        assert.ok(blocks[combinedSelector]);
      }
      for (const surface of ["background", "sidebar", "surface", "selected"]) {
        for (const text of ["text", "muted"]) {
          assert.ok(contrast(palette[text], palette[surface]) >= 4.5, `${text} on ${surface}`);
        }
        assert.ok(contrast(palette.primary, palette[surface]) >= 3, `accent on ${surface}`);
      }
    }
  }
});
