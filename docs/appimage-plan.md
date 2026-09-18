# AppImage packaging plan — T308

The maintainer selected AppImage to replace the Debian `.deb` release asset.
This is planned work, not an available artifact. Current code still builds and
publishes the Debian package; RPM, Arch and tar distributions remain in scope
unless separately changed.

## Bundle and host boundary

Reuse `scripts/stage-linux-bundle.sh` and the Debian 12 build baseline, then
assemble an AppDir containing the GUI, daemon, capture helper, replaceable
libevdi, stock FFmpeg, ADB and the required userspace libraries. Verify actual
dependency closure in a clean environment. Preserve third-party notices and
applicable library replacement/source requirements; do not patch FFmpeg.

An AppImage simplifies distribution but cannot supply a kernel module built for
every user's running kernel. EVDI/DKMS setup, GPU drivers and uinput/USB device
permissions remain host responsibilities. Reuse the existing explicit system
setup flow and prerequisite diagnostics. Keep the supported libc baseline and
graphics/backend limits documented. AppImage's [bundling guidance](https://docs.appimage.org/introduction/concepts.html)
also distinguishes bundled resources from system and graphics libraries.

## Process lifetime and launch paths

`gui/src/main.rs` currently finds a sibling daemon, then an installed executable;
`scripts/install.sh` writes fixed daemon/helper paths into a user service. These
paths need an AppImage-aware implementation, not references to a transient mount.

Provide GUI and daemon entry modes through `AppRun`. A daemon started by the
GUI must own an independent AppImage invocation, so closing the GUI cannot
retire its filesystem. Point autostart/service entries to a stable installed
AppImage path and route stop/status through the same distribution. Keep native
installations working and prevent concurrent daemon instances during migration.

Use the runtime's [APPIMAGE/APPDIR distinction](https://docs.appimage.org/packaging-guide/environment-variables.html):
`APPIMAGE` identifies the outer file, while `APPDIR` is its current mountpoint.
Neither temporary mount paths nor shell-expanded user paths belong in persistent
service configuration. Preserve existing path quoting and bounded shutdown.

Support [extracted execution when FUSE is unavailable](https://docs.appimage.org/user-guide/troubleshooting/fuse.html#extract-and-run-type-2-appimages).
That mode also needs a stable directory for background processes; test both
lifetimes explicitly. Application state stays outside the read-only bundle.

## Delivery and acceptance

1. Add a reproducible packaging target with pinned packaging-tool inputs and
   a clean-environment dependency check. Reuse existing ABI verification.
2. Implement and test GUI/daemon launch, helper lookup, service registration,
   paths containing spaces, extraction, GUI closure and shutdown.
3. Explain migration from `.deb`, prerequisite setup, stable placement and
   updates. Do not automatically uninstall the existing package or delete
   preferences. Use Geraldo Netto's [GitHub profile](https://github.com/geraldo-netto)
   for attribution; preserve the original author's attribution separately.
4. Replace the `.deb` expectation consistently in package building, publication
   inventory, checksums, release tests, CI and website/download documentation.
   Correct retained Arch maintainer attribution without inventing an email.
5. Smoke-test the actual artifact and its dependencies in isolation. Keep active
   desktop EVDI attachment outside packaging checks while T222 is unresolved.

Only claim that a dependency is bundled after inspecting and testing the actual
artifact. AppImage does not make Windows builds available or establish support
for every Linux kernel, compositor or GPU.
