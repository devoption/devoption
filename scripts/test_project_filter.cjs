const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");
const script = readFileSync(join(__dirname, "../crates/devoption-ui/assets/project-filter.js"), "utf8");

function page(names, restored = "") {
  let focused, ready;
  const node = () => {
    const listeners = new Map();
    return {
      hidden: false, disabled: false, value: "", textContent: "",
      addEventListener: (event, fn) => listeners.set(event, fn),
      emit: (event) => listeners.get(event)?.(),
      focus() { focused = this; },
      set innerHTML(_) { throw new Error("Filtering must not replace markup"); },
    };
  };
  const filter = node(), query = node(), clear = node(), status = node(), events = node();
  filter.hidden = true; query.value = restored;
  const items = (names || []).map(name => ({ ...node(),
    textContent: name + " unrelated repository checkout path",
    attachment: { value: "Retained checkout", open: true },
    querySelector: selector => { assert.equal(selector, "[data-project-name]"); return {textContent: name}; },
  }));
  const nodes = {"project-filter": names ? filter : null, "project-query": query, "project-filter-clear": clear, "project-filter-status": status};
  const forbidden = new Proxy({}, {get() { throw new Error("No browser storage access"); }});
  runInNewContext(script, {
    document: {
      getElementById: id => nodes[id],
      querySelectorAll: selector => { assert.equal(selector, "[data-project-item]"); return items; },
      addEventListener: (_, fn) => {ready = fn;},
    },
    window: {addEventListener: events.addEventListener},
    localStorage: forbidden, sessionStorage: forbidden,
    fetch: () => {throw new Error("No network requests");},
  });
  ready();
  return {filter, query, clear, status, items, focused: () => focused, event: events.emit,
    input: value => {query.value = value; query.emit("input");},
    visible: () => names.filter((_, index) => !items[index].hidden),
  };
}

test("filter matches only names, ignores case and surrounding query spaces, and preserves duplicate rows", () => {
  const p = page(["Application", "Documentation", "application"]);
  assert.equal(p.filter.hidden, false); assert.equal(p.clear.disabled, true);
  assert.equal(p.status.textContent, "3 projects");
  p.input(" APPLI ");
  assert.deepEqual(p.visible(), ["Application", "application"]);
  assert.equal(p.status.textContent, "2 of 3 projects");
  p.input("checkout"); assert.deepEqual(p.visible(), []);
  assert.equal(p.status.textContent, "No projects match. Clear the filter to see all projects.");
});

test("clear restores all original rows, attachment state and keyboard focus", () => {
  const p = page(["Application", "Documentation"]);
  const original = p.items.slice(), attachment = p.items[0].attachment;
  p.input("missing"); assert.equal(p.clear.disabled, false);
  p.clear.emit("click");
  assert.deepEqual(p.visible(), ["Application", "Documentation"]);
  assert.equal(p.query.value, ""); assert.equal(p.focused(), p.query);
  assert.equal(p.clear.disabled, true); assert.equal(p.status.textContent, "2 projects");
  assert.equal(p.items[0], original[0]); assert.equal(p.items[0].attachment, attachment);
  assert.deepEqual(attachment, {value: "Retained checkout", open: true});
});

test("Unicode normalization and punctuation are literal; restored fields recompute visibility", () => {
  const p = page(["Café 工具", "<app>[a].*", "Documentation"], "CAFE\u0301");
  assert.deepEqual(p.visible(), ["Café 工具"]);
  p.input("工具"); assert.deepEqual(p.visible(), ["Café 工具"]);
  p.input("<app>[a].*"); assert.deepEqual(p.visible(), ["<app>[a].*"]);
  p.query.value = "doc"; p.event("pageshow"); assert.deepEqual(p.visible(), ["Documentation"]);
  p.input("   "); assert.equal(p.visible().length, 3); assert.equal(p.clear.disabled, false);
  p.clear.emit("click"); assert.equal(p.clear.disabled, true);
});

test("single-project counts are readable and pages without the filter remain untouched", () => {
  const p = page(["Only project"]); assert.equal(p.status.textContent, "1 project");
  assert.equal(page(null).filter.hidden, true);
});
