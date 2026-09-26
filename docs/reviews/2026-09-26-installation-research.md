# T602: simpler installation and optional ADB

Research complete. Recommend guided setup using the **already bundled ADB** as
the first implementation, then a separate USB accessory transport prototype.
Installing outside Google Play does not inherently require ADB. Blent currently
needs it during operation, so a store release alone cannot remove it.

## Current dependency inventory

| Role | Current implementation | Replacement needed to remove ADB |
|---|---|---|
| Linux tooling | AppImage bundles ADB (`packaging/appimage/build.py`); Debian declares it; `scripts/install.sh` installs distro tools | No full Android SDK required; improve onboarding first |
| APK install/update | ADB installs the signed APK | User package installer/direct APK, or a store |
| Discovery/attachment identity | `host/src/adb_inventory.rs`, `discovery.rs`, attachment lease | Native USB/accessory discovery or authenticated network discovery |
| Launch and secret delivery | `linux_main.rs::app_launch_command` / `launch_app_using`; `common/src/android.rs` | User launch/accessory attach and a new authenticated pairing exchange |
| Video/input tunnel | `common/src/adb_reverse.rs`, `host/src/monitor/forwarding.rs`; Android `VideoReceiver.HOST=127.0.0.1` | Native bulk stream or authenticated LAN endpoint, with framing/backpressure |
| Reconnect | Lease/route repair; Wi-Fi setup calls `adb tcpip` and `adb connect` | Native transport reconnect and stale-session rejection |
| Diagnostics/camera control | DUMP-protected receivers, doctor and explicit camera adapter | In-protocol authenticated control; camera stays opt-in |

The existing Wi-Fi feature is ADB over a network. It is not an ADB-free transport.
`TokenActivity`, `TokenReceiver`, `CodecReportReceiver` and `CameraReceiver` are
protected by `android.permission.DUMP`. Removing that guard would not be an
acceptable shortcut for pairing. Linux EVDI/kernel setup remains a separate
prerequisite regardless of APK distribution or transport.

## Distribution facts checked 2026-09-26

A release APK can be downloaded directly and installed through Android's package
installer. Android 8+ uses per-source permission for unknown-app installation.
A QR code can point to the release download, but does not bypass installer
confirmation or package/signing checks. See Android's
[alternative distribution guidance](https://developer.android.com/distribute/marketing-tools/alternative-distribution).

Keep a stable Blent signing identity and monotonic version codes. Direct
self-updates need an explicit package-installer handoff or a documented manual
update flow; store delivery delegates update distribution to that store.
Neither channel grants shell privileges to the installed app.

Developer verification is changing. The current official schedule starts
September 30, 2026 in Brazil, Indonesia, Singapore and Thailand, initially
covering selected app stores; broader rollout is planned for 2027. Do not claim
that every direct APK is already globally blocked. Recheck the actual target
region/channel before publishing. ADB remains available for developer installs;
an advanced user flow and limited distribution account are documented options.
See [Google's current rollout details](https://support.google.com/android-developer-console/answer/16561738?hl=en).
No publishing, account registration or distribution policy change was performed.

## Transport comparison

| Option | Developer options | Engineering/permission cost | Evidence and tradeoff |
|---|---|---|---|
| Guided bundled ADB | USB debugging and RSA confirmation | Existing protocol; clearer device states and install action | Working Linux/tablet route; immediate onboarding improvement |
| Direct APK/store plus existing tunnel | Still needed for current runtime | Distribution only | Reduces installer friction, does not eliminate ADB |
| Android Open Accessory bulk USB | Not required by AOA | Linux libusb/udev adapter; Android accessory permission and app attach; multiplex video/input/control | This tablet reports AOA 2; streaming, charging and reconnect unvalidated |
| Authenticated LAN pairing | Not needed by a new app-native transport | New endpoint, discovery, TLS/pairing, firewall and permission handling | No cable dependency; Wi-Fi latency/power must be measured, not assumed |
| Reimplement ADB directly | Still needed | Recreates authentication, transport and maintenance burden | No user-facing removal of debugging prerequisite; reject for this goal |

AOA permits USB communication with debugging disabled. The PC initiates accessory
mode, causing re-enumeration, then uses bulk endpoints. See the
[AOSP protocol](https://source.android.com/docs/core/interaction/accessories/aoa).
Android exposes the accessory through `UsbManager`/`UsbAccessory`, with user
permission; the attached host supplies bus power. This does not guarantee enough
power to maintain charge under a running display/decoder workload. See the
[Android accessory API](https://developer.android.com/develop/connectivity/usb/accessory).

### Physical-device probe

The attached tablet advertises `android.hardware.usb.accessory` and returned
protocol **2** to a read-only libusb control transfer:
`bmRequestType=0xc0, bRequest=51, wValue=0, wIndex=0, wLength=2`, timeout 1000 ms.
The two returned bytes are a little-endian protocol version. Device observed:
VID `18d1`, PID `4ee8`; [retained result](artifacts/2026-09-26-followup/t602/aoa-query.json).
No interface was claimed; no identification strings, START_ACCESSORY request,
reset or mode switch was issued. The live ADB session remained available.
This establishes protocol-query support only, not a working Blent AOA backend.

## Concrete implementation proposal

1. Linux app opens a setup view: kernel/display readiness, cable/device state,
   debugging authorization and Blent APK state. Reuse existing doctor/discovery
   results and bundled ADB; distinguish missing cable, unauthorized device,
   missing APK and incompatible signature. Offer one explicit install/update
   action, then connect automatically through existing authenticated routes.
2. Offer direct signed APK download/QR as an alternative installer path, with
   normal Android consent. This is useful before ADB-free runtime exists, but
   label the current connection requirement accurately.
3. Prototype AOA on this tablet before a wider transport implementation. First
   test attach/permission/re-enumeration, then bounded bidirectional framing,
   authentication, backpressure/cancellation and reconnect with USB debugging
   disabled. Use app-native control in place of DUMP receiver invocation.
4. Promote AOA only after matched 1280×800 frame delivery, p95/p99 render ACKs,
   throughput, CPU, live RSS and tablet charging/thermal measurements pass.
   Keep explicit ADB fallback and an unsupported capability on other backends.
   LAN pairing is a separate optional route, not a silent fallback to an
   unauthenticated network listener.

Portable code owns attachment identity, framing, authentication state and
capability reporting. Linux owns libusb/udev; Android owns accessory lifecycle;
future Windows support needs its own driver/permission validation. macOS remains
out of scope. Required implementation regressions cover denied permissions,
wrong signer/version, unplug/replug, mode-switch failure, stale tokens, partial
I/O, malformed lengths, cancellation, route ownership and camera-off defaults.
Research closes T602; these proposed flows and transport backends are not
implemented by this documentation commit.
