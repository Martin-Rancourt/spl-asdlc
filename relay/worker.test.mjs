import { test } from 'node:test';
import assert from 'node:assert/strict';
import { webcrypto } from 'node:crypto';
import worker, { WakeQueue, verify } from './worker.mjs';
globalThis.crypto ??= webcrypto;
const enc = new TextEncoder();
async function signed(body, secret = 'test-webhook-secret') {
  const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return 'sha256=' + Buffer.from(await crypto.subtle.sign('HMAC', key, enc.encode(body))).toString('hex');
}
function fixture() {
  const values = new Map();
  const storage = { get: async k => values.get(k), put: async (k, v) => values.set(k, v), delete: async k => values.delete(k), transaction: async fn => fn(storage) };
  const object = new WakeQueue({ storage });
  const env = { REPOSITORIES: 'example-org/acme', WEBHOOK_SECRET: 'test-webhook-secret', PULL_TOKEN: 'test-pull-token', QUEUE: { idFromName: n => n, get: () => object } };
  const get = (path, token = env.PULL_TOKEN) => worker.fetch(new Request('https://relay.example.org' + path, { headers: { Authorization: `Bearer ${token}` } }), env);
  const post = async (payload, kind = 'push', signature) => {
    const body = JSON.stringify(payload);
    return worker.fetch(new Request('https://relay.example.org/webhook', { method: 'POST', body, headers: { 'X-GitHub-Event': kind, 'X-Hub-Signature-256': signature ?? await signed(body) } }), env);
  };
  return { storage, env, get, post };
}

test('signature is bound to raw bytes and secret', async () => {
  const sig = await signed('body');
  assert(await verify(enc.encode('body'), sig, 'test-webhook-secret'));
  assert(!await verify(enc.encode('Body'), sig, 'test-webhook-secret'));
  assert(!await verify(enc.encode('body'), sig, 'wrong'));
  assert(!await verify(enc.encode('body'), null, 'test-webhook-secret'));
});
test('signed events coalesce, retain only hints, and pull consumes', async () => {
  const f = fixture();
  for (let i = 0; i < 3; i++) assert.equal((await f.post({ repository: { full_name: 'Example-Org/Acme' }, secret_payload: 'discard' })).status, 202);
  assert.equal((await f.get('/health')).status, 200);
  const hints = await (await f.get('/events?repository=example-org/acme')).json();
  assert.deepEqual(hints, [{ repository: 'example-org/acme', kind: 'push' }]);
  assert.deepEqual(await (await f.get('/events?repository=example-org/acme')).json(), []);
});
test('rejects unsigned webhooks and unauthorized or foreign pulls', async () => {
  const f = fixture();
  assert.equal((await f.post({}, 'push', 'bad')).status, 401);
  assert.equal((await f.get('/events?repository=example-org/acme', 'bad')).status, 401);
  assert.equal((await f.get('/health', 'bad')).status, 401);
  assert.equal((await f.get('/health?repository=foreign/repo')).status, 403);
  assert.equal((await f.get('/events?repository=foreign/repo')).status, 403);
  await f.post({ repository: { full_name: 'foreign/repo' } });
  await f.post({ repository: { full_name: 'example-org/acme' } }, 'unknown');
  assert.deepEqual(await (await f.get('/events?repository=example-org/acme')).json(), []);
});
test('long poll wakes immediately when a signed event arrives', async () => {
  const f = fixture();
  const pending = f.get('/events?repository=example-org/acme&wait=25');
  await new Promise(resolve => setTimeout(resolve, 20));
  await f.post({ repository: { full_name: 'example-org/acme' } });
  assert.equal((await (await pending).json()).length, 1);
});
test('pending hint survives object recreation', async () => {
  const f = fixture();
  await f.post({ repository: { full_name: 'example-org/acme' } });
  const restarted = new WakeQueue({ storage: f.storage });
  assert.equal((await (await restarted.fetch(new Request('https://queue.internal/events?wait=0'))).json()).length, 1);
});
test('health is read only and oversized bodies are rejected', async () => {
  const f = fixture();
  assert.equal((await f.post({ padding: 'x'.repeat(1024 * 1024) })).status, 413);
  assert.deepEqual(await (await f.get('/events?repository=example-org/acme')).json(), []);
});
