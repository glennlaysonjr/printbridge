# HTTP & WebSocket API

The SDK wraps this API. Call it directly from any language that can make HTTP requests
from the same machine as the agent.

- Base URL: `http://127.0.0.1:9631`, or whatever `--port` is set to.
- Request and response bodies are JSON with `camelCase` keys.
- The `Host` header must be `localhost`, `127.0.0.1`, or `[::1]`, with any port. Other
  values get `403`. Browsers and normal HTTP clients send this automatically.
- When `--allowed-origins` isn't `*`, browsers only get responses for listed origins
  (CORS), and WebSocket upgrades from other origins get `403`.

| Method | Path | Purpose |
|---|---|---|
| `GET` | [`/api/health`](#get-apihealth) | Liveness and version |
| `GET` | [`/api/printers`](#get-apiprinters) | List printers (cached, or rescans if stale) |
| `POST` | [`/api/printers/refresh`](#post-apiprintersrefresh) | Force a full discovery pass |
| `POST` | [`/api/print`](#post-apiprint) | Submit a print job |
| `GET` (upgrade) | [`/ws`](#websocket-ws) | Live printer list |

## `GET /api/health`

```bash
curl http://127.0.0.1:9631/api/health
```

```json
{ "ok": true, "name": "PrintBridge", "version": "1.0.0", "platform": "macos", "arch": "aarch64" }
```

`platform` is `macos`, `windows`, or `linux`. `arch` is `x86_64` or `aarch64`.

## `GET /api/printers`

| Query | Type | Default | Meaning |
|---|---|---|---|
| `refresh` | `true` / `false` | `false` | Run a full discovery pass before answering |

Without `refresh`, the agent returns the cache. It still rescans first if the cache is older than
`--cache-seconds`. A discovery pass takes about `--mdns-scan-seconds` (4 s by default).

Response: an array of [Printer](#printer), sorted by `source` (local, mdns, portScan) then by
name, case-insensitively.

```bash
curl "http://127.0.0.1:9631/api/printers?refresh=true"
```

## `POST /api/printers/refresh`

Runs a discovery pass and returns the new [Printer](#printer) array. If a pass is already
running, the request waits for it and returns its result instead of starting a second one.

## `POST /api/print`

Body: a [PrintRequest](#printrequest). The body can be up to 64 MiB, which is about 48 MB of
document after base64.

```bash
# Text / ZPL
curl -X POST http://127.0.0.1:9631/api/print \
  -H 'Content-Type: application/json' \
  -d '{"printerId":"mdns:192.168.1.50:9100:raw","text":"^XA^FDHello^FS^XZ"}'

# A PDF file
curl -X POST http://127.0.0.1:9631/api/print \
  -H 'Content-Type: application/json' \
  -d "{\"printerId\":\"local:Office\",\"contentType\":\"application/pdf\",\"dataBase64\":\"$(base64 < doc.pdf | tr -d '\n')\"}"
```

| Status | Body | When |
|---|---|---|
| `200` | [PrintResult](#printresult) with `ok: true` | The job was accepted by the printer or the OS spooler |
| `400` | PrintResult with `ok: false` | Invalid request: malformed JSON, missing `printerId`, unknown printer id, invalid base64, empty document. Nothing was sent. |
| `502` | PrintResult with `ok: false` | The agent tried, and the printer, network, or spooler failed. `message` has the details. |

`200` means the job was *handed off*, not that paper came out. IPP printers and the OS spooler
accept a job and then print it asynchronously.

## WebSocket `/ws`

```js
const ws = new WebSocket('ws://127.0.0.1:9631/ws');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
ws.onopen = () => ws.send('refresh');
```

**Server → client:** one text frame right after connecting, then one after every discovery
pass, whether the HTTP API, a client, or the cache expiry triggered it:

```json
{ "type": "printers", "printers": [ /* Printer[] */ ] }
```

**Client → server:** any text frame that contains `refresh` starts a discovery pass. The result
arrives as a `printers` frame. Other messages are ignored.

The server sends a ping every 30 seconds. Browsers answer automatically.

With an origin allowlist, upgrade requests whose `Origin` header isn't listed get `403`, and
so do requests with no `Origin` header (non-browser clients). Use the HTTP endpoints from
scripts.

## Schemas

### Printer

| Field | Type | Notes |
|---|---|---|
| `id` | string | Pass to `/api/print`. Formats: `local:<queue>`, `mdns:<ip>:<port>:<protocol>`, `scan:<ip>:<port>` |
| `name` | string | Queue name, mDNS instance name, or `"<ip> (<protocol>)"` for scan results |
| `source` | `"local"` \| `"mdns"` \| `"portScan"` | How it was found |
| `host` | string \| null | IP address of a network printer. `null` for local queues |
| `port` | number \| null | Port of the preferred protocol |
| `protocols` | string[] | Any of `spooler`, `ipp`, `ipps`, `raw`, `lpd`, most preferred first |
| `uri` | string \| null | Preferred submission URI: `ipp://`, `ipps://`, `socket://`, `lpd://` |
| `makeAndModel` | string \| null | From mDNS TXT `ty` or `product`, or the Windows driver name |
| `location` | string \| null | mDNS TXT `note`, or the Windows printer location |
| `isDefault` | boolean | The OS default printer (local queues only) |
| `state` | string \| null | e.g. `idle`, `printing`, `disabled`, `offline`. Not always available |
| `discoveredAt` | string | RFC 3339 timestamp of the discovery pass |

### PrintRequest

| Field | Type | Required | Default |
|---|---|---|---|
| `printerId` | string | ✅ | |
| `text` | string | one of `text` / `dataBase64` | Sent as UTF-8 bytes |
| `dataBase64` | string | one of `text` / `dataBase64` | Standard base64 with padding. If both are present, `dataBase64` wins |
| `contentType` | string | | `application/octet-stream`. See [transport selection](usage.md#how-the-content-type-picks-a-transport) |
| `title` | string | | `PrintBridge job` (the job name in the queue) |
| `copies` | integer | | `1` (clamped to 1–999) |

### PrintResult

| Field | Type | Notes |
|---|---|---|
| `ok` | boolean | |
| `printerId` | string \| null | Echoes the request. `null` if the body couldn't be parsed |
| `via` | `"ipp"` \| `"raw"` \| `"spooler"` \| `"none"` | Transport used. `none` when nothing was sent |
| `message` | string \| null | Human-readable status or error |
