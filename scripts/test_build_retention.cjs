const test = require("node:test");
const assert = require("node:assert/strict");
const {readFileSync} = require("node:fs");
const {runInNewContext} = require("node:vm");
const script = readFileSync(new URL("../crates/devoption-ui/assets/build-retention.js", `file://${__filename}`), "utf8");
const response = (status, csrf = null) => ({status, headers: {get: () => csrf}});
function page() {
  const listeners = new Map(), windowEvents = new Map(), calls = [], queue = [], navigation = [];
  const input = {value: "10", disabled: false}, status = {textContent: ""}, submit = {disabled: false};
  const form = {dataset: {project: "ab".repeat(16)}, reportValidity: () => true,
    querySelector: () => submit, addEventListener: (name, callback) => listeners.set(name, callback)};
  const nodes = {"retention-form": form, "retention-count": input, "retention-status": status};
  let ready;
  runInNewContext(script, {
    document: {getElementById: id => nodes[id], addEventListener: (_, callback) => {ready = callback;}},
    window: {addEventListener: (event, callback) => windowEvents.set(event, callback), location: {reload: () => navigation.push("reload")}},
    fetch: async (url, options) => {calls.push({url, ...options}); const item = queue.shift(); if (item instanceof Error) throw item; return await item;},
  });
  ready();
  return {input, status, submit, form, calls, queue, navigation,
    save: () => listeners.get("submit")({preventDefault() {}}), event: (name, value) => windowEvents.get(name)(value)};
}

test("retention saves with a fresh same-origin CSRF session and reloads for actual cleanup evidence", async () => {
  const p=page();p.input.value="1";p.queue.push(response(204,"ef".repeat(32)),response(204));await p.save();
  assert.deepEqual(p.calls.map(c=>c.url),["/auth/session",`/projects/${"ab".repeat(16)}/retention`]);
  assert.equal(p.calls[1].body,"1");assert.equal(p.calls[1].headers["x-csrf-token"],"ef".repeat(32));
  assert.equal(p.calls[1].credentials,"same-origin");assert.equal(p.calls[1].redirect,"error");
  assert.deepEqual(p.navigation,["reload"]);assert.doesNotMatch(p.status.textContent,/reclaimed|success/i);
});

test("invalid values do not request a session or mutate a policy", async () => {
  for(const value of ["0","1001","-1","1.5","1e2",""]){const p=page();p.input.value=value;await p.save();assert.equal(p.calls.length,0);}
  const p=page();p.form.dataset.project="../other";await p.save();assert.equal(p.calls.length,0);
});

test("uncertain saves stay disabled and never retry against stale evidence", async () => {
  for(const result of [response(403),response(503),new Error("connection lost")]){
    const p=page();p.queue.push(response(204,"ef".repeat(32)),result);await p.save();await p.save();
    assert.equal(p.calls.length,2);assert.equal(p.input.disabled,true);assert.equal(p.submit.disabled,true);
    assert.match(p.status.textContent,/could not be confirmed/);assert.deepEqual(p.navigation,[]);
  }
});

test("invalid sessions and late completion after navigation do not save or claim success", async () => {
  const p=page();p.queue.push(response(401));await p.save();assert.equal(p.calls.length,1);assert.match(p.status.textContent,/could not be confirmed/);
  const late=page();let finish;late.queue.push(new Promise(resolve=>{finish=resolve;}));const saving=late.save();
  late.event("pagehide");finish(response(204,"ef".repeat(32)));await saving;
  assert.equal(late.calls.length,1);assert.deepEqual(late.navigation,[]);
});
