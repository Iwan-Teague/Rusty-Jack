# PowerShell wrapper for running Docker container for ARM64 cross-compilation
# Requires Docker Desktop with binfmt/qemu enabled for ARM emulation

$ErrorActionPreference = "Stop"

$ImageName = "rustyjack/arm64-dev"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..\..") | Select-Object -ExpandProperty Path

# Check if Docker is accessible (with timeout)
Write-Host "Checking Docker availability..." -ForegroundColor Cyan
$dockerJob = Start-Job -ScriptBlock { docker version --format '{{.Server.Version}}' 2>&1 }
$result = Wait-Job $dockerJob -Timeout 5
if ($null -eq $result) {
    Stop-Job $dockerJob
    Remove-Job $dockerJob
    Write-Host "`nWARNING: Docker is not responding quickly." -ForegroundColor Yellow
    Write-Host "Proceeding anyway - Docker build may fail if Docker isn't ready." -ForegroundColor Yellow
} else {
    $output = Receive-Job $dockerJob
    Remove-Job $dockerJob
    if (-not [string]::IsNullOrWhiteSpace($output)) {
        Write-Host "Docker is running (version: $output)" -ForegroundColor Green
    }
}

# Default to bash if no arguments provided
if ($args.Count -eq 0) {
    $ContainerArgs = @("bash")
} else {
    $ContainerArgs = $args
}

Write-Host "Building Docker image: $ImageName" -ForegroundColor Cyan
docker build --pull --platform linux/arm64 -t $ImageName $ScriptDir

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
docker run --rm -it --platform linux/arm64 `
    -v "${RepoRoot}:/work" -w /work `
    -e TMPDIR=/work/tmp `
    $ImageName `
    $ContainerArgs

exit $LASTEXITCODE
