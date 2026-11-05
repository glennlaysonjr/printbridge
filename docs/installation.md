# Installation

PrintBridge is a single executable with no runtime dependencies. Install it on each
machine whose printers your web app should reach.

- [Download a release](#download-a-release)
- [macOS](#macos)
- [Windows](#windows)
- [Linux](#linux)
- [Start at login](#start-at-login)
- [Build from source](#build-from-source)
- [Uninstall](#uninstall)

## Download a release

Every tagged release on the GitHub [Releases](../../../releases) page has these archives:

| Platform | Archive |
|---|---|
| macOS, Apple Silicon | `printbridge-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `printbridge-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Windows x64 (also runs on Windows on ARM through emulation) | `printbridge-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| Linux x64 (glibc 2.35+) | `printbridge-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 (glibc 2.35+) | `printbridge-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |

Each archive contains the binary, `README.md`, `LICENSE`, the browser SDK (`sdk/`), and the
autostart installer for that OS.

Verify the download against `SHA256SUMS.txt` from the same release:

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing     # macOS
sha256sum -c SHA256SUMS.txt --ignore-missing         # Linux
```

```powershell
Get-FileHash .\printbridge-*.zip -Algorithm SHA256   # Windows: compare by eye
```

## macOS

```bash
tar -xzf printbridge-*-apple-darwin.tar.gz
cd printbridge-*-apple-darwin
./printbridge
```

**Gatekeeper.** Release binaries aren't notarized, so macOS blocks a binary
downloaded through a browser ("cannot be opened because the developer cannot be verified").
Remove the quarantine flag once:

```bash
xattr -d com.apple.quarantine ./printbridge
```

`packaging/install.sh` does this for you.

**Local Network permission** (macOS 15 and later). The first time the agent sends mDNS
queries or probes the subnet, macOS asks whether it may "find devices on your local
network". Click **Allow**. If you deny it, only locally installed queues are listed.
You can change it later in *System Settings → Privacy & Security → Local Network*.

**Local queues.** These come from CUPS, which ships with macOS. Printers added in
*System Settings → Printers & Scanners* appear as `source: "local"`.

## Windows

1. Extract `printbridge-*-x86_64-pc-windows-msvc.zip`.
2. Run `printbridge.exe` from a terminal, or install it to start at logon (see [Start at login](#start-at-login)).

**SmartScreen.** Unsigned executables downloaded from the internet may trigger
"Windows protected your PC". Choose *More info → Run anyway*, or run
`Unblock-File .\printbridge.exe`. `install.ps1` unblocks the file for you.

**Firewall.** The HTTP API listens only on `127.0.0.1`, so it never needs a firewall rule.
mDNS discovery uses multicast UDP port 5353, so Windows may ask to allow network access.
Allow it on *Private* networks to discover network printers.

**PDFs to local queues.** Windows has no built-in way to rasterize a PDF for a print queue.
PrintBridge hands PDFs to the registered PDF application's *PrintTo* command, so you need one that
supports it, such as Adobe Acrobat Reader or SumatraPDF. Network printers don't need this,
because PDFs go to them directly over IPP.

## Linux

```bash
tar -xzf printbridge-*-linux-gnu.tar.gz
cd printbridge-*-linux-gnu
./printbridge
```

**Requirements:**

- **CUPS client tools** for local queues: `lpstat` and `lp`.
  - Debian/Ubuntu: `sudo apt install cups-client`
  - Fedora/RHEL: `sudo dnf install cups-client`
  - Arch: `sudo pacman -S cups`

  Without them, the local source returns nothing, but mDNS and the port sweep still work.
- **No Avahi needed.** PrintBridge has its own mDNS implementation. It coexists with
  `avahi-daemon`.
- **Firewall.** If you use `ufw` or `firewalld`, allow inbound UDP 5353 (mDNS answers) for
  network discovery, e.g. `sudo ufw allow 5353/udp`.

## Start at login

The archives include installer scripts. They copy the binary into your user profile and register it to
start at login, under your user account with no root or admin rights needed.

### macOS and Linux

```bash
./install.sh ./printbridge
```

To restrict which sites may use the agent (recommended), pass the allowlist at install time:

```bash
PRINTBRIDGE_ALLOWED_ORIGINS="https://app.example.com" ./install.sh ./printbridge
```

| | Installed binary | Autostart | Logs |
|---|---|---|---|
| macOS | `~/.local/bin/printbridge` | LaunchAgent `~/Library/LaunchAgents/dev.printbridge.agent.plist` | `~/Library/Logs/PrintBridge/printbridge.log` |
| Linux | `~/.local/bin/printbridge` | systemd user unit `~/.config/systemd/user/printbridge.service` | `journalctl --user -u printbridge -f` |

On Linux, user services stop when you log out unless lingering is on:
`sudo loginctl enable-linger $USER`.

### Windows

In PowerShell:

```powershell
.\install.ps1 -Binary .\printbridge.exe -AllowedOrigins "https://app.example.com"
```

This copies the binary to `%LOCALAPPDATA%\PrintBridge\` and registers a per-user scheduled task
named **PrintBridge**. The task starts the agent hidden at logon and restarts it if it exits.
If script execution is blocked, run it once with
`powershell -ExecutionPolicy Bypass -File .\install.ps1 ...`.

### Check it's running

```bash
curl http://127.0.0.1:9631/api/health
# {"arch":"aarch64","name":"PrintBridge","ok":true,"platform":"macos","version":"1.0.0"}
```

## Build from source

Requirements: [Rust](https://rustup.rs) 1.88 or newer, and a C compiler. The TLS library (`aws-lc-rs`)
compiles some C:

- **macOS:** `xcode-select --install`
- **Windows:** Visual Studio Build Tools with the "Desktop development with C++" workload
- **Linux:** `build-essential` (Debian/Ubuntu) or `gcc` and `make`

```bash
git clone https://github.com/glennlaysonjr/printbridge.git
cd printbridge
cargo build --release
./target/release/printbridge --help
```

The binary is at `target/release/printbridge` (`printbridge.exe` on Windows). Or install it
onto your `PATH` with `cargo install --path .`.

## Uninstall

```bash
./install.sh --uninstall          # macOS / Linux
```

```powershell
.\install.ps1 -Uninstall          # Windows
```

If you didn't use the installers, stop the process and delete the binary. PrintBridge writes
nothing else to disk, apart from short-lived temp files while it spools a job.
