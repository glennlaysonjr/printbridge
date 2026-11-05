# Development

- [Prerequisites](#prerequisites)
- [Build and run](#build-and-run)
- [Checks](#checks)
- [Browser end-to-end tests](#browser-end-to-end-tests)
- [Testing with fake printers](#testing-with-fake-printers)
- [Checking other platforms locally](#checking-other-platforms-locally)
- [Making changes](#making-changes)
- [Releasing](#releasing)

## Prerequisites

- [Rust](https://rustup.rs) 1.88+ (edition 2024) and a C compiler (see [Installation](installation.md#build-from-source))
- Node.js 24.8+ (or 22.22.3+), only for the browser e2e tests

## Build and run

```bash
cargo run                                   # debug build, default port 9631
cargo run -- --port 9700 --disable-port-scan
RUST_LOG=debug cargo run
cargo build --release                       # optimized, stripped binary in target/release/
```

Then serve the repo and open the demo:

```bash
python3 -m http.server 8080      # → http://localhost:8080/demo/
```

To point the demo at an agent on another port, add `?agentPort=9700` to the URL.

## Checks

CI runs these on Linux, macOS, and Windows. Run them before you open a PR:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Unit tests sit next to the code (`#[cfg(test)] mod tests`). They cover:

| Module | What is tested |
|---|---|
| `discovery/local.rs` | `lpstat` parsing (states, default printer). Windows `ConvertTo-Json` parsing (single object vs. array) |
| `registry.rs` | Merge rules: mDNS beats port scan, protocol folding and preference, RAW port kept |
| `ipp.rs` | Print-Job byte layout, `copies` attribute, `ipp://` → `http://` mapping (IPv6, default port), status decoding |
| `server.rs` | Host-header allowlist (DNS-rebinding guard) |
| `print.rs` | Raw-payload detection, IPv6 URI bracketing |

## Browser end-to-end tests

`tests/e2e/` uses [e2e](https://github.com/tester-army/e2e), which drives Chromium through
Playwright. The tests use plain locators and assertions only. **No AI model or API key is needed.**

```bash
cargo build                       # the tests run target/debug/printbridge
cd tests/e2e
npm ci
npx e2e-web install chromium      # first time only
npm test                          # = E2E_TELEMETRY_DISABLED=1 e2e run
```

The runner starts `serve.mjs`, which:

1. launches the agent on port **9651** (`AGENT_PORT`) with `--allowed-origins` set to the
   test server's own origin, so CORS and WebSocket origin checks run under real
   production settings;
2. serves the repository root on a free port, so `/demo/` and `/sdk/` load as they would in
   real use.

| File | Covers |
|---|---|
| `tests/e2e/tests/demo.e2e.ts` | Demo connects, lists printers, rescans, shows the install hint when no agent runs |
| `tests/e2e/tests/api.e2e.ts` | Health, printer schema, DNS-rebinding 403, CORS allow/deny, Private Network Access, WebSocket snapshot and origin check, 400 error contract |

To test a release build, set `PRINTBRIDGE_BIN=/path/to/printbridge`. Results, logs, and
traces go to `tests/e2e/.e2e/`. e2e sends anonymous usage telemetry by default. The npm
script and CI turn it off with `E2E_TELEMETRY_DISABLED=1`.

## Testing with fake printers

You don't need a real printer to exercise discovery and printing. On macOS, advertise a fake
device with `dns-sd`. On Linux, use `avahi-publish`:

```bash
# Terminal 1: a "printer" that just captures RAW bytes
nc -l 9100 > job.bin

# Terminal 2: announce it (macOS)
dns-sd -R "Fake Label Printer" _pdl-datastream._tcp local 9100
#           Linux: avahi-publish -s "Fake Label Printer" _pdl-datastream._tcp 9100

# Terminal 3
cargo run -- --disable-port-scan
curl -s -X POST localhost:9631/api/printers/refresh            # find its id
curl -s -X POST localhost:9631/api/print -H 'Content-Type: application/json' \
  -d '{"printerId":"mdns:<ip>:9100:raw","text":"^XA^FDHi^FS^XZ"}'
cat job.bin
```

For IPP, advertise `_ipp._tcp` with `rp=ipp/print` and run any HTTP server that answers
`POST /ipp/print` with an IPP response. The first 8 bytes are `02 00 00 00` followed by the
request's 4-byte request-id, then `03` (end of attributes).

## Checking other platforms locally

Platform-specific code is behind `#[cfg(windows)]` / `#[cfg(not(windows))]`. To type-check
the Windows code from macOS or Linux, use rustup targets. The TLS backend compiles C, so
check against reqwest's provider-less variant, which needs no cross C compiler:

```bash
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
# in a scratch copy of the repo:
sed -i.bak 's/features = \["rustls"\]/features = ["rustls-no-provider"]/' Cargo.toml
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo clippy --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
```

CI builds and tests natively on all three OSes in any case.

## Making changes

- **The JSON contract is shared** by `src/models.rs`, `sdk/printbridge.js`,
  `sdk/printbridge.umd.js`, and `docs/http-api.md`. Change all four together.
- **Never run a shell.** Build commands as argv (`sys::command`). Pass values into PowerShell
  through `env` and read them as `$env:NAME`.
- **Don't weaken the security layers** in `server.rs`: loopback bind, Host check,
  CORS, WebSocket origin check. See [Security model](architecture.md#security-model).
- **Discovery sources must not fail the pass.** Log a warning and return an empty `Vec`.
- New transports go in `PrintService::dispatch`. New discovery sources go in `src/discovery/`
  and are wired into `PrinterRegistry::refresh` and `merge`.

## Releasing

1. Bump `version` in `Cargo.toml` (it's reported by `/api/health`) and update `CHANGELOG.md`.
2. Commit, then tag and push:

   ```bash
   git tag v1.0.1
   git push origin v1.0.1
   ```

3. `.github/workflows/release.yml` builds five targets, packages them with the SDK and
   installers, writes `SHA256SUMS.txt`, and publishes a GitHub Release with generated notes.

Release binaries aren't code-signed. To get rid of the Gatekeeper and SmartScreen prompts,
add Apple notarization (`codesign` + `notarytool`) and Windows Authenticode signing steps
to the workflow. Both need paid certificates.
