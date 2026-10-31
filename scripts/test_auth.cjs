const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const { runInNewContext } = require("node:vm");

const script = readFileSync(join(__dirname, "../crates/devoption-ui/assets/auth.js"), "utf8");
const csrf = "ab".repeat(32);
const nextCsrf = "cd".repeat(32);
const response = (status, token, retry, code) => ({
  status, headers: new Map([["x-csrf-token", token], ["retry-after", retry], ["x-devoption-error", code]]),
});
const tick = () => new Promise((resolve) => setImmediate(resolve));

// Exercise the shipped script with controlled responses and navigation races.
// Native focus, cookies, form validation and layout are checked in a real browser.
function page({ configured = true, form = true, profile = true, projects = false, rename, repositories = false, picker = false, initial = response(401) } = {}) {
  const nodes = new Map();
  let focused;
  function node(id) {
    const listeners = new Map();
    const attributes = new Map();
    const element = {
      textContent: "", value: "", disabled: false, hidden: false, dataset: {},
      addEventListener: (name, callback) => listeners.set(name, callback),
      emit: (name, event = {}) => listeners.get(name)?.(event),
      setAttribute: (name, value) => attributes.set(name, value),
      removeAttribute: (name) => attributes.delete(name),
      getAttribute: (name) => attributes.get(name),
      focus: () => { assert.equal(element.disabled, false); focused = id; },
      children: [],
      replaceChildren: (...children) => { element.children = children; element.value = children[0]?.value || ""; },
    };
    nodes.set(id, element);
    return element;
  }
  ["page-title", "page-message"].forEach(node);
  if (profile) ["profile-state", "profile-note", "profile-account", "profile-status"].forEach(node);
  if (form) ["signin-form", "auth-email", "auth-password", "auth-submit", "auth-signed-in", "auth-recheck", "auth-status"].forEach(node);
  if (projects) ["project-form", "project-name", "project-submit", "project-status"].forEach(node);
  if (rename !== undefined) nodes.get("project-form").dataset.project = rename;
  const repositoryForms = repositories ? [0, 1].map((index) => {
    const form = node(`repository-${index}-form`);
    form.dataset.project = (index ? "02" : "01").repeat(16);
    const fields = new Map(["path", "submit", "status"].map((field) => [`[data-repository-${field}]`, node(`repository-${index}-${field}`)]));
    if (picker) ["choice", "refresh", "options-status"].forEach((field) => fields.set(`[data-repository-${field}]`, node(`repository-${index}-${field}`)));
    form.querySelector = (selector) => fields.get(selector);
    const disclosure = node(`repository-${index}-disclosure`);
    disclosure.open = false;
    form.closest = () => disclosure;
    return form;
  }) : [];
  const signouts = [...(profile ? [node("profile-logout")] : []), ...(form ? [node("page-logout")] : [])];
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
      createElement: (tag) => node(`created-${tag}-${nodes.size}`),
      querySelectorAll: (selector) => selector === ".repository-form" ? repositoryForms : signouts,
      addEventListener: (_, callback) => { ready = callback; },
      dispatchEvent: () => {},
      get cookie() { throw new Error("Auth must not read cookies"); },
    },
    window: { addEventListener: events.addEventListener, location: { replace: (url) => navigation.push(`replace:${url}`), assign: (url) => navigation.push(url), reload: () => navigation.push("reload") } },
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
    localStorage: storage, sessionStorage: storage, TextEncoder, URLSearchParams,
    fetch: async (url, options) => {
      calls.push({ url, ...options });
      assert.equal(options.method, url === "/repository-options" ? "GET" : "POST");
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
    attach: (value, index = 0) => {
      nodes.get(`repository-${index}-path`).value = value;
      return nodes.get(`repository-${index}-form`).emit("submit", { preventDefault() {} });
    },
    create: (value) => {
      nodes.get("project-name").value = value;
      return nodes.get("project-form").emit("submit", { preventDefault() {} });
    },
  };
}

test("rename uses the saved project context, validates input and reloads only after confirmed save", async () => {
  const id = "ab".repeat(16);
  const current = page({form: false, projects: true, rename: id, initial: response(204, csrf)});
  await tick();
  for (const name of ["", " padded ", "line\nbreak", "é".repeat(65)]) {
    await current.create(name);
    assert.equal(current.calls.length, 1);
    assert.equal(current.focused(), "project-name");
  }
  current.queue.push(response(204));
  await current.create("工具 & <renamed>");
  assert.equal(current.calls.at(-1).url, `/projects/${id}/name`);
  assert.equal(current.calls.at(-1).body, "工具 & <renamed>");
  assert.equal(current.calls.at(-1).headers["x-csrf-token"], csrf);
  assert.deepEqual(current.navigation, ["reload"]);
  assert.equal(current.nodes.get("project-submit").disabled, true);
  const invalid = page({form: false, projects: true, rename: "../private", initial: response(204, csrf)});
  await tick(); await invalid.create("Denied fixture");
  assert.equal(invalid.calls.length, 1);
});

test("rename errors preserve the entered name and never claim success", async () => {
  for (const [failure, message] of [
    [response(400), /Check the project name/], [response(413), /Check the project name/],
    [response(401), /session ended/], [response(404), /Project unavailable/],
    [response(403), /could not be confirmed/], [response(503), /could not be confirmed/],
    [new Error("offline"), /could not be confirmed/],
  ]) {
    const current = page({form: false, projects: true, rename: "ab".repeat(16), initial: response(204, csrf)});
    await tick(); current.queue.push(failure); await current.create("Retained name");
    assert.equal(current.nodes.get("project-name").value, "Retained name");
    assert.match(current.nodes.get("project-status").textContent, message);
    assert.deepEqual(current.navigation, []);
    assert.equal(current.nodes.get("project-submit").disabled, failure.status === 401);
  }
});

test("rename disables duplicate saves and ignores late completion after navigation", async () => {
  const current = page({form: false, projects: true, rename: "ab".repeat(16), initial: response(204, csrf)});
  await tick(); let finish;
  current.queue.push(new Promise(resolve => {finish = resolve;}));
  const pending = current.create("Pending name");
  await tick(); await current.create("Duplicate name");
  assert.equal(current.calls.length, 2);
  assert.equal(current.nodes.get("project-submit").disabled, true);
  current.event("pagehide"); finish(response(204)); await pending;
  assert.deepEqual(current.navigation, []);
  assert.match(current.nodes.get("project-status").textContent, /Reload this page to check/);
  current.queue.push(response(204, csrf)); current.event("pageshow", {persisted: true}); await tick();
  assert.equal(current.nodes.get("project-submit").disabled, false);
  assert.match(current.nodes.get("project-status").textContent, /Reload this page to check/);
});

test("project creation waits for a session, validates bytes, and sends only the name with CSRF", async () => {
  let finish;
  const current = page({ form: false, projects: true, initial: new Promise((resolve) => { finish = resolve; }) });
  assert.equal(current.nodes.get("project-submit").disabled, true);
  await current.create("Blocked fixture");
  assert.equal(current.calls.length, 1);
  finish(response(204, csrf));
  await tick();
  for (const name of ["", " padded ", "line\nbreak", "é".repeat(65)]) {
    await current.create(name);
    assert.equal(current.calls.length, 1);
    assert.equal(current.focused(), "project-name");
    assert.equal(current.nodes.get("project-name").getAttribute("aria-invalid"), "true");
  }
  current.queue.push(response(204));
  await current.create("工具 & <fixture>");
  assert.equal(current.calls.at(-1).body, "工具 & <fixture>");
  assert.equal(current.calls.at(-1).headers["x-csrf-token"], csrf);
  assert.equal(current.calls.at(-1).headers["content-type"], "text/plain;charset=UTF-8");
  assert.deepEqual(current.navigation, ["/projects"]);
});

test("project creation failures retain input, report uncertainty and never navigate to success", async () => {
  for (const [failure, message] of [
    [response(400), /Check the project name/], [response(413), /Check the project name/],
    [response(409), /limit of 64/], [response(401), /session ended/],
    [response(403), /could not be confirmed/], [response(503), /could not be confirmed/],
    [new Error("private fixture error"), /could not be confirmed/],
  ]) {
    const current = page({ form: false, projects: true, initial: response(204, csrf) });
    await tick();
    failure.headers?.set("location", `/projects/${"ab".repeat(16)}`);
    current.queue.push(failure);
    await current.create("Retained fixture");
    assert.equal(current.nodes.get("project-name").value, "Retained fixture");
    assert.match(current.nodes.get("project-status").textContent, message);
    assert.deepEqual(current.navigation, []);
    assert.equal(current.nodes.get("project-submit").disabled, failure.status === 401);
  }
});

test("confirmed creation opens the returned project or falls back to the list", async () => {
  const target = `/projects/${"ab".repeat(16)}`;
  for (const location of [target, undefined, "", "/projects", "/projects/not-an-id", `${target}?next=1`, `https://example.invalid${target}`, `//example.invalid${target}`]) {
    const current = page({ form: false, projects: true, initial: response(204, csrf) });
    await tick();
    const saved = response(204);
    saved.headers.set("location", location);
    current.queue.push(saved);
    await current.create("Repeated project name");
    assert.deepEqual(current.navigation, [location === target ? target : "/projects"]);
    assert.match(current.nodes.get("project-status").textContent, /Project saved\. Opening/);
    assert.equal(current.nodes.get("project-submit").disabled, true);
  }
});

test("project requests cannot double-submit or navigate after pagehide", async () => {
  const current = page({ form: false, projects: true, initial: response(204, csrf) });
  await tick();
  let finish;
  current.queue.push(new Promise((resolve) => { finish = resolve; }));
  const pending = current.create("One fixture");
  await current.create("Second fixture");
  assert.equal(current.calls.length, 2);
  assert.equal(current.nodes.get("project-submit").disabled, true);
  current.event("pagehide");
  finish(response(204));
  await pending;
  assert.deepEqual(current.navigation, []);
});

test("confirmed history recovery clears only temporary repository session errors", async () => {
  for (const recovered of [response(204, nextCsrf), response(401)]) {
    const current = page({ form: false, repositories: true, initial: response(503) });
    await tick();
    assert.match(current.nodes.get("repository-0-status").textContent, /session could not be checked/);
    current.event("pagehide");
    current.queue.push(recovered);
    current.event("pageshow", { persisted: true });
    await tick();
    assert.equal(current.nodes.get("repository-0-status").textContent, "");
    assert.equal(current.nodes.get("repository-0-submit").disabled, recovered.status === 401);
  }
});

test("attachment feedback survives failed and successful history session checks", async () => {
  const current = page({ form: false, repositories: true, initial: response(204, csrf) });
  await tick();
  current.queue.push(response(409, null, null, "already-associated"));
  await current.attach("team/app");
  const feedback = current.nodes.get("repository-0-status").textContent;
  for (let attempt = 0; attempt < 2; attempt++) {
    current.event("pagehide");
    current.queue.push(response(503));
    current.event("pageshow", { persisted: true });
    await tick();
    assert.match(current.nodes.get("repository-0-status").textContent, /session could not be checked/);
  }
  current.event("pagehide");
  current.queue.push(response(204, nextCsrf));
  current.event("pageshow", { persisted: true });
  await tick();
  assert.equal(current.nodes.get("repository-0-status").textContent, feedback);
  assert.equal(current.nodes.get("repository-0-path").value, "team/app");
});

test("unconfigured preview performs no authentication requests", async () => {
  const current = page({ configured: false, form: false });
  await tick();
  assert.equal(current.calls.length, 0);
  assert.equal(current.nodes.get("profile-state").textContent, "Account");
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
  assert.equal(current.nodes.get("profile-state").textContent, "Account");
  assert.ok(current.signouts.every((button) => !button.disabled && !button.hidden));
  assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session"]);
});

test("only confirmed anonymous recovery enables sign-in; failed checks remain unconfirmed", async () => {
  for (const initial of [response(401), response(503), response(204, "invalid"), new Error("fixture")]) {
    const current = page({ initial });
    await tick();
    const unchecked = initial.status !== 401;
    assert.equal(current.nodes.get("signin-form").hidden, unchecked);
    assert.equal(current.nodes.get("auth-submit").disabled, unchecked);
    assert.equal(current.nodes.get("auth-recheck").hidden, !unchecked);
    assert.equal(current.signouts[0].hidden, true);
    assert.equal(current.nodes.get("profile-state").textContent, unchecked ? "Check account" : "Signed out");
    if (unchecked) assert.match(current.status.textContent, /could not be checked/);
  }
});

test("sign-in preserves password bytes and enters the workspace only after cookie confirmation", async () => {
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
  assert.deepEqual(current.navigation, ["replace:/"]);
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
    assert.deepEqual(current.navigation, []);
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
    assert.deepEqual(current.navigation, []);
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
  assert.deepEqual(current.navigation, []);
  assert.equal(current.nodes.get("signin-form").hidden, false);
  assert.equal(current.signouts[0].hidden, true);
});

test("failed verification after successful sign-in requires checking the account, not another password", async () => {
  for (const failure of [new Error("fixture network failure"), response(503), response(204, "invalid")]) {
    const current = page();
    await tick();
    current.queue.push(response(204, csrf), response(204, nextCsrf), failure);
    await current.submit();
    assert.equal(current.nodes.get("profile-state").textContent, "Check account");
    assert.equal(current.nodes.get("auth-password").value, "");
    assert.equal(current.nodes.get("auth-submit").disabled, true);
    assert.equal(current.nodes.get("auth-password").disabled, true);
    assert.equal(current.nodes.get("auth-recheck").hidden, false);
    assert.equal(current.focused(), "auth-recheck");
    assert.equal(current.signouts[0].hidden, true);
    assert.match(current.status.textContent, /could not be checked.*Check account/);
    assert.deepEqual(current.navigation, []);
    assert.doesNotMatch(current.status.textContent, /cookies|try signing in|private/);
    const count = current.calls.length;
    await current.submit();
    assert.equal(current.calls.length, count);

    // A fresh page checks the retained cookie without a challenge or sign-in.
    const reloaded = page({ initial: response(204, nextCsrf) });
    await tick();
    assert.equal(reloaded.nodes.get("profile-state").textContent, "Account");
    assert.deepEqual(reloaded.calls.map((call) => call.url), ["/auth/session"]);
  }
});

test("a failed reload still offers only account checking until the session is confirmed", async () => {
  for (const form of [true, false]) {
    const current = page({ form, initial: new Error("fixture network failure") });
    await tick();
    assert.equal(current.nodes.get("profile-account").textContent, "Check account");
    assert.equal(current.signouts[0].hidden, true);
    current.queue.push(response(204, nextCsrf));
    current.event("pageshow", { persisted: true });
    await tick();
    assert.equal(current.nodes.get("profile-state").textContent, "Account");
    assert.deepEqual(current.calls.map((call) => call.url), ["/auth/session", "/auth/session"]);
  }
});

test("verification failure after navigation cannot overwrite the restored session", async () => {
  const current = page();
  await tick();
  let finish;
  current.queue.push(response(204, csrf), response(204, nextCsrf), new Promise((resolve) => { finish = resolve; }));
  const work = current.submit();
  await tick();
  current.event("pagehide");
  current.queue.push(response(204, nextCsrf));
  current.event("pageshow", { persisted: true });
  await tick();
  finish(new Error("late fixture failure"));
  await work;
  assert.equal(current.nodes.get("profile-state").textContent, "Account");
  assert.equal(current.nodes.get("auth-recheck").hidden, true);
  assert.equal(current.status.textContent, "");
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
    assert.equal(current.nodes.get("profile-state").textContent, "Account");
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
  assert.notEqual(current.nodes.get("profile-state").textContent, "Account");
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


test("repository forms wait for a verified session and validate bounded root-relative paths", async () => {
  let finish;
  const current = page({ form: false, repositories: true, initial: new Promise((resolve) => { finish = resolve; }) });
  assert.equal(current.nodes.get("repository-0-submit").disabled, true);
  await current.attach("blocked");
  assert.equal(current.calls.length, 1);
  finish(response(204, csrf));
  await tick();
  for (const path of ["", "/absolute", "../outside", "team/./app", "team//app", "team/app/", "line\nbreak", "é".repeat(2049), Array(65).fill("a").join("/")]) {
    await current.attach(path);
    assert.equal(current.calls.length, 1);
    assert.equal(current.focused(), "repository-0-path");
    assert.equal(current.nodes.get("repository-0-path").getAttribute("aria-invalid"), "true");
  }
  current.queue.push(response(204));
  await current.attach("team/工具 & app", 1);
  assert.equal(current.calls.at(-1).url, `/projects/${"02".repeat(16)}/repositories`);
  assert.equal(current.calls.at(-1).body, "team/工具 & app");
  assert.equal(current.calls.at(-1).headers["x-csrf-token"], csrf);
  assert.equal(current.calls.at(-1).headers["content-type"], "text/plain;charset=UTF-8");
  assert.deepEqual(current.navigation, ["reload"]);
});

test("repository failures retain paths, distinguish conflicts, and never claim a save", async () => {
  for (const [failure, message] of [
    [response(400), /Check the relative path/], [response(413), /Check the relative path/],
    [response(409, null, null, "already-associated"), /already attached.*default checkout was not changed/],
    [response(409, null, null, "checkout-changed"), /checkout or trusted root changed/],
    [response(409, null, null, "catalog-full"), /catalog has reached/],
    [response(409, null, null, "<private>"), /conflicts with saved state/],
    [response(404), /Project unavailable/], [response(401), /session ended/],
    [response(503, null, null, "root-unconfigured"), /not configured/],
    [response(503, null, null, "unavailable"), /storage is unavailable.*could not be confirmed/],
    [response(403), /request was denied/], [response(503), /could not be confirmed/],
    [response(200), /could not be confirmed/], [new Error("private fixture error"), /could not be confirmed/],
  ]) {
    const current = page({ form: false, repositories: true, initial: response(204, csrf) });
    await tick();
    current.queue.push(failure);
    await current.attach("retained/checkout");
    const status = current.nodes.get("repository-0-status").textContent;
    assert.match(status, message);
    assert.doesNotMatch(status, /private|Repository attached/);
    assert.equal(current.nodes.get("repository-0-path").value, "retained/checkout");
    assert.deepEqual(current.navigation, []);
    assert.equal(current.nodes.get("repository-1-submit").disabled, failure.status === 401);
  }
});

test("attachment shares the mutation gate and ignores responses after pagehide", async () => {
  const current = page({ form: false, projects: true, repositories: true, initial: response(204, csrf) });
  await tick();
  let finish;
  current.queue.push(new Promise((resolve) => { finish = resolve; }));
  const pending = current.attach("first");
  await current.attach("second", 1);
  await current.create("blocked project");
  await current.logout();
  assert.equal(current.calls.length, 2);
  assert.equal(current.nodes.get("repository-1-submit").disabled, true);
  current.event("pagehide");
  finish(response(204));
  await pending;
  assert.deepEqual(current.navigation, []);
  assert.match(current.nodes.get("repository-0-status").textContent, /Reload.*whether.*completed/);
  current.queue.push(response(204, nextCsrf));
  current.event("pageshow", { persisted: true });
  await tick();
  assert.equal(current.nodes.get("repository-0-submit").disabled, false);
  assert.match(current.nodes.get("repository-0-status").textContent, /Reload.*whether.*completed/);
});

test("failed session recovery and malformed project context cannot submit a checkout", async () => {
  const unchecked = page({ form: false, repositories: true, initial: response(503) });
  await tick();
  await unchecked.attach("team/app");
  assert.equal(unchecked.calls.length, 1);
  assert.equal(unchecked.nodes.get("repository-0-submit").disabled, true);
  assert.match(unchecked.nodes.get("repository-0-status").textContent, /session could not be checked/);
  const invalid = page({ form: false, repositories: true, initial: response(204, csrf) });
  await tick();
  invalid.nodes.get("repository-0-form").dataset.project = "../private";
  await invalid.attach("team/app");
  assert.equal(invalid.calls.length, 1);
  assert.match(invalid.nodes.get("repository-0-status").textContent, /Project unavailable/);
});

test("checkout picker loads on dialog opening, fills the existing path, and preserves manual entry", async () => {
  const current = page({form: false, repositories: true, picker: true, initial: response(204, csrf)});
  await tick();
  const choice = current.nodes.get('repository-0-choice');
  const field = current.nodes.get('repository-0-path');
  assert.equal(choice.disabled, true);
  assert.equal(current.calls.length, 1);
  let finish;
  current.queue.push(new Promise(resolve => {finish = resolve;}));
  const disclosure = current.nodes.get('repository-0-disclosure');
  disclosure.open = true; disclosure.emit('dialogopened');
  assert.match(current.nodes.get('repository-0-options-status').textContent, /Loading/);
  assert.equal(current.nodes.get('repository-0-refresh').disabled, true);
  assert.equal(field.disabled, false);
  finish({...response(200), text: async () => 'app\n工具 & <fixture>'});
  await tick();
  assert.equal(choice.disabled, false);
  assert.deepEqual(choice.children.map(option => option.textContent), ['Choose a checkout', 'app', '工具 & <fixture>']);
  choice.value = '工具 & <fixture>'; choice.emit('change');
  assert.equal(field.value, '工具 & <fixture>');
  field.value = 'nested/manual'; field.emit('input');
  assert.equal(choice.value, '');
  assert.equal(field.value, 'nested/manual');
  assert.equal(current.calls.at(-1).url, '/repository-options');
  assert.equal(current.calls.at(-1).headers, undefined);
});

test("checkout picker empty, partial and failed lists have truthful recoverable feedback", async () => {
  const partial = {...response(200), text: async () => 'app'};
  partial.headers.set('x-devoption-list-limited', 'true');
  for (const [result, expected, enabled] of [
    [{...response(200), text: async () => ''}, /No supported checkouts/, false],
    [partial, /first 64 entries/, true],
    [response(503), /could not be loaded.*Refresh/, false],
    [new Error('private error'), /could not be loaded/, false],
    [{...response(200), text: async () => '../escape'}, /could not be loaded/, false],
    [{...response(200), text: async () => Array(65).fill('app').join('\n')}, /could not be loaded/, false],
  ]) {
    const current = page({form:false, repositories:true, picker:true, initial:response(204,csrf)});
    await tick();
    current.nodes.get('repository-0-path').value = 'preserved';
    current.queue.push(result);
    await current.nodes.get('repository-0-refresh').emit('click');
    assert.match(current.nodes.get('repository-0-options-status').textContent, expected);
    assert.equal(current.nodes.get('repository-0-choice').disabled, !enabled);
    assert.equal(current.nodes.get('repository-0-refresh').disabled, false);
    assert.equal(current.nodes.get('repository-0-path').value, 'preserved');
    current.queue.push({...response(200), text: async () => 'recovered'});
    await current.nodes.get('repository-0-refresh').emit('click');
    assert.equal(current.nodes.get('repository-0-choice').disabled, false);
  }
});

test("checkout picker discards delayed results after leaving the page", async () => {
  const current = page({form:false, repositories:true, picker:true, initial:response(204,csrf)});
  await tick();
  let finish;
  current.queue.push(new Promise(resolve => {finish = resolve;}));
  const pending = current.nodes.get('repository-0-refresh').emit('click');
  current.event('pagehide');
  finish({...response(200), text: async () => 'stale'});
  await pending;
  assert.equal(current.nodes.get('repository-0-choice').disabled, true);
  assert.deepEqual(current.nodes.get('repository-0-choice').children, []);
  assert.doesNotMatch(current.nodes.get('repository-0-options-status').textContent, /Loading/);
});

test("dedicated auth page gates loading and signs in and out without profile elements", async () => {
  let checked, challenged;
  const current = page({profile: false, initial: new Promise(resolve => { checked = resolve; })});
  assert.equal(current.nodes.get("auth-submit").textContent, "Checking…");
  assert.equal(current.nodes.get("signin-form").getAttribute("aria-busy"), "true");
  checked(response(401)); await tick();
  assert.equal(current.nodes.get("auth-submit").textContent, "Sign in");
  assert.equal(current.nodes.get("signin-form").getAttribute("aria-busy"), "false");
  current.queue.push(new Promise(resolve => { challenged = resolve; }), response(204), response(204, nextCsrf));
  const signingIn = current.submit();
  assert.equal(current.nodes.get("auth-submit").textContent, "Signing in…");
  assert.equal(current.nodes.get("auth-password").value, "");
  await current.submit(); assert.equal(current.calls.length, 2);
  challenged(response(204, csrf)); await signingIn;
  assert.equal(current.nodes.get("page-title").textContent, "Account");
  assert.equal(current.nodes.get("auth-signed-in").hidden, false);
  current.queue.push(response(204)); await current.logout();
  assert.equal(current.nodes.get("signin-form").hidden, false);
  assert.equal(current.focused(), "auth-password");
  assert.equal(current.status.textContent, "Signed out.");
});

test("dedicated recovery failure hides the password form until a confirmed session result", async () => {
  const current = page({profile: false, initial: response(503)}); await tick();
  assert.equal(current.nodes.get("page-title").textContent, "Check account");
  assert.equal(current.nodes.get("auth-recheck").hidden, false);
  assert.equal(current.nodes.get("auth-password").disabled, true);
  await current.submit(); assert.equal(current.calls.length, 1);
  current.queue.push(response(204, csrf)); current.event("pageshow", {persisted: true}); await tick();
  assert.equal(current.nodes.get("auth-signed-in").hidden, false);
  assert.equal(current.nodes.get("auth-recheck").hidden, true);
  assert.deepEqual(current.calls.map(call => call.url), ["/auth/session", "/auth/session"]);
});

test("a formerly verified auth page explains an ended session after history recovery", async () => {
  const current = page({profile: false, initial: response(204, csrf)}); await tick();
  current.nodes.get("auth-password").value = "fixture";
  current.event("pagehide"); current.queue.push(response(401));
  current.event("pageshow", {persisted: true}); await tick();
  assert.equal(current.nodes.get("auth-password").value, "");
  assert.equal(current.nodes.get("signin-form").hidden, false);
  assert.equal(current.nodes.get("auth-submit").disabled, false);
  assert.match(current.status.textContent, /Your session ended/);
  assert.equal(current.nodes.get("auth-signed-in").hidden, true);
});

test("dedicated CLI setup page makes no authentication requests without form or profile", async () => {
  const current = page({configured: false, form: false, profile: false}); await tick();
  assert.deepEqual(current.calls, []);
  assert.equal(current.signouts.length, 0);
});


test("navigation waits for verification, ignores duplicate submits and discards a late success", async () => {
  for (const leave of [false, true]) {
    const current = page();
    await tick();
    let finish;
    current.queue.push(response(204, csrf), response(204, nextCsrf), new Promise(resolve => { finish = resolve; }));
    const pending = current.submit();
    await tick();
    assert.deepEqual(current.navigation, []);
    await current.submit("duplicate");
    assert.equal(current.calls.length, 4);
    if (leave) current.event("pagehide");
    finish(response(204, nextCsrf));
    await pending;
    assert.deepEqual(current.navigation, leave ? [] : ["replace:/"]);
  }
});

test("an already authenticated account visit stays available, and an expired session can sign in again", async () => {
  const current = page({ initial: response(204, csrf) });
  await tick();
  assert.deepEqual(current.navigation, []);
  assert.equal(current.nodes.get("auth-signed-in").hidden, false);
  current.event("pagehide");
  current.queue.push(response(401));
  current.event("pageshow", {persisted: true});
  await tick();
  assert.match(current.status.textContent, /session ended/);
  current.queue.push(response(204, csrf), response(204, nextCsrf), response(204, nextCsrf));
  await current.submit();
  assert.deepEqual(current.navigation, ["replace:/"]);
});


test("member sign-in binds the submitted email and password while waiting for a challenge", async () => {
  const current = page();
  await tick();
  current.nodes.get("auth-email").value = "member@example.test";
  let release;
  current.queue.push(new Promise((resolve) => { release = resolve; }), response(204, csrf), response(204, csrf));
  const submitted = current.submit("public member fixture password");
  assert.equal(current.nodes.get("auth-email").disabled, true);
  current.nodes.get("auth-email").value = "changed@example.test";
  release(response(204, csrf));
  await submitted;
  const request = current.calls.find((call) => call.url === "/auth/user-sign-in");
  assert.equal(request.headers["content-type"], "text/plain;charset=UTF-8");
  assert.deepEqual(JSON.parse(request.body), { email: "member@example.test", password: "public member fixture password" });
  assert.deepEqual(current.navigation, ["replace:/"]);
});
