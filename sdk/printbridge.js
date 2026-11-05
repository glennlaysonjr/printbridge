/**
 * PrintBridge JS SDK
 * ------------------
 * Browser client for the PrintBridge local agent (the Rust binary built from this repo).
 * Zero dependencies. Ships as an ES module; a <script>-tag build is in printbridge.umd.js.
 *
 * Usage:
 *   import { PrintBridge } from './printbridge.js';
 *
 *   const bridge = new PrintBridge();
 *   if (!(await bridge.connect())) {
 *     // agent not running — prompt the user to install/start it
 *   }
 *   const printers = await bridge.getPrinters();
 *   await bridge.print({ printerId: printers[0].id, text: 'Hello\n' });
 */

const DEFAULT_PORT = 9631;

export class PrintBridgeError extends Error {
  constructor(message, code) {
    super(message);
    this.name = 'PrintBridgeError';
    this.code = code; // 'AGENT_UNREACHABLE' | 'PRINT_FAILED' | 'BAD_REQUEST'
  }
}

export class PrintBridge {
  /**
   * @param {object} [options]
   * @param {number} [options.port=9631]  Port the local agent listens on.
   * @param {number} [options.timeoutMs=5000]  Per-request timeout (discovery calls get longer).
   */
  constructor({ port = DEFAULT_PORT, timeoutMs = 5000 } = {}) {
    this.baseUrl = `http://127.0.0.1:${port}`;
    this.wsUrl = `ws://127.0.0.1:${port}/ws`;
    this.timeoutMs = timeoutMs;
    this.connected = false;
    this._ws = null;
    this._listeners = new Map(); // event -> Set<fn>
  }

  /**
   * Checks whether the local agent is running.
   * @returns {Promise<boolean>} true when reachable — never throws.
   */
  async connect() {
    try {
      const info = await this._request('GET', '/api/health');
      this.connected = info.ok === true;
      if (this.connected) this._emit('connect', info);
      return this.connected;
    } catch {
      this.connected = false;
      return false;
    }
  }

  /**
   * Lists every printer the agent knows about: locally installed queues plus
   * network printers found via mDNS/Bonjour and the port sweep.
   *
   * @param {object} [options]
   * @param {boolean} [options.refresh=false]  Force a fresh network scan instead of the cache.
   * @returns {Promise<Array<{
   *   id: string, name: string, source: 'local'|'mdns'|'portScan',
   *   host: string|null, port: number|null, protocols: string[],
   *   uri: string|null, makeAndModel: string|null, location: string|null,
   *   isDefault: boolean, state: string|null
   * }>>}
   */
  async getPrinters({ refresh = false } = {}) {
    // Discovery scans (mDNS window + subnet sweep) take a few seconds.
    const timeout = refresh ? Math.max(this.timeoutMs, 20000) : this.timeoutMs;
    return this._request('GET', `/api/printers?refresh=${refresh}`, undefined, timeout);
  }

  /** Forces a full rescan and resolves with the updated list. */
  async refreshPrinters() {
    return this._request('POST', '/api/printers/refresh', undefined, 30000);
  }

  /**
   * Sends a print job.
   *
   * @param {object} job
   * @param {string} job.printerId        Id from getPrinters().
   * @param {string} [job.text]           Text payload (plain text, ZPL, ESC/POS...).
   * @param {ArrayBuffer|Uint8Array|Blob|string} [job.data]
   *        Binary payload (e.g. a PDF). Strings are treated as already-base64.
   * @param {string} [job.contentType='application/octet-stream']
   * @param {string} [job.title]
   * @param {number} [job.copies=1]
   * @returns {Promise<{ok: boolean, printerId: string, via: string, message: string|null}>}
   */
  async print({ printerId, text, data, contentType = 'application/octet-stream', title = 'PrintBridge job', copies = 1 }) {
    if (!printerId) throw new PrintBridgeError('printerId is required', 'BAD_REQUEST');
    if (text == null && data == null) throw new PrintBridgeError('Provide text or data', 'BAD_REQUEST');

    const body = { printerId, contentType, title, copies };
    if (text != null) body.text = text;
    else body.dataBase64 = await toBase64(data);

    const result = await this._request('POST', '/api/print', body, 60000);
    if (!result.ok) throw new PrintBridgeError(result.message || 'Print failed', 'PRINT_FAILED');
    return result;
  }

  /** Convenience: print a PDF from a fetch()-able URL, File input, or Blob. */
  async printPdf(printerId, pdf, options = {}) {
    const blob = typeof pdf === 'string' ? await (await fetch(pdf)).blob() : pdf;
    return this.print({ printerId, data: blob, contentType: 'application/pdf', ...options });
  }

  /**
   * Opens a live WebSocket to the agent. Fires 'printers' with the current list
   * immediately and again whenever a discovery pass changes it.
   * @returns {Promise<void>} resolves once the socket is open.
   */
  subscribe() {
    if (this._ws && this._ws.readyState <= WebSocket.OPEN) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(this.wsUrl);
      this._ws = ws;
      ws.onopen = () => resolve();
      ws.onerror = () => reject(new PrintBridgeError('WebSocket connection failed', 'AGENT_UNREACHABLE'));
      ws.onclose = () => this._emit('disconnect');
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.type === 'printers') this._emit('printers', msg.printers);
        } catch { /* ignore malformed frames */ }
      };
    });
  }

  /** Asks the agent (over the open WebSocket) to rescan; results arrive via the 'printers' event. */
  requestRefresh() {
    if (this._ws?.readyState === WebSocket.OPEN) this._ws.send('refresh');
  }

  /** Closes the WebSocket opened by subscribe(). */
  unsubscribe() {
    this._ws?.close();
    this._ws = null;
  }

  /**
   * @param {'connect'|'disconnect'|'printers'} event
   * @param {(payload:any)=>void} handler
   * @returns {() => void} unsubscribe function
   */
  on(event, handler) {
    if (!this._listeners.has(event)) this._listeners.set(event, new Set());
    this._listeners.get(event).add(handler);
    return () => this._listeners.get(event)?.delete(handler);
  }

  _emit(event, payload) {
    this._listeners.get(event)?.forEach((fn) => {
      try { fn(payload); } catch (e) { console.error('[PrintBridge] listener error', e); }
    });
  }

  async _request(method, path, body, timeoutMs = this.timeoutMs) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const res = await fetch(this.baseUrl + path, {
        method,
        signal: controller.signal,
        headers: body ? { 'Content-Type': 'application/json' } : undefined,
        body: body ? JSON.stringify(body) : undefined,
      });
      const json = await res.json().catch(() => null);
      if (!res.ok) {
        throw new PrintBridgeError(
          json?.message || `Agent returned HTTP ${res.status}`,
          res.status === 400 ? 'BAD_REQUEST' : 'PRINT_FAILED',
        );
      }
      return json;
    } catch (err) {
      if (err instanceof PrintBridgeError) throw err;
      throw new PrintBridgeError(
        'PrintBridge agent is not reachable. Is it installed and running?',
        'AGENT_UNREACHABLE',
      );
    } finally {
      clearTimeout(timer);
    }
  }
}

async function toBase64(data) {
  if (typeof data === 'string') return data; // caller already encoded it
  const blob = data instanceof Blob ? data : new Blob([data]);
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
  return dataUrl.slice(dataUrl.indexOf(',') + 1);
}

export default PrintBridge;
