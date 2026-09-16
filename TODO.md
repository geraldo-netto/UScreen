# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T148 | open | medium | medium | Serialize and reconcile keyboard suppression across touch-device changes. `input.rs::osk_touch_device_added/removed` and `DeviceOwner::drop` update an atomic count but independently await/spawn `osk::disable/restore`; detach during an in-flight disable can restore before the backup exists, then leave the keyboard suppressed with no tablets. Add a permanent controlled-future regression reproducing detach during suppression and concurrent attach/detach, then ensure the final applied state follows current device count and shutdown restoration shares serialization. |
| T141 | blocked | medium | small | Resolve installer desktop-path inconsistency: `make install` quotes the GUI executable in its desktop entry, but `scripts/install.sh::install_files` emits an unquoted Exec path. Installing under a home directory containing spaces creates a broken app-menu launcher. Unblock by correctly encoding the executable path in both entry points; add a permanent sandboxed regression that launches the generated entry from a path with spaces and reserved desktop-entry characters. |
