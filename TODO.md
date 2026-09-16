# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T141 | blocked | medium | small | Resolve installer desktop-path inconsistency: `make install` quotes the GUI executable in its desktop entry, but `scripts/install.sh::install_files` emits an unquoted Exec path. Installing under a home directory containing spaces creates a broken app-menu launcher. GLib/GIO also rejects a percent-escaped executable name before expanding its field codes, despite the Desktop Entry specification requiring `%%`. Unblock by correctly encoding the path in both entry points and using a launcher whose executable name needs no field expansion; add a permanent sandboxed regression that launches the generated entry from a path with spaces and reserved desktop-entry characters. |
| T150 | blocked | low | small | Resolve remaining guidance contradictions: `CONTRIBUTING.md` and `TouchCapture.kt::sendRendered` call the daemon metric end-to-end latency although `docs/benchmarks.md` excludes capture and encoding; website/llms update guidance omits Android’s independent switch. Unblock by aligning these descriptions with measured boundaries and separate host/tablet settings. |
