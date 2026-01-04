param(
    [switch]$UseWsl
)

Write-Host "Running cargo test (daemon unit tests)"
cargo test -p rustyjack-daemon

if ($UseWsl) {
    Write-Host "Running integration tests in WSL (requires WSL and a Linux distro)"
    wsl bash -lc "cd /mnt/c/$(get-location).Path -P && ./tests/daemon/run_daemon_tests.sh"
} else {
    Write-Host "Skipping integration tests. To run them use -UseWsl (requires WSL)"
}
