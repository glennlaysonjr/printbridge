# AGENTS.md

Guidance for AI coding assistants (Claude Code, Codex, Cursor, Copilot, etc.) working in this
repository. Human contributors may find it useful too.

## What this project is

PrintBridge is a **local print agent** written in Rust. It runs on an end user's machine, finds
every reachable printer (OS queues, mDNS/Bonjour, a /24 port sweep), and exposes a
**loopback-only** HTTP + WebSocket API on `127.0.0.1:9631`. Web pages use it through the
browser SDK in `sdk/`. It targets macOS, Windows, and Linux from one codebase.

## Commands

```bash
cargo build                                   # debug build
cargo run -- --port 9700 --disable-port-scan  # run locally
cargo fmt --all                               # format (CI checks --check)
cargo clippy --all-targets -- -D warnings     # lint (must be clean)
cargo test                                    # unit tests
cd tests/e2e && npm ci && npm test            # browser + API e2e (needs `cargo build` first)
```

Before you consider a change done, all four checks must pass: fmt, clippy with
`-D warnings`, `cargo test`, and the e2e suite.

## Repository map

```
src/main.rs              entry: config, logging, bind 127.0.0.1, serve, graceful shutdown
src/config.rs            CLI flags + PRINTBRIDGE_* env vars (clap)
src/models.rs            JSON contract: Printer, PrinterSource, PrintRequest, PrintResult
src/server.rs            axum routes, Host-header guard, CORS, WebSocket, status mapping
src/registry.rs          parallel discovery, merge/dedupe, cache, broadcast
src/discovery/local.rs   lpstat (CUPS) / Win32_Printer via PowerShell
src/discovery/mdns.rs    mDNS browse (mdns-sd crate)
src/discovery/port_scan.rs  connect-only probes of 9100/631/515
src/print.rs             transport selection, RAW socket, lp / PowerShell / WinSpool FFI
src/ipp.rs               minimal IPP 2.0 Print-Job encoder
src/sys.rs               process helpers (no window on Windows, C locale, timeouts)
sdk/printbridge.js       browser SDK (ES module)       ┐ keep these two
sdk/printbridge.umd.js   browser SDK (script global)   ┘ in sync
demo/index.html          demo page (?agentPort= overrides the port)
tests/e2e/               e2e tests: serve.mjs starts the agent on 9651 + a static server
packaging/               install.sh (LaunchAgent/systemd), install.ps1 (scheduled task)
docs/                    installation, usage, http-api, architecture, development, troubleshooting
```

## Invariants: do not break these

1. **Bind loopback only** (`Ipv4Addr::LOCALHOST` in `main.rs`). Never `0.0.0.0`.
2. **The Host-header guard** (`require_localhost_host` in `server.rs`) stays the outermost layer.
   It blocks DNS rebinding.
3. **The WebSocket origin check** in `websocket()` must enforce the same allowlist as CORS. Browsers
   don't apply CORS to WebSockets.
4. **Never invoke a shell.** Use `sys::command(...)` with argv. For PowerShell, use `sys::powershell`
   with fixed script text and pass user input through `env`. Never interpolate user input into a script.
5. **The JSON contract** (`models.rs`) is shared with both SDK builds and `docs/http-api.md`.
   Changes must update all four. Keep fields `camelCase` and keep it backward compatible.
6. **HTTP status semantics for `/api/print`**: `200` ok, `400` = invalid request (`PrintResult::rejected`),
   `502` = printer/transport failure (`PrintResult::fail`). The SDK maps 400 → `BAD_REQUEST` and
   everything else → `PRINT_FAILED`.
7. **Discovery sources never return errors.** They log with `tracing::warn!` and return an empty `Vec`.
8. **Platform code** goes behind `#[cfg(windows)]` / `#[cfg(not(windows))]`. Code used on only one
   platform needs `#[cfg_attr(..., allow(dead_code))]` so clippy stays clean on the other.
9. **Treat network-supplied strings as untrusted.** mDNS names and TXT values are attacker-controllable.
   Never render them as HTML (the demo uses `textContent`).

## Conventions

- Rust edition 2024, MSRV 1.88. Let-chains (`if let … && let …`) are fine.
- Errors: `anyhow::Result` with `.context(...)` inside the agent. Format user-facing messages with `{err:#}`.
- Logging: `tracing` macros. No `println!`.
- Comments explain *why* (protocol quirks, security reasons), not *what*.
- Unit tests live in a `#[cfg(test)] mod tests` at the bottom of each module. Add one for any
  parsing or encoding logic you touch.
- Keep dependencies minimal and pure Rust where possible. Release binaries must not need
  OpenSSL or other system libraries.

## Testing without hardware

Advertise a fake printer and capture what it receives (macOS shown; on Linux use `avahi-publish`):

```bash
nc -l 9100 > job.bin &
dns-sd -R "Fake" _pdl-datastream._tcp local 9100 &
cargo run -- --disable-port-scan
```

See `docs/development.md#testing-with-fake-printers` for IPP.

## Common tasks

- **New discovery source:** add `src/discovery/<name>.rs` with `pub async fn discover(...) -> Vec<Printer>`,
  add a `PrinterSource` variant (its position sets the sort order), call it in `PrinterRegistry::refresh`,
  and decide its merge priority in `registry::merge`.
- **New transport:** extend `PrintService::dispatch` in `print.rs`. Return `PrintResult::ok(.., via, ..)`
  with a new `via` string, and document it in `docs/http-api.md`.
- **New config option:** add a field to `Config` with `#[arg(long, env = "PRINTBRIDGE_…")]`, and document
  it in `docs/usage.md#configuration`.
