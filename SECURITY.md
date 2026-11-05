# Security Policy

PrintBridge runs on end-user machines and is reachable from web pages, so we take security
reports seriously.

## Reporting a vulnerability

**Please do not open a public issue.** Report it privately through GitHub's
[private vulnerability reporting](../../security/advisories/new) for this repository.

Please include the affected version and platform, steps to reproduce, and the impact you see
(e.g. a website being able to print without being in `--allowed-origins`).

You can expect an acknowledgement within a few days. Once a fix is released, we're happy to
credit you.

## Supported versions

Only the latest release receives security fixes.

## Scope

In scope: anything that lets a remote party or an unlisted web origin reach the agent, read
the printer list, or print; command injection through API inputs; memory-safety or crash bugs
triggered by network input (mDNS packets, IPP responses).

The security model and its known trade-offs (for example, no certificate validation for
`ipps://` to self-signed printers) are described in
[docs/architecture.md](docs/architecture.md#security-model).
