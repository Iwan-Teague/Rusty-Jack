<#
Run daemon tests on Windows.

This script runs unit tests (cargo test -p rustyjack-daemon). If WSL is present,
it can optionally run the macOS/Linux integration script inside WSL to exercise
the UDS-based integration tests (Unix domain sockets are not fully supported on
native Windows for the daemon binary).
#>
param(
    [switch]$UseWsl
)

Write-Host "Running cargo test (daemon unit tests)"
cargo test -p rustyjack-daemon

if ($UseWsl) {
    Write-Host "Running integration tests in WSL (requires WSL and a Linux distro)"
    wsl bash -lc "cd /mnt/c/$(get-location).Path -P && ./tests/daemon/run_daemon_tests.sh"
} else {
    Write-Host "Delegating to tests/daemon runner for consistency"
    & "$(Split-Path -Parent $MyInvocation.MyCommand.Path)\daemon\run_daemon_tests.ps1" @args
}
