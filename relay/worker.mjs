// One deployment per bridge installation. Only this Worker can address its objects.
const encoder = new TextEncoder();
const repoPattern = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const kinds = new Set(['push', 'pull_request', 'pull_request_review', 'pull_request_review_thread', 'check_run', 'check_suite', 'workflow_run', 'release', 'create', 'delete', 'issues', 'issue_comment', 'status']);
const reply = (body, status = 200) => new Response(body, { status, headers: { 'Cache-Control': 'no-store' } });

async function authorized(request, token) {
  if (!token) return false;
  const key = await crypto.subtle.importKey('raw', encoder.encode(token), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']);
  const expected = await crypto.subtle.sign('HMAC', key, encoder.encode('queue-access'));
  const header = request.headers.get('Authorization') ?? '';
  if (!header.startsWith('Bearer ')) return false;
  const supplied = header.slice(7);
  const candidate = await crypto.subtle.importKey('raw', encoder.encode(supplied || 'missing'), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']);
  return crypto.subtle.verify('HMAC', candidate, expected, encoder.encode('queue-access'));
}

export async function verify(body, signature, secret) {
  if (!secret || !/^sha256=[0-9a-f]{64}$/.test(signature ?? '')) return false;
  const key = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']);
  const bytes = Uint8Array.from(signature.slice(7).match(/../g), x => parseInt(x, 16));
  return crypto.subtle.verify('HMAC', key, bytes, body);
}

// Stream with a hard cap, including requests that omit Content-Length.
async function boundedBody(request) {
  const reader = request.body?.getReader();
  if (!reader) return new Uint8Array();
  const chunks = []; let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > 1024 * 1024) { await reader.cancel(); throw new Error('too large'); }
    chunks.push(value);
  }
  const body = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) { body.set(chunk, offset); offset += chunk.length; }
  return body;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const allowed = new Set((env.REPOSITORIES ?? '').split(',').map(s => s.trim().toLowerCase()).filter(s => repoPattern.test(s)));
    if (request.method === 'POST' && url.pathname === '/webhook') {
      let body;
      try { body = await boundedBody(request); } catch { return reply('too large', 413); }
      if (!await verify(body, request.headers.get('X-Hub-Signature-256'), env.WEBHOOK_SECRET)) return reply('unauthorized', 401);
      let event;
      try { event = JSON.parse(new TextDecoder().decode(body)); } catch { return reply('invalid JSON', 400); }
      const kind = request.headers.get('X-GitHub-Event');
      const repository = event.repository?.full_name?.toLowerCase();
      if (!kinds.has(kind) || !allowed.has(repository)) return reply('ignored', 202);
      const object = env.QUEUE.get(env.QUEUE.idFromName(repository));
      return object.fetch(new Request('https://queue.internal/enqueue', { method: 'POST', body: JSON.stringify({ repository, kind }) }));
    }
    if (request.method !== 'GET' || !['/events', '/health'].includes(url.pathname)) return reply('not found', 404);
    if (!await authorized(request, env.PULL_TOKEN)) return reply('unauthorized', 401);
    if (url.pathname === '/health') {
      const repository = (url.searchParams.get('repository') ?? allowed.values().next().value ?? '').toLowerCase();
      if (!allowed.has(repository)) return reply('repository not configured', 403);
      return env.QUEUE.get(env.QUEUE.idFromName(repository)).fetch(new Request('https://queue.internal/health'));
    }
    const repository = (url.searchParams.get('repository') ?? '').toLowerCase();
    if (!allowed.has(repository)) return reply('repository not configured', 403);
    const object = env.QUEUE.get(env.QUEUE.idFromName(repository));
    return object.fetch(new Request(`https://queue.internal/events?wait=${Math.min(25, Math.max(0, Number(url.searchParams.get('wait')) || 0))}`));
  }
};

// Coalesce all events for a repository into one durable wake hint. A tick re-reads
// all facts, so there is no benefit in retaining a payload or every delivery.
export class WakeQueue {
  constructor(ctx) { this.ctx = ctx; this.waiters = new Set(); }
  async take() {
    return this.ctx.storage.transaction(async storage => {
      const event = await storage.get('pending');
      if (event) await storage.delete('pending');
      return event ? [event] : [];
    });
  }
  async fetch(request) {
    if (new URL(request.url).pathname === '/health') {
      await this.ctx.storage.get('pending');
      return reply('ok');
    }
    if (request.method === 'POST') {
      await this.ctx.storage.put('pending', await request.json());
      for (const notify of this.waiters) notify();
      return reply('queued', 202);
    }
    // Register before the first read: an enqueue during that read cannot be lost.
    let notify;
    const signal = new Promise(resolve => { notify = resolve; });
    this.waiters.add(notify);
    let timer;
    try {
      let events = await this.take();
      if (!events.length) {
        const seconds = Number(new URL(request.url).searchParams.get('wait')) || 0;
        await Promise.race([signal, new Promise(resolve => { timer = setTimeout(resolve, seconds * 1000); })]);
        events = await this.take();
      }
      return reply(JSON.stringify(events));
    } finally { clearTimeout(timer); this.waiters.delete(notify); }
  }
}
