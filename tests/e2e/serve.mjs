// Test harness started by the e2e runner (see e2e.config.ts):
//   1. launches the PrintBridge agent on AGENT_PORT, locked to this server's origin;
//   2. serves the repository root over HTTP so /demo/ and /sdk/ load like in real use.
// Usage: node serve.mjs <http-port>
import { spawn } from 'node:child_process';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..');
const httpPort = Number(process.argv[2] ?? 8080);
const agentPort = Number(process.env.AGENT_PORT ?? 9651);
const exe = process.platform === 'win32' ? 'printbridge.exe' : 'printbridge';
const agentBin = process.env.PRINTBRIDGE_BIN ?? join(repoRoot, 'target', 'debug', exe);

if (!existsSync(agentBin)) {
  console.error(`PrintBridge binary not found at ${agentBin} — run "cargo build" first.`);
  process.exit(1);
}

const agent = spawn(agentBin, [
  '--port', String(agentPort),
  '--allowed-origins', `http://127.0.0.1:${httpPort}`,
  '--mdns-scan-seconds', '1',
  '--disable-port-scan',
], { stdio: 'inherit' });
agent.on('exit', (code) => {
  console.error(`PrintBridge agent exited (${code})`);
  process.exit(1);
});
const stop = () => { agent.kill(); process.exit(0); };
process.on('SIGINT', stop);
process.on('SIGTERM', stop);

// Wait for the agent before serving, so the first page load finds it.
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`http://127.0.0.1:${agentPort}/api/health`)).ok) break;
  } catch {}
  if (i > 100) { console.error('agent did not start'); stop(); }
  await new Promise((r) => setTimeout(r, 100));
}

const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css' };
createServer((req, res) => {
  let path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname));
  let file = join(repoRoot, path);
  if (!file.startsWith(repoRoot)) { res.writeHead(403).end(); return; }
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
  if (!existsSync(file)) { res.writeHead(404).end('not found'); return; }
  res.writeHead(200, { 'Content-Type': types[extname(file)] ?? 'application/octet-stream' });
  createReadStream(file).pipe(res);
}).listen(httpPort, '127.0.0.1', () => {
  console.log(`serving ${repoRoot} on http://127.0.0.1:${httpPort}, agent on ${agentPort}`);
});
