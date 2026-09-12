#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Build and package AI-Bridge for macOS (.dmg / .zip via cargo-bundle).
# Requires macOS with Xcode command line tools.
#
# Code signing / notarization is OUT OF SCOPE for the first build but REQUIRED
# before public distribution (Developer ID + notarization). Unsigned .dmg
# triggers Gatekeeper warnings.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Building release (macOS)"
cargo build --release -p ai-bridge-ui -p ai-bridge 2>&1

if command -v cargo-bundle >/dev/null 2>&1; then
  echo "==> Bundling .app / .dmg with cargo-bundle"
  cargo bundle --release --package ai-bridge-ui 2>&1 \
    || echo "WARN: cargo-bundle failed (install: cargo install cargo-bundle)"
else
  echo "==> cargo-bundle not found; installing it"
  cargo install cargo-bundle
  cargo bundle --release --package ai-bridge-ui 2>&1
fi

echo ""
echo "Artifacts in target/release/bundle/. Config installed under ~/.config/ai-bridge/."
