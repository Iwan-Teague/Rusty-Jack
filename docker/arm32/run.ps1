# PowerShell wrapper for running Docker container for ARM32 cross-compilation
# Requires Docker Desktop with binfmt/qemu enabled for ARM emulation

$ErrorActionPreference = "Stop"

$ImageName = "rustyjack/arm32-dev"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..\..") | Select-Object -ExpandProperty Path

# Default to bash if no arguments provided
if ($args.Count -eq 0) {
    $ContainerArgs = @("bash")
} else {
    $ContainerArgs = $args
}

Write-Host "Building Docker image: $ImageName" -ForegroundColor Cyan
docker build --pull --platform linux/arm/v7 -t $ImageName $ScriptDir

if ($LASTEXITCODE -ne 0) {
    Write-Host "Docker build failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}

# Ensure tmp directory exists
$TmpDir = Join-Path $RepoRoot "tmp"
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Path $TmpDir | Out-Null
}

Write-Host "Running Docker container..." -ForegroundColor Cyan
docker run --rm -it --platform linux/arm/v7 `
    -v "${RepoRoot}:/work" -w /work `
    -e TMPDIR=/work/tmp `
    $ImageName `
    $ContainerArgs

exit $LASTEXITCODE
