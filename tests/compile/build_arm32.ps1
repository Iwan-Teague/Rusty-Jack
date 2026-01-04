# Build Rustyjack for 32-bit ARM (Pi Zero 2 W on 32-bit Pi OS) inside the arm32 container.
# Requires Docker Desktop with binfmt/qemu enabled.

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent (Split-Path -Parent $ScriptDir)

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
    exit 1
}
$output = Receive-Job $dockerJob
Remove-Job $dockerJob
if ([string]::IsNullOrWhiteSpace($output)) {
    Write-Host "`nERROR: Docker is not running properly." -ForegroundColor Red
    exit 1
}
Write-Host "Docker is running (version: $($output.Trim()))" -ForegroundColor Green

& "$RepoRoot\docker\arm32\run.ps1" env CARGO_TARGET_DIR=/work/target-32 cargo build --target armv7-unknown-linux-gnueabihf -p rustyjack-ui -p rustyjack-daemon -p rustyjack-portal -p rustyjack-core --features rustyjack-core/cli

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nBuild successful. Copying binaries to prebuilt\arm32..." -ForegroundColor Green

    $TargetDir = Join-Path $RepoRoot "target-32\armv7-unknown-linux-gnueabihf\debug"
    $PrebuiltDir = Join-Path $RepoRoot "prebuilt\arm32"
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
