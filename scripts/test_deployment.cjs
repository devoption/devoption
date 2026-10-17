const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('crates/devoption-ui/assets/deployment.js', 'utf8');
function fixture(action = 'approve') {
  const handlers = {}, events = {}, calls = [], queue = [], navigation = [];
  const status = {};
  const button = { dataset: {deployAction: action, value: 'ab'.repeat(16)}, addEventListener: (name, cb) => { handlers[name] = cb; } };
  const root = {dataset: {base: '/projects/' + '01'.repeat(16) + '/releases'}, querySelectorAll: () => [button], querySelector: () => ({value: "ok A"})};
  const document = {getElementById: id => id === 'app-deployment' ? root : status, addEventListener: (_, cb) => cb()};
  const window = {addEventListener: (name, cb) => {events[name] = cb;}, location: {reload: () => navigation.push('reload'), assign: value => navigation.push(value)}};
  const fetch = async (url, options) => { calls.push({url, ...options}); const response = await queue.shift(); if (response instanceof Error) throw response; return response; };
  vm.runInNewContext(source, {document, window, fetch});
  return {button, status, calls, queue, navigation, events, click: () => handlers.click()};
}
const response = (status, csrf = 'cd'.repeat(32)) => ({status, headers: {get: () => csrf}});
test('approval uses a fresh session and exact one-use review token', async () => {
  const p = fixture(); p.queue.push(response(204), response(204)); await p.click();
  assert.equal(p.calls.length, 2); assert.equal(p.calls[0].url, '/auth/session');
  assert.equal(p.calls[1].body, 'ab'.repeat(16)); assert.equal(p.calls[1].headers['x-csrf-token'], 'cd'.repeat(32));
  assert.equal(p.calls[1].method, 'POST'); assert.equal(p.calls[1].redirect, 'error');
  assert.equal(p.navigation[0], 'reload');
});
test('missing or invalid session prevents any build action', async () => {
  for (const session of [response(401), response(204, 'invalid'), new Error('network')]) {
    const p = fixture(); p.queue.push(session); await p.click();
    assert.equal(p.calls.length, 1); assert.equal(p.navigation.length, 0);
  }
});
test('ambiguous approval is never automatically retried', async () => {
  const p = fixture(); p.queue.push(response(204), new Error('network')); await p.click();
  assert.equal(p.calls.length, 2); assert.equal(p.navigation.length, 0);
  assert.match(p.status.textContent, /logs before retrying/);
});
test('double click while pending cannot duplicate approval; pagehide ignores late success', async () => {
  const p = fixture(); let finish;
  p.queue.push(response(204), new Promise(resolve => {finish = resolve;}));
  const pending = p.click(); await new Promise(resolve => setImmediate(resolve));
  await p.click(); assert.equal(p.calls.length, 2);
  p.events.pagehide(); finish(response(204)); await pending;
  assert.equal(p.navigation.length, 0); assert.equal(p.button.disabled, true);
  p.events.pageshow({persisted:true}); assert.deepEqual(p.navigation, ['reload']);
});
test('review binds release and exact health expectation; stop has no payload', async () => {
  const p = fixture('prepare'); p.button.dataset.value = 'ab'.repeat(32); p.queue.push(response(204),response(204)); await p.click();
  assert.equal(p.calls[1].body, 'ab'.repeat(32) + '\nok A');
  const stop = fixture('stop'); stop.button.dataset.value = '';
  stop.queue.push(response(204),response(204)); await stop.click();
  assert.equal(stop.calls[1].body, '');
});
