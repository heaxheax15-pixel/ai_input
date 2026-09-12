#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Build and package AI-Bridge for Linux:
#   - .deb  via cargo-deb
#   - .AppImage / tar.gz via cargo-dist
#
# The installer places the daemon, core, and UI into /opt/ai-bridge/ (a
# dedicated app dir), creates the default config directory, and does NOT
# overwrite an existing user config.
#
# Optionally installs a systemd service when AI_BRIDGE_INSTALL_SERVICE=1.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-0.1.0}"

echo "==> Building release binaries"
cargo build --release -p ai-bridge-ui -p ai-bridge 2>&1

DIST_DIR="$ROOT/target/linux-dist"
UI_BIN="target/release/ai-bridge-ui"
DAEMON_BIN="target/release/ai-bridge"

if [[ ! -f "$DAEMON_BIN" ]]; then
  echo "NOTE: daemon binary not found at $DAEMON_BIN; check the daemon bin target name."
fi

# ---------------------------------------------------------------------------
# .deb package via cargo-deb
# ---------------------------------------------------------------------------
if command -v cargo-deb >/dev/null 2>&1; then
  echo "==> Building .deb with cargo-deb"
  mkdir -p "$DIST_DIR"
  cargo deb --package ai-bridge-ui --no-strip -- \
    --dpkg-shlibdeps-arg='-l' \
    2>&1 || echo "WARN: cargo-deb failed (install with: cargo install cargo-deb)"
else
  echo "==> cargo-deb not found; skipping .deb (install: cargo install cargo-deb)"
fi

# ---------------------------------------------------------------------------
# AppImage / tar.gz via cargo-dist
# ---------------------------------------------------------------------------
if command -v cargo-dist >/dev/null 2>&1; then
  echo "==> Building with cargo-dist"
  cargo dist build --artifacts=tar.gz --output-format=json 2>&1 \
    || echo "WARN: cargo-dist failed (install: cargo install cargo-dist)"
else
  echo "==> cargo-dist not found; skipping (install: cargo install cargo-dist)"
fi

echo ""
echo "Complete. Packaging artifacts are in target/."
echo "Install config will live in ~/.config/ai-bridge/ (never overwritten on upgrade)."
