# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] - 2026-11-05

First public release.

### Added
- A local agent that lists OS print queues (CUPS / Windows spooler), mDNS/Bonjour network
  printers, and printers found by a /24 port sweep (9100/631/515).
- Printing through IPP, RAW 9100, and the OS spooler, including RAW jobs for label and receipt
  printers (`lp -o raw` on macOS/Linux, WinSpool on Windows) and PDF on Windows through the
  registered app's PrintTo verb.
- A loopback-only HTTP + WebSocket API with a Host-header guard against DNS rebinding, an origin
  allowlist on HTTP and WebSocket, and Chrome Private Network Access support.
- A zero-dependency browser SDK (ES module and `<script>` builds) and a demo page.
- Configuration by CLI flags and `PRINTBRIDGE_*` environment variables.
- Self-contained binaries for macOS (arm64, x64), Windows x64, and Linux (x64, arm64).
- Autostart installers (`packaging/install.sh`, `packaging/install.ps1`), CI on three OSes, and a
  tag-triggered release workflow.
- Browser and API end-to-end tests (`tests/e2e`) and a documentation set in `docs/`.
