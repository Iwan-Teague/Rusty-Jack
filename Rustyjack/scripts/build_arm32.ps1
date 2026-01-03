# Build Rustyjack for 32-bit ARM (Pi Zero 2 W on 32-bit Pi OS) inside the arm32 container.
# Requires Docker Desktop with binfmt/qemu enabled.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir

Set-Location $RepoRoot

Write-Host "Building ARM32 (armv7) target using Docker..." -ForegroundColor Cyan
Write-Host "This will take several minutes on first run (downloading and building Docker image)..." -ForegroundColor Yellow

# Check if Docker is accessible
Write-Host "Checking Docker availability..." -ForegroundColor Cyan
$dockerJob = Start-Job -ScriptBlock { docker version --format '{{.Server.Version}}' 2>&1 }
$result = Wait-Job $dockerJob -Timeout 5
if ($null -eq $result) {
    Stop-Job $dockerJob
    Remove-Job $dockerJob
    Write-Host "`nERROR: Docker is not responding (timeout after 5 seconds)." -ForegroundColor Red
    Write-Host "This usually means Docker Desktop is starting up or having issues." -ForegroundColor Yellow
    Write-Host "`nSteps to fix:" -ForegroundColor Yellow
    Write-Host "  1. Restart Docker Desktop completely:" -ForegroundColor White
    Write-Host "     - Right-click Docker icon in system tray → Quit Docker Desktop" -ForegroundColor White
    Write-Host "     - Wait 10 seconds, then start Docker Desktop again" -ForegroundColor White
    Write-Host "  2. Wait until Docker fully starts (green whale icon)" -ForegroundColor White
    Write-Host "  3. If problem persists, restart Windows" -ForegroundColor White
    Write-Host "  4. Run this script again: .\scripts\build_arm32.ps1" -ForegroundColor White
    exit 1
}
$output = Receive-Job $dockerJob
Remove-Job $dockerJob
if ([string]::IsNullOrWhiteSpace($output)) {
    Write-Host "`nERROR: Docker is not running properly." -ForegroundColor Red
    Write-Host "Please ensure Docker Desktop is installed and running." -ForegroundColor Yellow
    Write-Host "`nSteps to fix:" -ForegroundColor Yellow
    Write-Host "  1. Download Docker Desktop: https://www.docker.com/products/docker-desktop" -ForegroundColor White
    Write-Host "  2. Start Docker Desktop and wait for green whale icon" -ForegroundColor White
    Write-Host "  3. Run this script again: .\scripts\build_arm32.ps1" -ForegroundColor White
    exit 1
}
Write-Host "Docker is running (version: $($output.Trim()))" -ForegroundColor Green

# Build the Docker image and run cargo build
& "$RepoRoot\docker\arm32\run.ps1" env CARGO_TARGET_DIR=/work/target-32 cargo build --target armv7-unknown-linux-gnueabihf -p rustyjack-ui -p rustyjack-core -p rustyjack-daemon

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nBuild successful. Copying binaries to prebuilt\\arm32..." -ForegroundColor Green

    $TargetDir = Join-Path $RepoRoot "target-32\armv7-unknown-linux-gnueabihf\debug"
    $PrebuiltDir = Join-Path $RepoRoot "prebuilt\arm32"
    $Bins = @("rustyjack-ui", "rustyjack-core", "rustyjackd")

    New-Item -ItemType Directory -Force -Path $PrebuiltDir | Out-Null
    foreach ($bin in $Bins) {
        $src = Join-Path $TargetDir $bin
        $dst = Join-Path $PrebuiltDir $bin
        if (Test-Path $src) {
            Copy-Item $src $dst -Force
        }
    }

    $missing = @()
    foreach ($bin in $Bins) {
        if (-not (Test-Path (Join-Path $PrebuiltDir $bin))) {
            $missing += $bin
        }
    }
    if ($missing.Count -eq 0) {
        Write-Host "Prebuilt binaries placed at prebuilt\\arm32: $($Bins -join ', ')" -ForegroundColor Green
    } else {
        Write-Host "Warning: built binaries not found to copy: $($missing -join ', ')" -ForegroundColor Yellow
    }
} else {
    Write-Host "`nBuild failed with exit code $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}
