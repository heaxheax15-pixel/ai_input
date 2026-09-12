#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Configure the AI-Bridge config directory during Linux install.
#
# - Creates ~/.config/ai-bridge/ if it does not exist.
# - Seeds default config files ONLY IF they are not already present, so an
#   upgrade NEVER overwrites an existing operator configuration.
# - Optionally installs the systemd service (AI_BRIDGE_INSTALL_SERVICE=1).
# ---------------------------------------------------------------------------
set -euo pipefail

CONFIG_DIR="${AI_BRIDGE_CONFIG_DIR:-$HOME/.config/ai-bridge}"
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/config"

echo "==> Ensuring config directory exists: $CONFIG_DIR"
mkdir -p "$CONFIG_DIR"
mkdir -p "$CONFIG_DIR/audit"

# Seed defaults only if missing (never overwrite existing user config).
for f in allowlist.toml executor_allowlist.toml roles.toml symbol_policy.toml criteria.toml sockets.toml branches.toml; do
  if [[ ! -f "$CONFIG_DIR/$f" && -f "$SOURCE_DIR/$f" ]]; then
    echo "    seeding $f"
    cp "$SOURCE_DIR/$f" "$CONFIG_DIR/$f"
  else
    echo "    keeping existing $f"
  fi
done

if [[ "${AI_BRIDGE_INSTALL_SERVICE:-0}" == "1" ]]; then
  echo "==> Installing systemd service (opt-in)"
  SERVICE="$(dirname "${BASH_SOURCE[0]}")/ai-bridge-daemon.service"
  sed "s/%USER%/$USER/g; s/%GROUP%/$(id -gn)/g" "$SERVICE" | sudo tee /etc/systemd/system/ai-bridge-daemon.service >/dev/null
  sudo systemctl daemon-reload
  sudo systemctl enable --now ai-bridge-daemon || echo "WARN: could not start service"
else
  echo "==> systemd service NOT installed (opt-in). Launch via the desktop app or run the daemon manually."
fi

echo "==> Done. Config ready at $CONFIG_DIR"
