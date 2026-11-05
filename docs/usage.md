# Usage

- [Running the agent](#running-the-agent)
- [Configuration](#configuration)
- [The JavaScript SDK](#the-javascript-sdk)
- [Printing recipes](#printing-recipes)
- [How the content type picks a transport](#how-the-content-type-picks-a-transport)
- [Production checklist](#production-checklist)

## Running the agent

```bash
printbridge                                        # defaults: port 9631, any origin
printbridge --allowed-origins https://app.example.com
printbridge --help
```

On startup the agent binds `127.0.0.1:<port>` and starts a discovery pass in the
background, so the first `getPrinters()` call is answered from a warm cache. It
shuts down cleanly on Ctrl+C or SIGTERM.

## Configuration

Every option is a command-line flag and an environment variable. Flags win over variables.

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--port` | `PRINTBRIDGE_PORT` | `9631` | TCP port. Always bound to `127.0.0.1`. |
| `--allowed-origins` | `PRINTBRIDGE_ALLOWED_ORIGINS` | `*` | Comma-separated web origins allowed to use the agent, e.g. `https://app.example.com,https://admin.example.com`. Applies to HTTP (CORS) and WebSocket connections. `*` allows every site. |
| `--mdns-scan-seconds` | `PRINTBRIDGE_MDNS_SCAN_SECONDS` | `4` | How long each discovery pass listens for mDNS answers. |
| `--port-probe-timeout-ms` | `PRINTBRIDGE_PORT_PROBE_TIMEOUT_MS` | `350` | TCP connect timeout per host and port in the subnet sweep. |
| `--disable-port-scan` | `PRINTBRIDGE_DISABLE_PORT_SCAN` | off | Skip the /24 sweep. Only local queues and mDNS are used. |
| `--cache-seconds` | `PRINTBRIDGE_CACHE_SECONDS` | `60` | `GET /api/printers` rescans when the cached list is older than this. |

**Logging** uses the standard `RUST_LOG` variable. The default is `info,mdns_sd=warn`.

```bash
RUST_LOG=debug printbridge          # everything
RUST_LOG=printbridge=debug printbridge
```

**Origins** must match the browser's `Origin` header exactly: scheme, host, and port, with no
path or trailing slash. For example, `http://localhost:3000` and `http://127.0.0.1:3000` are different origins.

## The JavaScript SDK

The SDK is a single file with no dependencies, in two builds:

```js
// ES module
import { PrintBridge, PrintBridgeError } from './sdk/printbridge.js';
```

```html
<!-- Classic script: defines window.PrintBridge and window.PrintBridgeError -->
<script src="/sdk/printbridge.umd.js"></script>
```

Copy the file into your app's static assets. The two builds must stay in sync when you edit them.

### Connect

```js
const bridge = new PrintBridge({ port: 9631, timeoutMs: 5000 }); // both optional

if (!(await bridge.connect())) {
  // Agent not installed or not running. Show your "install PrintBridge" UI.
}
```

`connect()` never throws. It resolves `true` or `false`.

### List printers

```js
const printers = await bridge.getPrinters();                 // cached (fast)
const fresh = await bridge.getPrinters({ refresh: true });   // full rescan (a few seconds)
```

Each printer looks like this:

```js
{
  id: 'mdns:192.168.1.40:631:ipp',   // pass this to print()
  name: 'Office LaserJet',
  source: 'mdns',                    // 'local' | 'mdns' | 'portScan'
  host: '192.168.1.40',              // null for local queues
  port: 631,
  protocols: ['ipp', 'raw'],         // 'spooler' | 'ipp' | 'ipps' | 'raw' | 'lpd'
  uri: 'ipp://192.168.1.40:631/ipp/print',
  makeAndModel: 'HP LaserJet Pro M404',
  location: '2nd floor',
  isDefault: false,                  // OS default printer (local queues only)
  state: 'idle',                     // free-form, may be null
  discoveredAt: '2026-11-05T17:19:01.66Z'
}
```

Ids are stable for the same device and address, but a printer that changes IP gets a new id.
If a print fails with "Unknown printer id", refresh the list.

### Print

```js
// Text, ZPL, EPL, ESC/POS...
await bridge.print({ printerId, text: '^XA^FO50,50^ADN,36,20^FDHello^FS^XZ' });

// Binary: ArrayBuffer, Uint8Array, Blob, or an already-base64 string
await bridge.print({
  printerId,
  data: pdfBlob,
  contentType: 'application/pdf',
  title: 'Invoice #42',   // job name shown in the queue
  copies: 2,              // 1–999
});

// PDF from a URL, File, or Blob
await bridge.printPdf(printerId, '/invoices/42.pdf');
```

`print()` resolves to `{ ok: true, printerId, via, message }`. `via` is `'ipp'`, `'raw'`, or
`'spooler'`, depending on the transport that was used.

### Live updates

```js
bridge.on('printers', (list) => render(list));   // initial snapshot + after every scan
bridge.on('disconnect', () => showOffline());
await bridge.subscribe();                         // opens the WebSocket

bridge.requestRefresh();                          // rescan; result arrives via 'printers'
bridge.unsubscribe();                             // close the socket
```

`on()` returns a function that removes the listener.

### Errors

Every failure throws `PrintBridgeError`. Check its `code`:

| `code` | Meaning | Typical response |
|---|---|---|
| `AGENT_UNREACHABLE` | Agent not running, wrong port, origin not allowed, or timeout | Show install/start instructions |
| `BAD_REQUEST` | Invalid job: unknown printer id, missing payload, bad base64 | Fix the call; refresh printers |
| `PRINT_FAILED` | The agent tried and the printer or spooler failed | Show `err.message` to the user |

```js
try {
  await bridge.print({ printerId, text });
} catch (err) {
  if (err.code === 'AGENT_UNREACHABLE') showInstallPrompt();
  else toast(err.message);
}
```

When the origin isn't in `--allowed-origins`, the browser blocks the response
and the SDK reports `AGENT_UNREACHABLE`. Check the browser console for the CORS error.

## Printing recipes

### Shipping label on a Zebra printer (ZPL)

```js
const zpl = `^XA
^FO50,50^A0N,50,50^FDOrder 1042^FS
^FO50,120^BY3^BCN,100,Y,N,N^FD1042^FS
^XZ`;
await bridge.print({ printerId, text: zpl, contentType: 'application/octet-stream' });
```

Network Zebras take this over RAW 9100. For a USB Zebra installed as a local queue,
`application/octet-stream` makes the agent submit it as a raw job (`lp -o raw` on macOS/Linux,
the RAW datatype on Windows), so the driver doesn't render the commands as text.

### Receipt on a thermal printer (ESC/POS)

```js
const ESC = '\x1b', GS = '\x1d';
const bytes = new TextEncoder().encode(
  `${ESC}@${ESC}a\x01Coffee Shop\n${ESC}a\x00Latte        4.50\n\n\n${GS}V\x41\x03`
);
await bridge.print({ printerId, data: bytes, contentType: 'application/octet-stream' });
```

Use `data` with a `Uint8Array` for byte values above 0x7F. `text` is sent as UTF-8.

### PDF to an office printer

```js
const pdf = await fetch('/reports/q3.pdf').then((r) => r.blob());
await bridge.print({ printerId, data: pdf, contentType: 'application/pdf', title: 'Q3 report' });
```

Most network printers made in the last decade accept PDF over IPP. If one doesn't, print to its
local queue instead (add it in the OS printer settings), so the driver renders the PDF.

## How the content type picks a transport

| Printer | `contentType` | What happens |
|---|---|---|
| Local queue, macOS/Linux | `application/octet-stream`, `application/vnd.cups-raw`, anything containing `zpl` or `escpos` | `lp -o raw`: bytes go to the device untouched |
| Local queue, macOS/Linux | anything else (`application/pdf`, `text/plain`, ...) | `lp`: CUPS filters and the driver render it |
| Local queue, Windows | `text/*` | PowerShell `Out-Printer` renders it through the driver |
| Local queue, Windows | `application/pdf` | The registered PDF application's *PrintTo* verb |
| Local queue, Windows | anything else | WinSpool RAW job: bytes go to the device untouched |
| Network, advertises IPP | any | IPP Print-Job with `document-format` = `contentType`. Falls back to RAW 9100 if it fails |
| Network, RAW only | any | Bytes to TCP 9100, or the advertised port |

## Production checklist

- [ ] Set `--allowed-origins` to your real origins. Never ship `*`.
- [ ] Install with the autostart scripts so the agent survives reboots.
- [ ] In your UI, handle `AGENT_UNREACHABLE` with a download link to your release.
- [ ] For HTTPS sites: Chrome and Edge allow `http://127.0.0.1` from secure pages, and the agent
      answers their Private Network Access preflight. Firefox and Safari may block mixed
      content to loopback. Test your target browsers.
- [ ] Use a non-default `--port` only if you also pass the same `port` to `new PrintBridge({ port })`.
