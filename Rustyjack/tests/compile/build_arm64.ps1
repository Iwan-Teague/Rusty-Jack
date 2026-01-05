#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..\..") | Select-Object -ExpandProperty Path
$Target = "aarch64-unknown-linux-gnu"
$DefaultBuild = $false

if ($args.Count -gt 0) {
    $ContainerArgs = $args
} else {
    $DefaultBuild = $true
    $ContainerArgs = @(
        "bash",
        "-c",
        "set -euo pipefail; " +
        "export PATH=/usr/local/cargo/bin:\$PATH; " +
        "export CARGO_TARGET_DIR=/work/target-64; " +
        "cargo build --target $Target -p rustyjack-ui; " +
        "cargo build --target $Target -p rustyjack-daemon; " +
        "cargo build --target $Target -p rustyjack-portal; " +
        "cargo build --target $Target -p rustyjack-core --bin rustyjack --features rustyjack-core/cli"
    )
}

& "$RepoRoot\docker\arm64\run.ps1" @ContainerArgs
$ExitCode = $LASTEXITCODE

if ($ExitCode -ne 0) {
    exit $ExitCode
}

if ($DefaultBuild) {
    $DestDir = Join-Path $RepoRoot "prebuilt\arm64"
    if (-not (Test-Path $DestDir)) {
        New-Item -ItemType Directory -Path $DestDir | Out-Null
    }

    $Bins = @("rustyjack-ui", "rustyjackd", "rustyjack-portal", "rustyjack")
    foreach ($bin in $Bins) {
        $Src = Join-Path $RepoRoot "target-64\$Target\debug\$bin"
        if (-not (Test-Path $Src)) {
            Write-Error "Missing binary: $Src"
            exit 1
        }
        Copy-Item -Force $Src (Join-Path $DestDir $bin)
    }
    Write-Host "Copied binaries to $DestDir"
}
exit 0
