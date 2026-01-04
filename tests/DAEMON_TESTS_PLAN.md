# Daemon test plan (macOS / Windows)

This document lists the tests implemented by the scripts in `tests/` and explains what they verify and how to run them.

Files
 - `tests/daemon/run_daemon_tests.sh` — macOS / Linux integration test script (builds daemon, runs unit tests, launches daemon on a temporary UDS socket and runs the Python test client).
 - `tests/daemon/run_daemon_tests.ps1` — PowerShell script for Windows: runs `cargo test -p rustyjack-daemon`. Optionally runs `run_daemon_tests.sh` inside WSL if `-UseWsl` flag is provided.
 - `tests/common/test_client.py` — Python test client that implements the IPC framing protocol and runs basic scenarios: Health, Version, JobStart (Sleep).

Test list & expectations

1) Unit tests (cross-platform)
  - Command: `cargo test -p rustyjack-daemon`
  - Verifies: internal units such as `JobManager` retention logic, authorization tier mapping, and small tests already present in the daemon crate.

2) Basic IPC integration (macOS/Linux only)
  - Script: `tests/run_daemon_tests.sh`
  - Precondition: a POSIX environment that supports Unix domain sockets
  - Steps:
    - Build daemon: `cargo build -p rustyjack-daemon --release`
    - Start the daemon with `RUSTYJACKD_SOCKET=/tmp/rustyjack_test.sock`
    - Run `tests/common/test_client.py` (or `tests/daemon/test_client.py` proxy) to call:
      - `Health` endpoint: expect an OK response
      - `Version` endpoint: expect a version response with protocol_version == 1
      - `JobStart(Sleep)` followed by `JobStatus` polling: expect job to complete
  - Verifies: handshake, protocol framing, endpoint handling, job lifecycle (start -> complete)

3) Windows behavior
  - Script: `tests/run_daemon_tests.ps1`
  - Steps:
    - Run `cargo test -p rustyjack-daemon` to execute cross-platform unit tests.
    - For integration tests that require UDS and a POSIX environment, run `tests/run_daemon_tests.sh` inside WSL using `-UseWsl`.

Notes & further tests to add
- Add tests for more endpoints (WifiCapabilitiesGet, WifiInterfacesList) — these require the system to have the necessary netlink support and may be more suitable for a Linux CI environment.
- Add negative tests: malformed frames, handshake timeouts, insufficient authorization (simulate a restricted UID by running tests as a normal user — current auth fallback grants Operator to non-root processes so test accordingly).
- Consider adding a test that verifies installer notification flow by simulating the `/tmp/rustyjack_wifi_result.json` content and observing daemon response (requires implementing watcher in daemon — currently the script-based approach is manual).

How to run

On macOS/Linux:
- Ensure Python 3 is installed.
- From the repo root: `./tests/daemon/run_daemon_tests.sh`

On Windows (PowerShell):
- From an elevated or normal PowerShell prompt: `.	ests\run_daemon_tests.ps1` (requires cargo in PATH)
- To run integration tests via WSL: `.	ests\run_daemon_tests.ps1 -UseWsl`
