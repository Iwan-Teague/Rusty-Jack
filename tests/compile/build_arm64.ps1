# Build Rustyjack for 64-bit ARM (aarch64) target using Docker — placed under tests/compile

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent (Split-Path -Parent $ScriptDir)

Set-Location $RepoRoot

Write-Host "Building ARM64 (aarch64) target using Docker..." -ForegroundColor Cyan

& "$RepoRoot\docker\arm64\run.ps1" env CARGO_TARGET_DIR=/work/target-64 cargo build --target aarch64-unknown-linux-gnu -p rustyjack-ui -p rustyjack-daemon -p rustyjack-portal --bin rustyjack --features rustyjack-core/cli

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nBuild successful. Copying binaries to prebuilt\arm64..." -ForegroundColor Green

    $TargetDir = Join-Path $RepoRoot "target-64\aarch64-unknown-linux-gnu\debug"
    $PrebuiltDir = Join-Path $RepoRoot "prebuilt\arm64"
    $Bins = @("rustyjack-ui", "rustyjackd", "rustyjack-portal", "rustyjack")

    New-Item -ItemType Directory -Force -Path $PrebuiltDir | Out-Null
    foreach ($bin in $Bins) {
        $src = Join-Path $TargetDir $bin
        $dst = Join-Path $PrebuiltDir $bin
        if (Test-Path $src) {
            Copy-Item $src $dst -Force
        }
    }
} else {
    Write-Host "`nBuild failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}
