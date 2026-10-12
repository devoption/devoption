const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");

const script = readFileSync(join(__dirname, "../crates/devoption-ui/assets/auth.js"), "utf8");
const csrf = "ab".repeat(32);
const nextCsrf = "cd".repeat(32);
const response = (status, token, retry) => ({
  status, headers: new Map([["x-csrf-token", token], ["retry-after", retry]]),
});
const tick = () => new Promise((resolve) => setImmediate(resolve));

// Exercise the shipped script with controlled responses and navigation races.
// Native focus, cookies, form validation and layout are checked in a real browser.
function page({ configured = true, form = true, initial = response(401) } = {}) {
  const nodes = new Map();
  let focused;
  function node(id) {
    const listeners = new Map();
    const attributes = new Map();
    const element = {
      textContent: "", value: "", disabled: false, hidden: false,
      addEventListener: (name, callback) => listeners.set(name, callback),
      emit: (name, event = {}) => listeners.get(name)?.(event),
      setAttribute: (name, value) => attributes.set(name, value),
      removeAttribute: (name) => attributes.delete(name),
      getAttribute: (name) => attributes.get(name),
      focus: () => { assert.equal(element.disabled, false); focused = id; },
    };
    nodes.set(id, element);
    return element;
  }
  ["profile-state", "profile-note", "profile-account", "profile-status", "page-title", "page-message"].forEach(node);
  if (form) ["signin-form", "auth-password", "auth-submit", "auth-signed-in", "auth-status"].forEach(node);
  const signouts = [node("profile-logout"), ...(form ? [node("page-logout")] : [])];
  const events = node("window");
  const calls = [];
  const queue = [initial];
  const navigation = [];
  const storage = new Proxy({}, { get() { throw new Error("Auth must not access browser storage"); } });
  let ready;
  runInNewContext(script, {
    document: {
      body: { dataset: { ownerConfigured: String(configured) } },
      getElementById: (id) => nodes.get(id) || null,
      querySelectorAll: () => signouts,
      addEventListener: (_, callback) => { ready = callback; },
      get cookie() { throw new Error("Auth must not read cookies"); },
    },
    window: { addEventListener: events.addEventListener, location: { assign: (url) => navigation.push(url) } },
    localStorage: storage, sessionStorage: storage, TextEncoder, URLSearchParams,
    fetch: async (url, options) => {
      calls.push({ url, ...options });
      assert.equal(options.method, "POST");
      assert.equal(options.credentials, "same-origin");
      assert.equal(options.cache, "no-store");
      assert.equal(options.redirect, "error");
      assert.equal(options.referrerPolicy, "no-referrer");
      assert.ok(queue.length, `Unexpected request: ${url}`);
      const result = await queue.shift();
      if (result instanceof Error) throw result;
      return result;
    },
  });
  ready();
  return {
    nodes, calls, queue, navigation, signouts,
    status: nodes.get(form ? "auth-status" : "profile-status"),
    focused: () => focused,
    event: events.emit,
    submit: (value = "fixture password only") => {
      nodes.get("auth-password").value = value;
      return nodes.get("signin-form").emit("submit", { preventDefault() {} });
    },
    logout: () => signouts[0].emit("click"),
  };
}

test("unconfigured preview performs no authentication requests", async () => {
  const current = page({ configured: false, form: false });
  await tick();
  assert.equal(current.calls.length, 0);
  assert.equal(current.nodes.get("profile-state").textContent, "Local preview");
  assert.equal(current.signouts[0].hidden, true);
});

test("initial recovery gates controls and restores a live account without signing in", async () => {
  let finish;
  const current = page({ initial: new Promise((resolve) => { finish = resolve; }) });
  assert.equal(current.nodes.get("auth-submit").disabled, true);
  assert.equal(current.nodes.get("auth-password").disabled, true);
  finish(response(204, csrf));
  await tick();
  assert.equal(current.nodes.get("signin-form").hidden, true);
  assert.equal(current.nodes.get("auth-signed-in").hidden, false);
  assert.equal(current.nodes.get("profile-state").textContent, "Owner");
  assert.ok(current.signouts.every((button) => !button.disabled && !button.hidden));
  assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session"]);
});

test("anonymous, failed and malformed recovery never report a signed-in account", async () => {
  for (const initial of [response(401), response(503), response(204, "invalid"), new Error("fixture")]) {
    const current = page({ initial });
    await tick();
    assert.equal(current.nodes.get("signin-form").hidden, false);
    assert.equal(current.nodes.get("auth-submit").disabled, false);
    assert.equal(current.signouts[0].hidden, true);
    assert.equal(current.nodes.get("profile-state").textContent, "Signed out");
    if (initial.status !== 401) assert.match(current.status.textContent, /Could not/);
  }
});

test("sign-in preserves exact password bytes, clears the field and confirms cookie retention", async () => {
  const current = page();
  await tick();
  current.queue.push(response(204, csrf), response(204, nextCsrf), response(204, nextCsrf));
  const work = current.submit("  fixture café +&=  ");
  assert.equal(current.nodes.get("auth-password").value, "");
  assert.equal(current.nodes.get("auth-submit").disabled, true);
  await work;
  assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session", "/auth/challenge", "/auth/sign-in", "/auth/session"]);
  const signin = current.calls[2];
  assert.equal(signin.headers["x-csrf-token"], csrf);
  assert.equal(signin.headers["content-type"], "application/x-www-form-urlencoded");
  assert.equal(signin.body.get("password"), "  fixture café +&=  ");
  assert.equal(current.status.textContent, "Signed in.");
  assert.equal(current.focused(), "page-title");
  current.queue.push(response(204));
  await current.logout();
  assert.equal(current.calls.at(-1).headers["x-csrf-token"], nextCsrf);
  assert.equal(current.status.textContent, "Signed out.");
  assert.equal(current.focused(), "auth-password");
});

test("invalid password lengths do not request a challenge", async () => {
  for (const value of ["", "é".repeat(513)]) {
    const current = page();
    await tick();
    await current.submit(value);
    assert.equal(current.calls.length, 1);
    assert.equal(current.nodes.get("auth-password").value, "");
    assert.equal(current.nodes.get("auth-password").getAttribute("aria-invalid"), "true");
    assert.equal(current.focused(), "auth-password");
  }
});

test("challenge failures stop password submission and keep fixed bounded errors", async () => {
  for (const [failure, message] of [
    [response(403), /expired or changed/],
    [response(503, null, "300"), /wait 300 seconds/],
    [response(503, null, "301"), /temporarily unavailable/],
    [response(429, null, "-1"), /temporarily unavailable/],
    [response(204, "malformed"), /could not be completed/],
    [new Error("private fixture detail"), /Could not reach/],
  ]) {
    const current = page();
    await tick();
    current.queue.push(failure);
    await current.submit();
    assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session", "/auth/challenge"]);
    assert.match(current.status.textContent, message);
    assert.doesNotMatch(current.status.textContent, /private fixture detail/);
    assert.equal(current.nodes.get("auth-password").value, "");
    assert.equal(current.nodes.get("auth-submit").disabled, false);
  }
});

test("failed password retries get a fresh challenge and never claim success", async () => {
  for (const [failure, message] of [
    [response(401), /Password not recognized/],
    [response(403), /expired or changed/],
    [response(429, null, "60"), /wait 60 seconds/],
    [response(503), /temporarily unavailable/],
    [new Error("fixture"), /Could not reach/],
  ]) {
    const current = page();
    await tick();
    current.queue.push(response(204, csrf), failure);
    await current.submit();
    assert.match(current.status.textContent, message);
    assert.equal(current.nodes.get("signin-form").hidden, false);
    assert.equal(current.nodes.get("auth-password").value, "");
    assert.equal(current.focused(), "auth-password");
    current.queue.push(response(204, nextCsrf), response(401));
    await current.submit();
    assert.equal(current.calls.at(-2).url, "/auth/challenge");
    assert.equal(current.calls.at(-1).headers["x-csrf-token"], nextCsrf);
  }
});

test("a blocked Secure cookie does not produce a successful account screen", async () => {
  const current = page();
  await tick();
  current.queue.push(response(204, csrf), response(204, nextCsrf), response(401));
  await current.submit();
  assert.match(current.status.textContent, /could not be retained/);
  assert.equal(current.nodes.get("signin-form").hidden, false);
  assert.equal(current.signouts[0].hidden, true);
});

test("stale logout never retries with a newer session and requires reload", async () => {
  const current = page({ initial: response(204, csrf) });
  await tick();
  current.queue.push(response(401));
  await current.logout();
  assert.match(current.status.textContent, /session changed or ended/);
  assert.equal(current.calls.length, 2);
  assert.equal(current.signouts[0].hidden, true);
  assert.deepEqual(current.navigation, []);
});

test("failed logout remains retryable and does not falsely show signed out", async () => {
  for (const failure of [response(503), new Error("fixture")]) {
    const current = page({ initial: response(204, csrf), form: false });
    await tick();
    current.queue.push(failure);
    await current.logout();
    assert.equal(current.nodes.get("profile-state").textContent, "Owner");
    assert.equal(current.signouts[0].disabled, false);
    assert.deepEqual(current.navigation, []);
    current.queue.push(response(204));
    await current.logout();
    assert.deepEqual(current.navigation, ["/sign-in"]);
  }
});

test("duplicate submissions and logout clicks produce one operation", async () => {
  const current = page();
  await tick();
  let finish;
  current.queue.push(new Promise((resolve) => { finish = resolve; }), response(204, nextCsrf), response(204, nextCsrf));
  const work = current.submit();
  await current.submit();
  assert.equal(current.calls.length, 2);
  finish(response(204, csrf));
  await work;
  current.queue.push(new Promise((resolve) => { finish = resolve; }));
  const logout = current.logout();
  await current.logout();
  assert.equal(current.calls.length, 5);
  finish(response(204));
  await logout;
});

test("pagehide discards pending recovery and bfcache restoration checks the live session", async () => {
  let finish;
  const current = page({ initial: new Promise((resolve) => { finish = resolve; }) });
  current.nodes.get("auth-password").value = "fixture";
  current.event("pagehide");
  assert.equal(current.nodes.get("auth-password").value, "");
  finish(response(204, csrf));
  await tick();
  assert.notEqual(current.nodes.get("profile-state").textContent, "Owner");
  current.queue.push(response(204, nextCsrf));
  current.event("pageshow", { persisted: true });
  await tick();
  current.queue.push(response(204));
  await current.logout();
  assert.equal(current.calls.at(-1).headers["x-csrf-token"], nextCsrf);
});

test("navigation during a challenge never sends the captured password afterward", async () => {
  const current = page();
  await tick();
  let finish;
  current.queue.push(new Promise((resolve) => { finish = resolve; }));
  const work = current.submit();
  current.event("pagehide");
  finish(response(204, csrf));
  await work;
  assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session", "/auth/challenge"]);
  assert.equal(current.nodes.get("auth-password").value, "");
});
