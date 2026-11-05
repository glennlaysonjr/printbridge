#!/bin/sh
# Installs PrintBridge for the current user and starts it at login.
#   macOS: ~/.local/bin/printbridge + LaunchAgent ~/Library/LaunchAgents/dev.printbridge.agent.plist
#   Linux: ~/.local/bin/printbridge + systemd user unit ~/.config/systemd/user/printbridge.service
#
# Usage:
#   ./install.sh [path/to/printbridge]          install (binary defaults to ./printbridge)
#   ./install.sh --uninstall                    stop and remove
#
# Agent flags go in PRINTBRIDGE_* variables before installing, e.g.
#   PRINTBRIDGE_ALLOWED_ORIGINS=https://app.example.com ./install.sh
set -eu

BIN_DIR="$HOME/.local/bin"
TARGET="$BIN_DIR/printbridge"
LABEL="dev.printbridge.agent"
OS="$(uname -s)"

uninstall() {
  case "$OS" in
    Darwin)
      PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
      launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
      rm -f "$PLIST"
      ;;
    Linux)
      systemctl --user disable --now printbridge.service 2>/dev/null || true
      rm -f "$HOME/.config/systemd/user/printbridge.service"
      systemctl --user daemon-reload 2>/dev/null || true
      ;;
  esac
  rm -f "$TARGET"
  echo "PrintBridge removed."
}

if [ "${1:-}" = "--uninstall" ]; then
  uninstall
  exit 0
fi

SRC="${1:-./printbridge}"
if [ ! -f "$SRC" ]; then
  echo "Binary not found: $SRC" >&2
  echo "Usage: $0 [path/to/printbridge] | --uninstall" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
cp "$SRC" "$TARGET"
chmod 755 "$TARGET"
# Downloaded binaries are quarantined by Gatekeeper; this one was chosen explicitly.
[ "$OS" = "Darwin" ] && xattr -d com.apple.quarantine "$TARGET" 2>/dev/null || true

ORIGINS="${PRINTBRIDGE_ALLOWED_ORIGINS:-*}"
PORT="${PRINTBRIDGE_PORT:-9631}"

case "$OS" in
  Darwin)
    PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
    LOG_DIR="$HOME/Library/Logs/PrintBridge"
    mkdir -p "$(dirname "$PLIST")" "$LOG_DIR"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$TARGET</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PRINTBRIDGE_PORT</key><string>$PORT</string>
    <key>PRINTBRIDGE_ALLOWED_ORIGINS</key><string>$ORIGINS</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$LOG_DIR/printbridge.log</string>
  <key>StandardErrorPath</key><string>$LOG_DIR/printbridge.log</string>
</dict>
</plist>
EOF
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "$PLIST"
    echo "Installed. Logs: $LOG_DIR/printbridge.log"
    ;;
  Linux)
    UNIT_DIR="$HOME/.config/systemd/user"
    mkdir -p "$UNIT_DIR"
    cat > "$UNIT_DIR/printbridge.service" <<EOF
[Unit]
Description=PrintBridge local print agent
After=network-online.target

[Service]
ExecStart=$TARGET
Environment=PRINTBRIDGE_PORT=$PORT
Environment="PRINTBRIDGE_ALLOWED_ORIGINS=$ORIGINS"
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF
    systemctl --user daemon-reload
    systemctl --user enable --now printbridge.service
    echo "Installed. Logs: journalctl --user -u printbridge -f"
    ;;
  *)
    echo "Unsupported OS: $OS (use install.ps1 on Windows)" >&2
    exit 1
    ;;
esac

echo "Check: curl http://127.0.0.1:$PORT/api/health"
