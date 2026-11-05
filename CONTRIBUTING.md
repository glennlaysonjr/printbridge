# Contributing

Thanks for helping improve PrintBridge! Bug reports, printer compatibility notes, and pull
requests are all welcome.

## Reporting a bug

Please include:

- OS and version, and the PrintBridge version (`printbridge --version`)
- The printer make/model and how it is connected (USB/local queue, network, mDNS or not)
- The agent log with `RUST_LOG=debug`, and the request you sent (`contentType`, payload size)

## Pull requests

1. Fork, create a branch, make your change.
2. Run the checks (CI runs them on Linux, macOS, and Windows):

   ```bash
   cargo fmt --all --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   cd tests/e2e && npm ci && npm test
   ```

3. Add or update unit tests for any parsing, encoding, or routing logic you touch.
4. If you change the JSON API, update `src/models.rs`, both SDK builds in `sdk/`, and
   `docs/http-api.md` together.
5. Add a line to `CHANGELOG.md` under *Unreleased*.

See [docs/development.md](docs/development.md) for the full workflow, including how to test
without a real printer, and [AGENTS.md](AGENTS.md) for the invariants a change must keep
(loopback-only binding, no shell invocation, origin checks).

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
