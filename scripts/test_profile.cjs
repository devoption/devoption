const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");
const script = readFileSync(join(__dirname, "../crates/devoption-ui/assets/profile.js"), "utf8");
const tick = () => new Promise(resolve => setImmediate(resolve));
const response = (status, name = "", csrf = "ab".repeat(32)) => ({status, text: async () => name, headers: new Map([["x-csrf-token", csrf]])});
function page({configured = true, form = true} = {}) {
  const nodes = new Map(); const events = new Map(); const windowEvents = new Map(); const calls = []; const queue = [];
  for (const id of ["profile-state", "profile-title", "profile-initials", "profile-avatar-icon", ...(form ? ["profile-form", "display-name", "profile-save", "profile-reload", "profile-save-status", "profile-signin", "profile-role"] : [])]) {
    const listeners = new Map(); const attributes = new Map();
    nodes.set(id, { value: "", textContent: "", hidden: false, disabled: true, focused: false,
      setAttribute: (key, value) => attributes.set(key, value), getAttribute: key => attributes.get(key),
      addEventListener: (key, callback) => listeners.set(key, callback), emit: (key, event = {}) => listeners.get(key)?.(event),
      focus() { this.focused = true; },
      set innerHTML(_) { throw new Error("Names must be text"); }
    });
  }
  const storage = new Proxy({}, {get() {throw new Error("Profile must not use browser storage");}});
  runInNewContext(script, { document: {getElementById: id => nodes.get(id), addEventListener: (key, fn) => events.set(key, fn), body: {dataset: {ownerConfigured: String(configured)}}, get cookie() {throw new Error("No cookie reads");}},
    window: {addEventListener: (key, fn) => windowEvents.set(key, fn)}, TextEncoder, localStorage: storage, sessionStorage: storage,
    fetch: async (url, options) => {calls.push({url, ...options}); assert.equal(options.credentials, "same-origin"); assert.equal(options.cache, "no-store"); assert.equal(options.redirect, "error"); assert.equal(options.referrerPolicy, "no-referrer"); assert.ok(queue.length, `Unexpected fetch ${url}`); const next = await queue.shift(); if (next instanceof Error) throw next; return next;}
  });
  events.get("DOMContentLoaded")();
  return {nodes, calls, queue, session: detail => events.get("devoption-session")({detail}), event: key => windowEvents.get(key)(), save: name => {nodes.get("display-name").value = name; return nodes.get("profile-form").emit("submit", {preventDefault(){}});}, status: () => nodes.get("profile-save-status").textContent};
}
async function loaded(name = "") {const p = page(); p.queue.push(response(200, name)); p.session("signed-in"); await tick(); return p;}

test("profile loads only after a verified session and shows Unicode names as text", async () => {
  const p = page(); assert.equal(p.calls.length, 0); p.queue.push(response(200, "Élodie 林 <&>")); p.session("signed-in"); await tick();
  assert.equal(p.nodes.get("profile-state").textContent, "Élodie 林 <&>"); assert.equal(p.nodes.get("display-name").value, "Élodie 林 <&>"); assert.equal(p.nodes.get("profile-initials").textContent, "É林"); assert.equal(p.nodes.get("profile-save").disabled, false);
  p.session("signed-out"); assert.equal(p.nodes.get("profile-title").textContent, "Account"); assert.equal(p.nodes.get("profile-initials").hidden, true); assert.equal(p.nodes.get("profile-save").disabled, true);
});
test("saving and clearing use a fresh CSRF response and update only after success", async () => {
  const p = await loaded("Old name");
  for (const name of ["Alice Smith", "林", ""]) {
    p.queue.push(response(204), response(204)); await p.save(name);
    assert.equal(p.nodes.get("profile-state").textContent, name || "Account"); assert.equal(p.nodes.get("profile-initials").textContent, name === "Alice Smith" ? "AS" : name);
    const call = p.calls.at(-1); assert.equal(call.url, "/account/profile"); assert.equal(call.method, "POST"); assert.equal(call.headers["x-csrf-token"], "ab".repeat(32)); assert.equal(call.body, name);
  }
  assert.match(p.status(), /Name cleared/);
});
test("invalid byte length, whitespace, controls and direction overrides never send", async () => {
  const p = await loaded(); const before = p.calls.length;
  for (const name of [" Alice", "Alice ", "Ada\uFEFF", "\uFEFFAda", "a\nb", "a\u0085", "a\u202e", "é".repeat(65)]) {await p.save(name); assert.match(p.status(), /128 UTF-8 bytes/);}
  assert.equal(p.calls.length, before); assert.equal(p.nodes.get("display-name").focused, true);
});
test("duplicate submissions wait for confirmation and navigation discards late results", async () => {
  const p = await loaded("Before"); let resolve;
  p.queue.push(response(204), new Promise(done => resolve = done));
  const first = p.save("After"); await tick(); const count = p.calls.length; await p.save("Duplicate"); assert.equal(p.calls.length, count);
  assert.equal(p.nodes.get("profile-state").textContent, "Before"); assert.equal(p.nodes.get("profile-form").getAttribute("aria-busy"), "true");
  p.event("pagehide"); resolve(response(204)); await first; assert.notEqual(p.status(), "Name saved.");
});
test("unconfirmed save keeps the input and requires reloading before another write", async () => {
  for (const failure of [response(400), response(503), new Error("disconnected")]) {
    const p = await loaded("Before"); p.queue.push(response(204), failure); await p.save("Attempt");
    assert.equal(p.nodes.get("display-name").value, "Attempt"); assert.equal(p.nodes.get("profile-state").textContent, "Before"); assert.match(p.status(), /could not be confirmed/); assert.equal(p.nodes.get("profile-save").disabled, true); assert.equal(p.nodes.get("profile-reload").hidden, false);
    p.queue.push(response(200, "Actual saved name")); await p.nodes.get("profile-reload").emit("click"); assert.equal(p.nodes.get("display-name").value, "Actual saved name"); assert.equal(p.nodes.get("profile-save").disabled, false);
  }
});
test("missing, expired or malformed session never supplies a mutation credential", async () => {
  for (const session of [response(401), response(204, "", "invalid"), response(503)]) {
    const p = await loaded(); p.queue.push(session); await p.save("Attempt"); assert.equal(p.calls.at(-1).url, "/auth/session"); assert.equal(p.nodes.get("profile-signin").hidden, false); assert.equal(p.nodes.get("profile-save").disabled, true);
  }
  const p = await loaded(); p.queue.push(response(204), response(401)); await p.save("Attempt"); assert.equal(p.nodes.get("profile-signin").hidden, false);
});
test("failed or invalid loads expose retry without populating the name", async () => {
  for (const failure of [response(503), response(200, "a\n"), response(200, "é".repeat(65)), new Error("network")]) {
    const p = page(); p.queue.push(failure); p.session("signed-in"); await tick(); assert.match(p.status(), /could not be loaded/); assert.equal(p.nodes.get("profile-save").disabled, true); assert.equal(p.nodes.get("profile-reload").hidden, false);
  }
});
test("late loads cannot restore an expired session or overwrite a new recovery", async () => {
  const p = page(); let resolve; p.queue.push(new Promise(done => resolve = done)); p.session("signed-in"); p.session("unknown"); resolve(response(200, "Late name")); await tick(); assert.equal(p.nodes.get("profile-state").textContent, "Check account"); assert.equal(p.nodes.get("profile-save").disabled, true);
  p.queue.push(response(200, "Recovered")); p.session("signed-in"); await tick(); assert.equal(p.nodes.get("display-name").value, "Recovered");
});
test("the menu works without a form and unconfigured pages make no requests", async () => {
  const p = page({form: false}); p.queue.push(response(200, "Sample Person")); p.session("signed-in"); await tick(); assert.equal(p.nodes.get("profile-initials").textContent, "SP");
  const empty = page({configured:false}); assert.equal(empty.calls.length, 0); assert.match(empty.status(), /Owner setup is required/); assert.equal(empty.nodes.get("profile-role").hidden, true);
});
