/**
 * PrintBridge SDK — <script>-tag build.
 * Exposes window.PrintBridge (constructor) and window.PrintBridgeError.
 * Generated from printbridge.js; keep the two in sync.
 *
 *   <script src="printbridge.umd.js"></script>
 *   <script>
 *     const bridge = new PrintBridge();
 *     bridge.connect().then(ok => ...);
 *   </script>
 */
(function (global) {
  'use strict';

  const DEFAULT_PORT = 9631;

  class PrintBridgeError extends Error {
    constructor(message, code) {
      super(message);
      this.name = 'PrintBridgeError';
      this.code = code;
    }
  }

  class PrintBridge {
    constructor({ port = DEFAULT_PORT, timeoutMs = 5000 } = {}) {
      this.baseUrl = `http://127.0.0.1:${port}`;
      this.wsUrl = `ws://127.0.0.1:${port}/ws`;
      this.timeoutMs = timeoutMs;
      this.connected = false;
      this._ws = null;
      this._listeners = new Map();
    }

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

    async getPrinters({ refresh = false } = {}) {
      const timeout = refresh ? Math.max(this.timeoutMs, 20000) : this.timeoutMs;
      return this._request('GET', `/api/printers?refresh=${refresh}`, undefined, timeout);
    }

    async refreshPrinters() {
      return this._request('POST', '/api/printers/refresh', undefined, 30000);
    }

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

    async printPdf(printerId, pdf, options = {}) {
      const blob = typeof pdf === 'string' ? await (await fetch(pdf)).blob() : pdf;
      return this.print({ printerId, data: blob, contentType: 'application/pdf', ...options });
    }

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

    requestRefresh() {
      if (this._ws && this._ws.readyState === WebSocket.OPEN) this._ws.send('refresh');
    }

    unsubscribe() {
      if (this._ws) this._ws.close();
      this._ws = null;
    }

    on(event, handler) {
      if (!this._listeners.has(event)) this._listeners.set(event, new Set());
      this._listeners.get(event).add(handler);
      const set = this._listeners.get(event);
      return () => set.delete(handler);
    }

    _emit(event, payload) {
      const set = this._listeners.get(event);
      if (!set) return;
      set.forEach((fn) => {
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
            (json && json.message) || `Agent returned HTTP ${res.status}`,
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
    if (typeof data === 'string') return data;
    const blob = data instanceof Blob ? data : new Blob([data]);
    const dataUrl = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(blob);
    });
    return dataUrl.slice(dataUrl.indexOf(',') + 1);
  }

  global.PrintBridge = PrintBridge;
  global.PrintBridgeError = PrintBridgeError;
})(typeof window !== 'undefined' ? window : globalThis);
