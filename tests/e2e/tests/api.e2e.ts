import { request } from 'node:http';
import { test, expect } from 'e2e';

const AGENT_PORT = Number(process.env.AGENT_PORT ?? 9651);
const AGENT = `http://127.0.0.1:${AGENT_PORT}`;

/** Raw HTTP request, so tests can send headers fetch() forbids (Host, Origin). */
function raw(
  path: string,
  { method = 'GET', headers = {} as Record<string, string> } = {},
): Promise<{ status: number; headers: Record<string, string | string[] | undefined>; firstFrame?: string }> {
  return new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port: AGENT_PORT, path, method, headers });
    req.on('response', (res) => {
      res.resume();
      resolve({ status: res.statusCode ?? 0, headers: res.headers });
    });
    req.on('upgrade', (res, socket) => {
      socket.once('data', (buf: Buffer) => {
        // Unmasked server text frame: 0x81, length byte (126 => 16-bit length follows).
        const len = buf[1] & 0x7f;
        const offset = len === 126 ? 4 : len === 127 ? 10 : 2;
        resolve({ status: res.statusCode ?? 0, headers: res.headers, firstFrame: buf.subarray(offset).toString('utf8') });
        socket.destroy();
      });
    });
    req.on('error', reject);
    req.end();
  });
}

const wsHeaders = (origin: string) => ({
  Connection: 'Upgrade',
  Upgrade: 'websocket',
  'Sec-WebSocket-Version': '13',
  'Sec-WebSocket-Key': 'dGhlIHNhbXBsZSBub25jZQ==',
  Origin: origin,
});

test('GET /api/health identifies the agent', async () => {
  const res = await fetch(`${AGENT}/api/health`);
  expect(res.status).toBe(200);
  const body = await res.json();
  expect(body).toMatchObject({ ok: true, name: 'PrintBridge' });
  expect(typeof body.version).toBe('string');
  expect(['macos', 'windows', 'linux']).toContain(body.platform);
});

test('GET /api/printers returns a list of printers', async () => {
  const res = await fetch(`${AGENT}/api/printers`);
  expect(res.status).toBe(200);
  const printers = await res.json();
  expect(Array.isArray(printers)).toBe(true);
  for (const p of printers) {
    expect(typeof p.id).toBe('string');
    expect(['local', 'mdns', 'portScan']).toContain(p.source);
  }
});

test('rejects DNS-rebinding requests with a foreign Host header', async () => {
  const res = await raw('/api/health', { headers: { Host: `evil.example.com:${AGENT_PORT}` } });
  expect(res.status).toBe(403);
});

test('CORS allows the configured origin, including Private Network Access', async ({ app }) => {
  const res = await raw('/api/print', {
    method: 'OPTIONS',
    headers: {
      Origin: app.baseUrl,
      'Access-Control-Request-Method': 'POST',
      'Access-Control-Request-Headers': 'content-type',
      'Access-Control-Request-Private-Network': 'true',
    },
  });
  expect(res.headers['access-control-allow-origin']).toBe(app.baseUrl);
  expect(res.headers['access-control-allow-private-network']).toBe('true');
});

test('CORS withholds access from other origins', async () => {
  const res = await raw('/api/print', {
    method: 'OPTIONS',
    headers: { Origin: 'https://evil.example.com', 'Access-Control-Request-Method': 'POST' },
  });
  expect(res.headers['access-control-allow-origin']).toBeUndefined();
});

test('WebSocket pushes a printer snapshot to an allowed origin', async ({ app }) => {
  const res = await raw('/ws', { headers: wsHeaders(app.baseUrl) });
  expect(res.status).toBe(101);
  const msg = JSON.parse(res.firstFrame ?? '{}');
  expect(msg.type).toBe('printers');
  expect(Array.isArray(msg.printers)).toBe(true);
});

test('WebSocket refuses other origins (browsers skip CORS for sockets)', async () => {
  const res = await raw('/ws', { headers: wsHeaders('https://evil.example.com') });
  expect(res.status).toBe(403);
});

test('POST /api/print reports an unknown printer as a 400 with a message', async () => {
  const res = await fetch(`${AGENT}/api/print`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ printerId: 'local:does-not-exist', text: 'hello' }),
  });
  expect(res.status).toBe(400);
  const body = await res.json();
  expect(body).toMatchObject({ ok: false, printerId: 'local:does-not-exist', via: 'none' });
  expect(body.message).toContain('Unknown printer id');
});

test('POST /api/print rejects a body without printerId as JSON', async () => {
  const res = await fetch(`${AGENT}/api/print`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text: 'hello' }),
  });
  expect(res.status).toBe(400);
  const body = await res.json();
  expect(body.ok).toBe(false);
  expect(body.message).toContain('printerId');
});
