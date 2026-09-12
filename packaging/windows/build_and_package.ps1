# Build and package AI-Bridge for Windows (.msi via cargo-wix).
# Code signing (Authenticode) is REQUIRED before public distribution.
param(
    [switch]$SkipInstall = $false
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..").Path
Push-Location $root

Write-Host "==> Building release (Windows)"
cargo build --release -p ai-bridge-ui -p ai-bridge

$wix = Get-Command cargo-wix -ErrorAction SilentlyContinue
if (-not $wix) {
    if ($SkipInstall) { throw "cargo-wix not installed" }
    Write-Host "==> Installing cargo-wix"
    cargo install cargo-wix
}

Write-Host "==> Building .msi with cargo-wix"
cargo wix --package ai-bridge-ui --nocapture

Write-Host "Artifacts in target/wix/. Config installed under %APPDATA%\ai-bridge\."
Pop-Location
