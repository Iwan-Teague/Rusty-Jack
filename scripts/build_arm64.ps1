# Build Rustyjack for 64-bit ARM (Pi Zero 2 W on 64-bit Pi OS / other ARM64 Pis) inside the arm64 container.
# Requires Docker Desktop with binfmt/qemu enabled.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir

Set-Location $RepoRoot

Write-Host "Building ARM64 (aarch64) target using Docker..." -ForegroundColor Cyan

# Build the Docker image and run cargo build
& "$RepoRoot\docker\arm64\run.ps1" env CARGO_TARGET_DIR=/work/target-64 cargo build --target aarch64-unknown-linux-gnu -p rustyjack-ui

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nBuild successful! Binary located at: target-64/aarch64-unknown-linux-gnu/debug/rustyjack-ui" -ForegroundColor Green
} else {
    Write-Host "`nBuild failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}
