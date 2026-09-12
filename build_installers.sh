#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Top-level installer build orchestrator for AI-Bridge.
#
# Usage:
#   ./build_installers.sh all        # all platforms available locally
#   ./build_installers.sh linux
#   ./build_installers.sh macos
#   ./build_installers.sh windows
#
# It also authors the cargo-dist CI workflow the first time you run `init`:
#   ./build_installers.sh dist-init
#
# NOTE (required before PUBLIC distribution): code signing
#   - Windows: Authenticode (.exe / .msi)
#   - macOS: Developer ID + notarization (.dmg/.app)
# Unsigned installers trigger SmartScreen / Gatekeeper warnings.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

PLATFORM="${1:-all}"

case "$PLATFORM" in
  linux)
    echo "==> Building Linux .deb / AppImage"
    bash "$ROOT/packaging/linux/build_and_package.sh"
    ;;
  macos)
    echo "==> Building macOS .dmg"
    bash "$ROOT/packaging/macos/build_and_package.sh"
    ;;
  windows)
    echo "==> Building Windows .msi"
    powershell -ExecutionPolicy Bypass -File "$ROOT/packaging/windows/build_and_package.ps1"
    ;;
  dist-init)
    echo "==> Authoring cargo-dist CI workflow"
    if command -v cargo-dist >/dev/null 2>&1; then
      cargo dist init --yes --config "$ROOT/packaging/cargo-dist.toml" --force
    else
      echo "cargo-dist not found; install with: cargo install cargo-dist"
      exit 1
    fi
    ;;
  all)
    bash "$ROOT/packaging/linux/build_and_package.sh"
    bash "$ROOT/packaging/macos/build_and_package.sh"
    ;;
  *)
    echo "Usage: $0 {linux|macos|windows|dist-init|all}"
    exit 1
    ;;
esac

echo ""
echo "==> Installer build complete. See per-platform scripts for details."
