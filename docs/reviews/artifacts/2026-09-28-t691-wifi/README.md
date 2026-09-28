# T691 Wi-Fi evidence

Fresh Linux and ordinary-user native Windows collections pass every scoped
function at >=80% executable lines: Linux 85, Windows 93. Full host library,
Linux CLI/unit + Wi-Fi integration, and Windows host unit/integration suites pass.
Two existing opt-in benchmark ignores remain unchanged. GNU Windows cross-check
passes. Complexity: 6,679 functions, none above 9.

Retained failing-first regressions reproduce false reconnect success, alias
migration cleanup debt, route ownership after physical identity replacement,
and Windows `wifi --off` with missing ADB. The existing T525 native daemon
inventory-disappearance test also caught a cleanup regression; reconfirming the
still-reachable physical identity restores its unchanged assertion. Full green
logs include all these tests. New bounded invalid-address/inventory, configuration
change, reconnect, transport and ownership tests remain in the normal suites.

Replay the decompressed counters with scripts/coverage/report.py check and the
retained manifest. Scopes: host/src/wifi, host/src/usb,
host/src/transport.rs, host/src/tray_state.rs,
host/src/monitor/launch_policy.rs. Native Windows additionally covers
host/src/windows_main.rs; supply sources.json and prefix C:\src\blent.
Use the matching --rust-platform. SHA256SUMS hashes the compressed artifacts.

Native fixtures own their ADB executable, stores, sockets and routes. This is
not physical tablet/network acceptance; T522 remains blocked on that hardware.
See ../../2026-09-28-windows-wifi.md for implemented behavior and limits.
