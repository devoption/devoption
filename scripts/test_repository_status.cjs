const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {join} = require('node:path');
const {test} = require('node:test');
const {runInNewContext} = require('node:vm');
const script = readFileSync(join(__dirname, '../crates/devoption-ui/assets/repository-status.js'), 'utf8');
const tick = () => new Promise(resolve => setImmediate(resolve));
const response = (body = 'main\n' + 'a'.repeat(40) + '\n0\n0\n0\n0', status = 200, code = '') => ({status, headers: new Map([['x-devoption-error', code]]), text: async () => body});
function page() {
  function element() {
    const listeners = new Map();
    return {hidden: true, disabled: true, textContent: '', addEventListener: (name, fn) => listeners.set(name, fn), emit: (name, event = {}) => listeners.get(name)?.(event)};
  }
  const nodes = Object.fromEntries(['refresh', 'fields', 'note', 'branch', 'commit', 'changes'].map(name => [name, element()]));
  const view = {...element(), open: false, dataset: {statusUrl: '/projects/fixture/repositories/fixture/status'}, querySelector: selector => nodes[selector.slice(13, -1)]};
  const events = element(), queue = [], calls = [];
  let ready;
  runInNewContext(script, {
    document: {addEventListener: (_, fn) => ready = fn, querySelectorAll: () => [view]},
    window: {addEventListener: events.addEventListener},
    fetch: async (url, options) => {
      calls.push(url); assert.equal(url, view.dataset.statusUrl);
      assert.equal(options.method, 'GET'); assert.equal(options.credentials, 'same-origin');
      assert.equal(options.cache, 'no-store'); assert.equal(options.redirect, 'error');
      assert.equal(options.referrerPolicy, 'no-referrer');
      assert.ok(queue.length, 'unexpected status request');
      const next = await queue.shift(); if (next instanceof Error) throw next; return next;
    },
  });
  ready();
  return {nodes, queue, calls, view, event: events.emit, open: () => {view.open = true; view.emit('toggle');}, refresh: () => nodes.refresh.emit('click')};
}

test('status is lazy, has a loading state, and refresh replaces observations', async () => {
  const p = page(); assert.equal(p.calls.length, 0);
  let release; p.queue.push(new Promise(resolve => release = resolve)); p.open();
  assert.equal(p.nodes.fields.hidden, true); assert.equal(p.nodes.refresh.disabled, true);
  assert.match(p.nodes.note.textContent, /Reading/); p.refresh(); assert.equal(p.calls.length, 1);
  release(response()); await tick();
  assert.equal(p.nodes.fields.hidden, false); assert.equal(p.nodes.changes.textContent, 'Clean');
  assert.equal(p.nodes.branch.textContent, 'main'); assert.equal(p.nodes.commit.textContent, 'a'.repeat(40));
  p.queue.push(response('feature/<literal>\n\n1\n2\n3\n4')); await p.refresh();
  assert.equal(p.nodes.branch.textContent, 'feature/<literal>'); assert.equal(p.nodes.commit.textContent, 'No commits yet');
  assert.equal(p.nodes.changes.textContent, '1 staged · 2 modified · 3 untracked entries · 4 conflicted');
  p.queue.push(response('\n' + 'b'.repeat(40) + '\n0\n0\n0\n0')); await p.refresh();
  assert.equal(p.nodes.branch.textContent, 'Detached HEAD');
});

test('failed and malformed reads hide a prior clean result and allow retry', async () => {
  const p = page(); p.queue.push(response()); p.open(); await tick();
  for (const [next, message] of [
    [response('', 409, 'changed'), /checkout changed/], [response('', 503, 'unsupported'), /layout or configuration/],
    [response('', 503, 'limited'), /read limit/], [response('', 401), /could not be read/],
    [new Error('network'), /could not be read/], [response('main\n'), /could not be read/],
    [response('main\nnot-a-commit\n0\n0\n0\n0'), /could not be read/],
    [response('main\n\n1025\n0\n0\n0'), /could not be read/],
    [response('\n\n0\n0\n0\n0'), /could not be read/],
  ]) {
    p.queue.push(next); await p.refresh(); assert.equal(p.nodes.fields.hidden, true);
    assert.equal(p.nodes.refresh.disabled, false); assert.match(p.nodes.note.textContent, message);
  }
  p.queue.push(response()); await p.refresh(); assert.equal(p.nodes.fields.hidden, false);
});

test('leaving discards delayed status, and back-forward restoration rereads an open view', async () => {
  const p = page(); let release; p.queue.push(new Promise(resolve => release = resolve)); p.open();
  p.event('pagehide'); release(response()); await tick();
  assert.equal(p.nodes.fields.hidden, true); assert.equal(p.nodes.refresh.disabled, true);
  p.queue.push(response('main\n\n0\n1\n0\n0')); p.event('pageshow', {persisted: true}); await tick();
  assert.equal(p.nodes.fields.hidden, false); assert.equal(p.nodes.changes.textContent, '1 modified');
});
