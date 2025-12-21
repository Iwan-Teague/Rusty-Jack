# Build Rustyjack for 32-bit ARM (Pi Zero 2 W on 32-bit Pi OS) inside the arm32 container.
# Requires Docker Desktop with binfmt/qemu enabled.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir

Set-Location $RepoRoot

Write-Host "Building ARM32 (armv7) target using Docker..." -ForegroundColor Cyan

# Build the Docker image and run cargo build
& "$RepoRoot\docker\arm32\run.ps1" env CARGO_TARGET_DIR=/work/target-32 cargo build --target armv7-unknown-linux-gnueabihf -p rustyjack-ui

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nBuild successful! Binary located at: target-32/armv7-unknown-linux-gnueabihf/debug/rustyjack-ui" -ForegroundColor Green
} else {
    Write-Host "`nBuild failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}
