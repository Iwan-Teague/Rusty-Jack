#!/usr/bin/env bash
set -euo pipefail

# Run daemon tests (macOS / Linux)
# - Builds tidy
# - Runs unit tests for daemon
# - Runs an integration test by launching the daemon on a temporary UDS socket and exercising health/version/job-start via Python client

exec "$(dirname "$0")/daemon/run_daemon_tests.sh" "$@"
