# Hardware isolation spec — single active interface policy

Status: analysis and implementation plan to ensure only one network interface is active at a time (except controlled exceptions such as hotspot AP + upstream). This document describes current behavior, gaps, and a concrete plan to implement persistent single-interface enforcement in Rustyjack.

Goal
----
- Ensure that whichever interface the *user selects* (via the UI or configuration) becomes the single active interface used for all operations.
- All other interfaces must be kept down and effectively uninteractable unless explicitly allowed for an exceptional use case such as hosting a hotspot (AP) where the AP interface and an upstream may be allowed concurrently.
- Enforcement must be persistent (survives hotplug events, service restarts, and NetworkManager races) and observable (the UI reflects the enforced state).

Current behaviour (code references)
----------------------------------
- Interface enumeration and selection:
  - `rustyjack-core::system::list_interface_summaries()` scans `/sys/class/net` and reports `kind` (wireless/wired), `oper_state`, and `ip`.
  - UI 'Hardware Detect' (`rustyjack-core::operations::handle_hardware_detect`) shows interfaces and lets user set the active interface in the UI which saves to `gui_conf.json` and *calls* `ensure_route_for_interface`.
- Preference & route enforcement:
  - `handle_wifi_route_ensure()` writes `system_preferred` via `write_interface_preference(root, "system_preferred", &target_interface)` then calls `select_active_uplink()`.
  - `select_active_uplink()` runs `isolation_plan(preferred)` which calls `apply_interface_isolation(&allowed)` to bring allowed interfaces up, and bring others down (and block rfkill for blocked wireless interfaces).
- Isolation implementation:
  - `apply_interface_isolation()` enumerates `/sys/class/net`, brings allowed interfaces up, blocks rfkill for allowed wireless interfaces (unblock for allowed), brings all non-allowed interfaces down, and for non-wireless interfaces surfaces errors if they fail to bring up.
- Attack operations and some actions explicitly call `enforce_single_interface(interface)` (which calls `apply_interface_isolation`), ensuring attacks run on the requested interface.

Gaps & failure modes
--------------------
1. Startup: daemon currently does not automatically re-apply the preferred single-interface isolation at boot. `DaemonState::reconcile_on_startup()` only inspects /proc/mounts; it does not call `select_active_uplink()` or `apply_interface_isolation()` on boot.
2. Hotplug and installer notification: udev scripts write `/tmp/rustyjack_wifi_event` and the installer writes `/tmp/rustyjack_wifi_result.json`, but the daemon does not watch these files or receive proactive notifications. The user must trigger 'Hardware Detect' manually to adopt updates.
3. Network manager / external reconfiguration race: NetworkManager or other system services can re-enable interfaces after `apply_interface_isolation()` has brought them down.
4. Lack of explicit API to set the system-wide active interface: UI uses `ensure_route_for_interface()` (good) but there is no single high-level RPC named `SetActiveInterface` that unifies write preference + isolation + route verification.
5. Lack of background enforcement: currently isolation is performed on explicit actions (route ensure, job starts) but there is no background watcher to re-apply isolation on link/address events.

Design & Implementation proposal
--------------------------------
We will make the system enforce a single active interface by implementing the following changes (ordered by priority & risk):

Short-term changes (low risk)
1. Enforce on startup
   - Call `select_active_uplink()` (or `apply_interface_isolation` using `system_preferred`) from `DaemonState::reconcile_on_startup()` so the preferred interface is isolated at boot.
   - Implementation hint: add a small wrapper that calls `select_active_uplink()` and logs any errors, e.g. in `rustyjack-daemon/src/state.rs`'s `reconcile_on_startup()`.

2. Centralize 'select active interface' flow
   - Add a new convenience method / operation (if not already explicit) `set_active_interface(interface: &str)` in core or operations which:
     - Validates interface exists
     - Calls `write_interface_preference(root, "system_preferred", interface)`
     - Calls `handle_wifi_route_ensure()` or `select_active_uplink()` (so DHCP, route, IP gating runs)
     - Calls `apply_interface_isolation` (ensures non-allowed interfaces are down)
   - Update UI to call this central operation when the user selects an interface (instead of updating GUI-only config + calling ensure_route_for_interface). UI already calls `ensure_route_for_interface` so you can either ensure that function calls the new centralized behavior or make a one-line change in the UI to call `CoreDispatch(HardwareSetActive)`. Minimal change: keep UI calling `WifiRouteEnsure` but ensure that endpoint re-applies isolation (it already writes preference and calls select_active_uplink — verify and tighten per step 1).

Medium-term changes (improve resilience)
3. Add a netlink watcher to the daemon for link/address events
   - Spawn a Tokio background task in `rustyjack-daemon::main` (or `server::run`) that subscribes to link and address events using `rustyjack_netlink` or `netlink-sys`.
   - On events indicating interface up / new interface / address change, re-run `select_active_uplink()` (or re-apply `apply_interface_isolation`) to re-enforce the rule.
   - Ensure the watcher uses appropriate throttling/debounce to avoid excessive operations.
   - Implementation snippet (pseudocode):

    tokio::spawn(async move {
        let mut watcher = NetlinkWatcher::new().await?;
        loop {
           let event = watcher.next_event().await?;
           // If link up on a non-allowed iface or new iface appears
           let _ = crate::system::select_active_uplink();
        }
    });

4. Add installer/hotplug notifications (file watcher or RPC)
   - Prefer an explicit RPC: add a small endpoint `HotplugNotify` or `InstallerNotify` which the driver installer can call when it finishes (the installer scripts can do a simple UDS call to the daemon).
   - Alternatively (easier), add a file watcher in the daemon to watch `/tmp/rustyjack_wifi_result.json` and re-run `handle_hardware_detect()` and `select_active_uplink()` on change.

Long-term changes (optional / advanced)
5. Harden integration with NetworkManager
   - When applying isolation, optionally set non-allowed interfaces as unmanaged in NetworkManager so automatic re-up does not happen, or create a per-device policy to prevent NM from auto-re-enabling those interfaces. Provide this as a configurable opt-in behavior (requires nmcli/dbus usage).

6. Add test coverage
   - Unit tests for `apply_interface_isolation` and `isolation_plan` (using mocks for rfkill and netlink where possible).
   - Integration tests:
     - Simulate two interfaces, set preferred to A, assert B is down and route set to A.
     - Simulate interface hotplug: bring up B, assert watcher re-applies isolation and B is down.
     - Test hotspot exceptions: start hotspot on A or B and assert allowed set includes AP and upstream interfaces.

Detailed implementation steps
-----------------------------
1) Add enforcement on startup
   - Edit `rustyjack-daemon/src/state.rs::reconcile_on_startup()` to call `rustyjack_core::system::select_active_uplink()` after mount checks (log errors but don't fail startup).

2) Add netlink watcher task
   - Create `rustyjack-daemon/src/netlink_watcher.rs` (new file) with logic using `rustyjack_netlink` to get link/address events and call `select_active_uplink()` when relevant changes happen.
   - Spawn it from `rustyjack-daemon/src/main.rs` after `state` creation and before starting server.

3) Expose a small installation notify endpoint
   - Add an endpoint to `RequestBody` (in `rustyjack-ipc`) or reuse an existing system operation `SystemInstallWifiDrivers` that is called when the installer finishes; if installer can call the daemon via UDS (shell helper), then use that to notify the daemon to call `handle_hardware_detect()` and `select_active_uplink()`.

4) Optional: Add NM-managed policy
   - Implement in `apply_interface_isolation()` an optional code path that sets `nmcli device set <iface> managed no` for non-allowed interfaces when config says so. Provide a config flag in `DaemonConfig` to enable this behavior.

Safety and locking
------------------
- Use existing lock primitives: `select_active_uplink()` and `apply_interface_isolation()` are typically called from contexts that already serialize route changes; ensure netlink watcher and installer notifications acquire the necessary locks (use `lock_uplink()` or equivalent) when calling `select_active_uplink()` to avoid races.

User-visible details
--------------------
- The UI should show a clear indicator that interface isolation is enforced and list which interfaces are allowed/blocked.
- When the UI sets an active interface it should show the success/failure of enforcement (e.g., errors from `handle_wifi_route_ensure`) and propose to re-run detection if the device is missing.

Acceptance tests
----------------
1. Boot test: reboot or restart daemon → preferred interface remains active and others are down.
2. Select test: choose `wlan0` in UI → all other interfaces are down and WiFi operations happen only on `wlan0`.
3. Hotplug test: insert an attack-capable USB adapter → installer notifies daemon → upon user selection of new interface it becomes active and old interface is down.
4. Hotspot exception: when starting hotspot allow AP interface and upstream concurrently, then after stopping the hotspot revert to single-interface policy.

Wrap-up
-------
The core functionality to achieve single-active-interface largely exists (preference writing, `select_active_uplink`, `apply_interface_isolation`). The missing pieces are persistent enforcement (on startup), reactive enforcement (netlink/hotplug notifications), and optional NetworkManager integration to prevent external processes from re-enabling blocked interfaces.

Next step: I can implement the two highest-value items: (1) call `select_active_uplink()` from startup (`DaemonState::reconcile_on_startup`) and (2) add a simple file-watcher for `/tmp/rustyjack_wifi_result.json` that triggers `handle_hardware_detect()` and `select_active_uplink()`. Shall I open a PR for those two changes now?
