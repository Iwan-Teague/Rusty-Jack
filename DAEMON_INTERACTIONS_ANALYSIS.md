# Daemon interactions & feature gap analysis

Status: Audit of `rustyjackd` (the Rust daemon) and how it interacts with other components (core, UI, scripts, systemd, hardware). This document summarizes findings, obvious logic gaps, and prioritized implementation proposals with concrete steps and references to the codebase.

Summary
-------
- Overall the daemon is well-structured: it uses a Unix socket with an authenticated RPC-style protocol, has a robust job manager with cancellation and progress, interacts with core services via job tasks, integrates with systemd (socket activation, sd_notify, watchdog), and has proper authorization tiers.
- Key gaps / opportunities:
  - No reactive integration with hotplug/driver installer events (udev installer writes files in /tmp but the daemon doesn't watch or get notified automatically)
  - No netlink/udev subscription inside the daemon to detect interface add/remove in realtime (relying on manual 'Hardware Detect')
  - Lack of an event pubsub model for clients to subscribe to system events (new interface, installer results, job progress pushes)
  - Limited health checks / metrics exposure (just basic Health endpoint and logging)
  - Jobs and job progress are in-memory only (no persistence across restarts)

What I inspected
----------------
- Daemon core files and features reviewed:
  - `rustyjack-daemon/src/main.rs` — startup, systemd integration, shutdown
  - `rustyjack-daemon/src/server.rs` — UDS server, handshake, request loop, timeouts, authorization
  - `rustyjack-daemon/src/dispatch.rs` — mapping RPC endpoints to core services and jobs
  - `rustyjack-daemon/src/jobs` — job manager, kinds, locking policy, cancellation
  - `rustyjack-daemon/src/state.rs` — state, job manager instantiation
  - `rustyjack-daemon/src/systemd.rs` — socket activation, sd_notify, watchdog, and related helper functions
  - `scripts/wifi_hotplug.sh` and `scripts/wifi_driver_installer.sh` — hotplug + driver installer
  - `rustyjack-core/src/operations.rs` — user-facing operations the daemon invokes via jobs

Findings (detailed)
-------------------
1. Hotplug & driver installer coordination
   - The udev hotplug rule runs `scripts/wifi_hotplug.sh` which writes small JSON event files to `/tmp/rustyjack_wifi_event` and starts the driver installer in background.
   - The driver installer writes `/tmp/rustyjack_wifi_result.json` on completion and may POST to a Discord webhook.
   - The daemon *does not* watch `/tmp/rustyjack_wifi_event` or the result file; the core operation `system_install_wifi_drivers` runs the installer synchronously and reads the result file only when that explicit RPC is invoked.
   - Effect: automatic hardware changes aren't pushed to clients; users must trigger `Hardware Detect` manually to see a new interface.

2. No netlink / uevent subscription inside daemon
   - The codebase uses netlink functionality for many operations, but the daemon does not maintain a background netlink listener to react to interface add/remove / address changes.
   - Benefit of a listener: real-time updates, better UX, immediate re-evaluation of preferred/default routes, immediate enforcement of isolation when needed.

3. Missing event pub/sub in IPC
   - The IPC protocol supports Event responses in the type system, but the current server implementation is request/response only — there is no mechanism to push spontaneous events to connected clients nor a subscription API.
   - Job progress is stored and accessible via `JobStatus` calls, but clients cannot subscribe for live progress updates (other than polling).

4. Health & metrics
   - There is a `Health` endpoint and systemd readiness/watchdog integration, but the health check is basic (ok/up). There is no detailed health check that probes critical dependencies (netlink responsiveness, ability to claim GPIO lines, /dev/spidev access, or connectivity to NetworkManager) and no metrics endpoint (e.g., Prometheus) for monitoring.

5. Jobs persistence and auditing
   - JobManager keeps job records in-memory with a configurable retention policy (works fine for runtime observability) but there is no persistence for job history across restarts. This may be acceptable, but useful features (postmortem job logs, audits) are missing.

6. Security and authorization observations
   - Authorization is PID/group based using /proc/<pid>/status group parsing; this is pragmatic and functional, but could be noisy with transient client processes.
   - Socket ownership and group are configurable; systemd socket activation code sets group on socket file when configured. Good practice.

Recommendations (prioritized and actionable)
-------------------------------------------
I recommend three phases: short-term (low-risk), medium-term (feature additions), and long-term (architectural improvements).

Short-term (low risk, quick wins)
- 1. Add a small file watcher background task (in daemon) that watches `/tmp/rustyjack_wifi_result.json` and `/tmp/rustyjack_wifi_event` and triggers a `Hardware Detect` run and optionally emits an event to clients when changed.
   - Why: quick detection of installer results without changing scripts.
   - How: use `notify` crate (tokio-compatible) or a simple periodic poll if portability is a concern. On change: call `handle_hardware_detect()` or dispatch a `HardwareCommand::Detect` internally.

- 2. Modify `wifi_driver_installer.sh` to actively notify the daemon via a small UDS RPC call when complete (e.g., run `rustyjackctl notify hotplug ...`), or ship a simple shell helper that opens the socket and sends a minimal request.
   - Why: deterministic notification without relying on file IO semantics.
   - How: add a tiny subcommand in `rustyjackctl` (if present) or a small rust binary in `/opt/rustyjack/bin/hotplug-notify` that posts a `HotplugEvent` or triggers `system_install_wifi_drivers` RPC.

Medium-term (improve UX and capabilities)
- 3. Implement netlink event subscription in the daemon
   - Why: get interface up/down/add/remove and address changes in real-time and proactively adjust preferred interface, enforce isolation, and notify UI.
   - How: Use `rustyjack_netlink::WirelessManager` or directly `netlink` crate to subscribe to RTMGRP_LINK/RTMGRP_IPV4_IFADDR events; run as a background Tokio task that posts internal events and triggers `select_active_uplink()` or `handle_hardware_detect()` as appropriate.

- 4. Add an IPC subscription API (pub/sub) so clients can subscribe to events
   - Why: UI can update immediately; remote clients can react without polling.
   - How: Add `Endpoint::Subscribe` where a client opens a long-lived connection and the server sends `ResponseBody::Event` frames when events occur (interface added, driver install result, job progress). Implement a lightweight subscription registry in `DaemonState` to track subscribers and broadcast events. Use the existing frame format.

- 5. Emit job progress events to subscribers
   - Why: Live progress in UI without aggressive polling.
   - How: When `JobManager::update_progress` runs, push an event to subscribers (with job_id, percent, phase, message). Provide a `JobSubscribe` option per-job as well.

Long-term (architecture and reliability)
- 6. Add persistence for job history/audit (optional)
   - Why: For forensic audits and debugging across restarts.
   - How: Persist finished job metadata and results (JSON) to a configurable directory (e.g., `/var/lib/rustyjack/jobs`) and make `JobManager` load recent entries at startup (honoring retention policy). Keep logs small and rotate.

- 7. Health checks & metrics
   - Add richer health probes (netlink availability, ability to talk to NetworkManager, check that GPIO/SPI can be claimed for the UI path) and a metrics endpoint (Prometheus) or push to a metrics sink.

- 8. Consider an event bus for internal coordination
   - A small internal event bus (channel-based) decouples components: netlink watcher, hotplug file watcher, job manager, dispatch layer and subscriber broadcaster.

Design notes & example integration sketches
-----------------------------------------
- Netlink watcher sketch:
  - Spawn a tokio task at startup: `tokio::spawn(netlink_watcher(state.clone()));`
  - `netlink_watcher` subscribes to link/address events; on relevant event call `handle_hardware_detect()` or directly mutate a new `last_interface_event` timestamp, then broadcast `Event::InterfaceChanged`.

- IPC subscription API sketch:
  - New Endpoint: `Subscribe(SubscribeRequest { topics: Vec<Topic> })`.
  - Client opens a connection, sends `Subscribe`, receives ack with subscription id.
  - Server keeps the stream open and writes `ResponseEnvelope` frames with `ResponseBody::Event(EventPayload)` when events occur.
  - Topics: `Interface`, `Hotplug`, `Installer`, `JobProgress`, `HotspotWarning`.

- Hotplug notifier sketch (installer side):
  - Installer calls the daemon via UDS: `POST /hotplug?status=SUCCESS&interfaces=wlan1` (in protocol terms: send a `RequestBody::CustomHotplugEvent`). The daemon will validate the caller (Operator/Admin) and then run hardware detect and broadcast results.

Compatibility & security considerations
--------------------------------------
- Ensure subscriber streams have per-peer authorization: only Operator/Admin may subscribe to dangerous topics (Hotplug install results may be ok for Operator, but Admin-only for some topics).
- Avoid processing untrusted file contents directly; prefer explicit RPC notification from the installer over reading random /tmp files.

Examples to draw from (other daemons)
-------------------------------------
- NetworkManager: uses netlink to detect interface events and D-Bus to broadcast signals — draw inspiration for event-driven design and subscription semantics.
- wpa_supplicant: uses control sockets for commands and events (client can open control socket and receive events) — similar pattern for job progress and system events.
- systemd: uses socket activation and sd_notify — already used in code; consider using systemd unit transient states to record important lifecycle events.

Tests & validation plan
-----------------------
1. Add unit tests for the small file watcher and installer-notify handling (simulate file creation and assert daemon triggers `handle_hardware_detect`).
2. Integration test: simulate netlink link add/remove (requires root capability in test or a mock netlink interface) and assert daemon broadcasts `InterfaceChanged` event to a subscribed test client.
3. End-to-end: attach a supported USB adapter, verify driver installer runs, and ensure the UI sees the new interface without manual UI action.

Next steps
----------
- I can open PRs for any of the recommended items. Suggested order:
  1. Add a small file watcher for installer events + a simple event broadcast (low risk)
  2. Add an RPC for installer to notify the daemon directly (small change to installer script + daemon dispatch)
  3. Implement netlink watcher and subscription-based event broadcasting
  4. Add job persistence and richer health/metrics

If you'd like, I can implement the first two (file watch + installer notify) as a single small PR to demonstrate the pattern and provide infra for the larger changes.

-- End of analysis
