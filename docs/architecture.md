# Architecture

- [Overview](#overview)
- [Module map](#module-map)
- [Discovery](#discovery)
- [Deduplication](#deduplication)
- [Print routing](#print-routing)
- [Security model](#security-model)
- [Design decisions](#design-decisions)

## Overview

```
                    ┌──────────────────────────── printbridge (tokio runtime) ───────────────────────────┐
 browser ── HTTP ──►│ server.rs   Host check → CORS → routes      ┌──────────► discovery/local.rs   (lpstat / WMI)
 SDK      ── WS ───►│             /api/* , /ws  ──► registry.rs ──┼──────────► discovery/mdns.rs    (mDNS browse)
                    │                                   ▲ cache   └──────────► discovery/port_scan.rs (/24 sweep)
                    │             /api/print ──► print.rs ──┬─► ipp.rs  ── HTTP(S) ──► IPP printer
                    │                                       ├─► TCP 9100 ───────────► RAW printer
                    │                                       └─► lp / WinSpool / PowerShell ──► OS spooler
                    └────────────────────────────────────────────────────────────────────────────────────┘
```

The agent is a single async process. It has no database and no files of its own. It keeps the
printer list in memory and rebuilds it on every discovery pass.

## Module map

| File | Responsibility |
|---|---|
| `src/main.rs` | Parses config, sets up logging, binds `127.0.0.1:<port>`, warms the cache, shuts down gracefully |
| `src/config.rs` | CLI flags and environment variables (`clap`). Origin allowlist helpers |
| `src/models.rs` | `Printer`, `PrinterSource`, `PrintRequest`, `PrintResult`: the JSON contract with the SDK |
| `src/server.rs` | `axum` router, Host-header check, CORS, WebSocket loop, HTTP status mapping |
| `src/registry.rs` | Runs the three discovery sources in parallel, merges and dedupes, caches, broadcasts changes |
| `src/discovery/local.rs` | Installed queues: `lpstat -p` / `lpstat -d` (CUPS) or `Win32_Printer` through PowerShell (Windows) |
| `src/discovery/mdns.rs` | Browses `_ipp`, `_ipps`, `_pdl-datastream`, `_printer` over mDNS (`mdns-sd`, pure Rust) |
| `src/discovery/port_scan.rs` | Connect-only probes of 9100/631/515 across local /24 networks, 64 at a time |
| `src/print.rs` | Validates jobs, picks a transport, RAW socket sender, spooler integration (`lp`, PowerShell, WinSpool FFI) |
| `src/ipp.rs` | Minimal IPP 2.0 `Print-Job` encoder and status decoder (RFC 8010/8011) |
| `src/sys.rs` | Helpers for spawning processes: no console window on Windows, C locale, timeouts, `-EncodedCommand` PowerShell |

## Discovery

A *discovery pass* (`PrinterRegistry::refresh`) runs three sources at the same time:

1. **Local queues.** These print through the OS spooler, so the driver handles
   rendering. On macOS and Linux the agent runs `lpstat` under `LC_ALL=C` so the output is
   always in English and parseable. On Windows it queries WMI `Win32_Printer` through
   PowerShell, which reports the default printer, driver name, location, and status.
2. **mDNS / Bonjour.** This is how AirPrint and nearly all modern printers announce
   themselves. The agent listens for `--mdns-scan-seconds`. It reads the TXT records `rp`
   (resource path), `ty` / `product` (model), and `note` (location). It prefers an IPv4
   address and falls back to a routable IPv6 one.
3. **Port sweep (fallback).** This catches older devices that don't advertise. For every up,
   non-loopback, non-point-to-point IPv4 interface with a prefix of /24 or narrower, the agent
   tries a TCP connect to ports 9100, 631, and 515 on hosts .1–.254, in that order, and stops at
   the first open port. It only connects and never writes anything. Wider networks such as a /16
   are skipped on purpose. Turn the sweep off with `--disable-port-scan`.

A source that fails (no CUPS, multicast blocked, ...) logs a warning and returns nothing,
so it never hides the other sources. Concurrent refresh calls share one pass. After each
pass, the full list goes to WebSocket subscribers on a `tokio::sync::broadcast` channel.

## Deduplication

`registry::merge` applies these rules:

- **Local queues are always kept.** A local queue may point at a network printer the
  agent also found, but it has a different id and goes through a different path (the driver),
  so both are useful.
- **Network printers: one entry per host IP.** mDNS beats the port scan because it knows
  names, models, and IPP paths. Port-scan hits for a host mDNS already found are dropped.
- **Several protocols from one device** (for example IPP and RAW) merge into one entry.
  The entry keeps the id, `port`, and `uri` of the most preferred protocol, in the order
  **ipp > ipps > raw > lpd**, and lists all protocols in `protocols`. The RAW port is kept
  separately (`raw_port`, not serialized), so the RAW fallback dials the right port.

## Print routing

`PrintService::dispatch`:

1. **Local queue →** OS spooler:
   - macOS/Linux: `lp -d <queue> -n <copies> -t <title> [-o raw] -- <tempfile>`.
     `-o raw` is added for opaque payloads (`application/octet-stream`, `vnd.cups-raw`, `*zpl*`, `*escpos*`).
   - Windows: `text/*` → `Out-Printer`; `application/pdf` → the *PrintTo* verb of the registered
     PDF application (the temp file is kept for 5 minutes because that application opens it
     asynchronously); anything else → a WinSpool `RAW` job (`OpenPrinterW` / `StartDocPrinterW` /
     `WritePrinter`).
2. **Network printer that advertises `ipp`/`ipps` →** IPP `Print-Job` to its `uri`, or to
   `ipp://<host>:631/ipp/print`. The IPP request carries `document-format` = `contentType` and,
   when greater than 1, `copies`.
3. **Fallback →** RAW TCP to `raw_port`, or 9100. The payload is written `copies` times. If IPP
   was tried first, the error message includes both failures.

Temp files are created with unique names (`create_new`) in the OS temp directory and
deleted after spooling.

## Security model

The agent is a local service that any web page could try to reach. The protections, in
order:

| Threat | Mitigation |
|---|---|
| Other machines on the LAN calling the agent | It binds `127.0.0.1` only. Nothing listens on external interfaces. |
| DNS rebinding (`evil.com` resolving to 127.0.0.1 to become same-origin) | Middleware rejects any `Host` header other than `localhost` / `127.0.0.1` / `[::1]` with `403`. It runs before routing and CORS. |
| Arbitrary websites listing printers or printing | CORS origin allowlist (`--allowed-origins`). **The default `*` is for development only.** The agent logs a warning at startup while it is `*`. |
| Cross-site WebSocket hijacking (browsers don't apply CORS to WebSockets) | `/ws` checks the `Origin` header against the same allowlist and returns `403` on a mismatch. |
| Public HTTPS sites blocked from reaching loopback (Chrome Private Network Access) | The CORS layer answers `Access-Control-Allow-Private-Network: true` for allowed origins. |
| Command injection through queue names or job titles | Never goes through a shell. `lp` gets an argv array. PowerShell scripts are fixed strings passed with `-EncodedCommand`, and user values arrive as environment variables. |
| Script injection in UIs through mDNS names (any LAN device can pick its own name) | The demo inserts every network-provided string as text. Do the same in your UI. |
| Huge request bodies | 64 MiB body limit. `copies` is clamped to 1–999. |

**Known trade-off:** for `ipps://`, certificate validation is off, because network printers almost
universally ship self-signed certificates. You get transport encryption without authentication,
which is the same posture CUPS takes by default for LAN printers.

## Design decisions

- **Rust + tokio + axum.** One small static binary per platform, no runtime for users to install,
  and memory safety for a process that parses untrusted network input (mDNS packets, IPP responses).
- **Pure-Rust mDNS (`mdns-sd`).** It needs no Bonjour SDK on Windows and no Avahi on Linux, and it
  coexists with the system responder on port 5353.
- **Shell out to `lpstat` / `lp` instead of linking libcups.** That keeps the binary portable,
  with no libcups version coupling, and these tools are present wherever CUPS is.
- **WinSpool through `windows-sys` FFI for RAW jobs.** RAW is the only reliable way to deliver
  ZPL or ESC/POS to a USB label or receipt printer on Windows. PowerShell has no RAW cmdlet.
- **`rustls` with `aws-lc-rs`.** It needs no OpenSSL on Linux, so release binaries run on any
  glibc 2.35+ distribution.
- **Distinct error statuses.** `/api/print` returns `400` for invalid requests and `502` for
  printer or transport failures, so the SDK can tell `BAD_REQUEST` from `PRINT_FAILED`.
