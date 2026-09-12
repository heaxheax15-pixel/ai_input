# AI-Bridge Packaging

This directory contains the build/installer tooling for distributing the
AI-Bridge desktop admin UI, gatekeeper daemon, and core as installable packages
on all three major platforms.

## Quick start

From the workspace root:

```bash
./build_installers.sh all          # Linux + macOS locally
./build_installers.sh dist-init     # author the cargo-dist CI workflow
```

Platform-specific scripts:

| Platform | Script | Output |
|----------|--------|--------|
| Linux    | `packaging/linux/build_and_package.sh` | `.deb` (cargo-deb), `.AppImage` / `.tar.gz` (cargo-dist) |
| macOS    | `packaging/macos/build_and_package.sh` | `.dmg` (cargo-bundle) |
| Windows  | `packaging/windows/build_and_package.ps1` | `.msi` (cargo-wix) |

`cargo-dist` (see `packaging/cargo-dist.toml`) is the recommended way to automate
all three platforms from CI. Run `cargo dist init --yes --config packaging/cargo-dist.toml`
to generate the GitHub Actions / CI workflow.

## What gets installed

All three binaries are bundled into a **single installable package** for the
MVP (simpler ops than separate packages):

- `ai-bridge` — the gatekeeper daemon
- `ai-bridge-gatekeeper-core` — the policy engine (library, linked into the above)
- `ai-bridge-ui` — the desktop operator dashboard / admin UI

Install locations:

- **Linux**: `/opt/ai-bridge/` (not raw `/usr/bin`); config in `~/.config/ai-bridge/`
- **macOS**: `.app` bundle under `/Applications`; config in `~/.config/ai-bridge/`
- **Windows**: Program Files; config in `%APPDATA%\ai-bridge\`

## Config directory & upgrades

The installer creates the default config directory and seeds default files
(`allowlist.toml`, `executor_allowlist.toml`, `roles.toml`,
`symbol_policy.toml`, `criteria.toml`, `sockets.toml`, `branches.toml`) **only
if they do not already exist**. Existing operator configuration is **never
silently overwritten** on upgrade. Audit logs are stored separately under
`~/.config/ai-bridge/audit/`.

## systemd / launch agent / service (opt-in, default off)

For the MVP, launching is manual via the desktop app by default.

- **Linux**: set `AI_BRIDGE_INSTALL_SERVICE=1` before install to enable the
  systemd unit (`packaging/linux/ai-bridge-daemon.service`, installed to
  `/etc/systemd/system/`).
- **macOS** / **Windows** services are intentionally not auto-installed.

## ⚠️ Code signing (REQUIRED before public distribution)

The first build is **unsigned**, which means Windows SmartScreen and macOS
Gatekeeper will show warnings when users install it. Before any public/wide
distribution you MUST add:

- **Windows**: an Authenticode code-signing certificate (SignTool / ci)
- **macOS**: an Apple Developer ID cert + notarization (notarytool)

Configure these in `cargo-dist.toml` (`windows_signing_ceremonies` /
macOS notarization) or via your CI secrets.
