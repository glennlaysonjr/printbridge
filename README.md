# PrintBridge

[![CI](https://github.com/glennlaysonjr/printbridge/actions/workflows/ci.yml/badge.svg)](https://github.com/glennlaysonjr/printbridge/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**An open-source (MIT) alternative to paid browser-printing tools. Let any web app discover and print to every printer the user can reach, on macOS, Windows, and Linux.**

PrintBridge is a small native agent (one Rust binary, no runtime to install) plus a
zero-dependency browser SDK. The agent runs on the user's machine and exposes a
**loopback-only** HTTP + WebSocket API; your web page talks to it through the SDK.
It's the same architecture the commercial print bridges use, with no license fees, accounts,
or cloud service.

```
┌────────────────┐  fetch / WebSocket   ┌───────────────────────┐  lpstat/lp · WinSpool · mDNS · port sweep
│  Browser page  │ ───────────────────► │   PrintBridge agent   │ ─────────────────────────────────────────►  printers
│ printbridge.js │ http://127.0.0.1:9631│  (Rust, single binary)│  IPP · RAW 9100 · OS spooler
└────────────────┘                      └───────────────────────┘
```

## Open-source alternative to paid print bridges

Printing silently from a web page to a label printer, receipt printer, or office printer usually
means paying for a commercial print bridge or using a vendor tool that only supports one brand.
PrintBridge does the same job as open-source software, for any printer:

| Tool | Licensing / cost | Printers |
|---|---|---|
| PrintNode | Paid subscription; jobs go through its cloud service | Any |
| QZ Tray | Open-source core; paid commercial support plans | Any |
| Neodynamic JSPrintManager / WebClientPrint | Commercial license | Any |
| Zebra Browser Print | No cost, closed source | Zebra only |
| DYMO Connect | No cost, closed source | DYMO only |
| **PrintBridge** | **Open source, MIT licensed** | **Any** |

- **No license keys, accounts, or per-seat fees.** MIT licensed, so you can use it commercially,
  modify it, and ship it inside your own product.
- **No cloud relay.** Jobs go straight from the browser to the local agent and on to the printer.
  Documents never leave the user's network.
- **Any brand.** Office printers, label printers, and receipt printers all work through standard
  protocols (IPP/AirPrint, RAW 9100, the OS spooler).
- **You control the code.** Audit it, self-host the binaries, and build it yourself.

<sub>Product names are trademarks of their respective owners. Licensing details are summarized as
generally published by each vendor; check their sites for current terms.</sub>

## Features

- **Works with popular printers.** HP, Brother, Canon, Epson, Xerox, Zebra, DYMO, Star, and more. See [Supported printers](#supported-printers).
- **Finds every printer.** It lists installed OS queues (CUPS / Windows spooler), network
  printers that announce themselves over mDNS/Bonjour (AirPrint, IPP Everywhere), and
  older devices it finds by sweeping the local /24 subnet for ports 9100/631/515.
- **Prints anything the printer understands.** PDFs go through IPP (driverless) or the OS
  spooler. Raw ZPL/EPL/ESC-POS goes byte-for-byte to label and receipt printers, over
  RAW 9100 or a RAW spool job.
- **Live updates.** The agent pushes the printer list over a WebSocket whenever a scan finishes.
- **Secure by default.** It binds to `127.0.0.1` only, blocks DNS rebinding, enforces an origin
  allowlist on HTTP *and* WebSockets, and supports Chrome's Private Network Access.
- **Runs everywhere.** One static binary each for macOS (Apple Silicon + Intel), Windows x64,
  and Linux x64/arm64. It can start at login through LaunchAgent, systemd, or Task Scheduler.

## Quick start

**1. Run the agent.** Download the archive for your platform from the
[Releases](../../releases) page, extract it, and run:

```bash
./printbridge
```

Or build it from source (needs [Rust](https://rustup.rs) 1.88 or newer):

```bash
cargo run --release
```

**2. Use the SDK from your page:**

```html
<script type="module">
  import { PrintBridge } from './sdk/printbridge.js';

  const bridge = new PrintBridge();
  if (!(await bridge.connect())) {
    alert('Please install and start PrintBridge');
  } else {
    const printers = await bridge.getPrinters();
    await bridge.print({ printerId: printers[0].id, text: 'Hello from the web!\n' });
  }
</script>
```

**3. Try the demo.** Serve the repo over HTTP and open <http://localhost:8080/demo/>:

```bash
python3 -m http.server 8080
```

> **Before shipping:** restrict which websites may use the agent with
> `--allowed-origins https://your-app.example.com`. The default `*` lets *any* site the user
> visits list their printers and print. See [Security](docs/architecture.md#security-model).

## Documentation

| Doc | For |
|---|---|
| [Installation](docs/installation.md) | Downloading, verifying, OS permission prompts, autostart at login, building from source |
| [Usage](docs/usage.md) | CLI flags and environment variables, the JS SDK, printing recipes (PDF, ZPL labels, receipts) |
| [HTTP & WebSocket API](docs/http-api.md) | Endpoint reference and JSON schemas, for calling the agent without the SDK |
| [Architecture](docs/architecture.md) | How discovery, deduplication, print routing, and the security model work |
| [Development](docs/development.md) | Building, unit tests, browser e2e tests, cross-platform checks, releasing |
| [Troubleshooting](docs/troubleshooting.md) | Printers not found, jobs not printing, connection problems |
| [AGENTS.md](AGENTS.md) · [llms.txt](llms.txt) | Context for AI coding assistants and LLMs working on or with this repo |

## Supported printers

PrintBridge works with printers by **protocol**, not through per-model drivers. If a printer
is installed in the OS, advertises AirPrint/IPP, or accepts RAW jobs on port 9100, PrintBridge
can reach it. The models below are popular examples, grouped by the best way to print to them.

| Category | Popular models | Best route | Send |
|---|---|---|---|
| Office lasers & MFPs | HP LaserJet Pro / Enterprise, Brother HL-L & MFC-L, Canon imageCLASS, Xerox VersaLink / AltaLink, Lexmark MS/MX/CS, Ricoh IM, Kyocera ECOSYS | Network IPP (AirPrint), or the local queue | PDF (`application/pdf`) |
| Home & small-office inkjets | HP OfficeJet / ENVY / Smart Tank, Epson EcoTank / WorkForce, Canon PIXMA / MAXIFY, Brother INKvestment | **Local queue** (OS driver) | PDF |
| Shipping & barcode labels | Zebra ZD421 / ZD621 / GK420d / ZT411, Honeywell PC42, TSC TE200 | Network RAW 9100, or the local queue in raw mode | ZPL / EPL (`application/octet-stream`) |
| Desktop label printers | DYMO LabelWriter 550, Brother QL-820NWB / QL-1110NWB, Rollo | **Local queue** (OS driver) | PDF sized to the label |
| Receipt / POS | Epson TM-T88VI / TM-T88VII / TM-m30III, Star TSP143IV / mC-Print3, Bixolon SRP-350 | Network RAW 9100, or the local queue in raw mode | ESC/POS bytes (`application/octet-stream`) |

Rules of thumb:

- **PDF over the network** works on most office-class printers. Many inkjets and some budget lasers
  only accept AirPrint raster over IPP, not PDF. For those, add the printer in the OS and print to
  its local queue so the driver renders the PDF.
- **DYMO and Brother QL** use their own raster formats, so print PDFs through their installed driver
  (local queue) rather than sending raw bytes.
- **Star printers** speak ESC/POS only when their emulation mode is set to ESC/POS. Otherwise they
  use Star Line Mode commands.
- Not listed? If it shows up in `GET /api/printers`, it can receive jobs. What prints correctly
  depends on the payload. See [transport selection](docs/usage.md#how-the-content-type-picks-a-transport).

These models are listed by protocol compatibility. They haven't all been individually tested.
Reports of working (or non-working) models are very welcome; please
[open an issue](../../issues).

## Platform support

| | macOS | Windows | Linux |
|---|---|---|---|
| Local queues | CUPS (`lpstat`, `lp`) | WMI `Win32_Printer`, WinSpool | CUPS (`lpstat`, `lp`) |
| mDNS / Bonjour discovery | ✅ | ✅ | ✅ |
| Subnet port sweep | ✅ | ✅ | ✅ |
| IPP / IPPS printing | ✅ | ✅ | ✅ |
| RAW 9100 printing | ✅ | ✅ | ✅ |
| Prebuilt binaries | arm64, x64 | x64 | x64, arm64 |

## Repository layout

| Path | What it is |
|---|---|
| `src/` | The Rust agent ([module map](docs/architecture.md#module-map)) |
| `sdk/printbridge.js` | Browser SDK, ES module |
| `sdk/printbridge.umd.js` | The same SDK as a plain `<script>` global |
| `demo/index.html` | Working demo page |
| `packaging/` | Autostart installers: `install.sh` (macOS/Linux), `install.ps1` (Windows) |
| `tests/e2e/` | Browser and API end-to-end tests ([e2e](https://github.com/tester-army/e2e)) |
| `.github/workflows/` | CI (lint, test, e2e on 3 OSes) and the tag-triggered release build |

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). To report
a security issue, follow [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
