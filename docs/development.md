# Development

## Building from source

These instructions target the Linux host. The [Windows plan](windows-port.md)
is proposed work, not a supported build procedure. Use the fork checkout and
record `git rev-parse HEAD` when sharing results; version 1.2.3 alone does not
identify its unreleased changes.

Host build needs stable Rust/Cargo, a C/C++ compiler, make, pkg-config,
libdrm headers, the **libevdi userspace development library** (including the
unversioned `libevdi.so` linker name), and the GUI platform headers below.
The evdi kernel module alone cannot satisfy the helper's `-levdi` link.

On Debian 12 / Ubuntu, install the compiler and GUI prerequisites:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config git curl ca-certificates \
  libdrm-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev libssl-dev
# Install stable Rust with rustup if cargo/rustc are not already available.
```

If the distribution provides `libevdi-dev`, it supplies the linker library.
For Debian 12, or to match the bundled release library exactly, build the
pinned upstream userspace library (this does not build/load a kernel module):

```bash
git clone --depth 1 --branch v1.15.0 https://github.com/DisplayLink/evdi /tmp/uscreen-evdi
make -C /tmp/uscreen-evdi/library
sudo make -C /tmp/uscreen-evdi/library install PREFIX=/usr/local
sudo ldconfig
```

Runtime additionally needs `ffmpeg`, `adb`/`android-tools`, a compatible evdi
kernel module, and the permissions installed by `make setup-system`.
KDE output placement needs `kscreen-doctor`; KWin input mapping uses `busctl`
or a supported `qdbus` variant. X11 mapping needs
`xinput` and `xrandr`. See [installation.md](installation.md).

```bash
make build            # EVDI helper (C) + Rust daemon + GUI
make install          # copies to ~/.local/bin, installs/reloads the user unit
make setup-system     # modprobe.d / modules-load.d / udev rule (sudo)
```

These install/setup commands modify the machine. For a configured systemd
desktop, explicitly enable/start with `systemctl --user enable --now uscreen`;
`make install` does not do that. Without a user manager, launch `uscreen start`
in a terminal. See [installation](installation.md) before attaching EVDI.
Make delegates user-file installation to `scripts/install.sh --user-install`;
both routes honor absolute XDG data/config base directories, falling back to
HOME defaults for unset, empty or relative values.
The source helper normally finds the system libevdi installed above; bundling
is a separate release step. Add `~/.local/bin` to PATH if needed.

Native Linux Make/source-install workflows explicitly build into the repository's
`target/release`, overriding Cargo target-directory environment/config settings.
Install, run, status, stop, list, local packaging and clean use that same output
directory. Direct Cargo commands still honor their normal target-directory
settings; the isolated CI scripts explicitly manage their own directory.

The Android app needs JDK 17 or 21 and Android SDK platform 34 / build-tools
34.0.0. Use the committed Gradle wrapper (8.5); set `ANDROID_HOME` to the SDK
root or put `sdk.dir=/absolute/sdk/path` in ignored `android/local.properties`.
SDK tools must have their licenses accepted. No connected device is needed
for unit tests or APK builds.

The Android app:

```bash
cd android
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Or open `android/` in Android Studio.

## Running and testing

For a live foreground run, first stop any service/direct daemon, then use
`RUST_LOG=uscreen=debug uscreen start` in the desktop session. This can attach
a virtual display; it is separate from automated validation and inappropriate
for reproducing the known Cinnamon crash on a working desktop.

Normal automated checks (with their prerequisites installed):

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./android/gradlew -p android testDebugUnitTest assembleDebug lintDebug
```

The normal host suite includes permanent C sanitizer, FFmpeg, GUI startup,
installer and real Debian/RPM artifact tests. `.cargo/config.toml` serializes
test functions to avoid Linux ETXTBSY races between fixture writes and other
tests spawning processes; concurrency tests still run their own concurrent
clients/tasks. On Debian/Ubuntu, add:

```bash
sudo apt-get install -y python3 ffmpeg rpm fakeroot dpkg-dev \
  xvfb xauth x11-utils dbus-x11 at-spi2-core libglib2.0-bin libxkbcommon-x11-0 libegl1
```

GCC's ASan/UBSan/TSan runtimes must be installed with the compiler. The GUI
startup test uses an isolated Xvfb session and accessibility bus. Android
tests use Robolectric API 27 and 34; Gradle downloads their test images.

The separate [complexity gate](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/scripts/complexity/README.md)
checks the maximum of nine across owned source and tests. It uses pinned Python
parsers and a checksum-verified Kotlin compiler; follow its setup and boundary
test instructions before running `scripts/complexity/check.py`. CI runs both
the checker tests and the audit. Its documented macro/C limitations still
require review; this is not a native SonarQube analysis.

`scripts/fake-tablet.py` pretends to be a tablet on the loopback ports
(authenticates, reports a resolution, acks frames). With
`USCREEN_FAKE_TABLET=fake1,fake2` and `max_tablets = 2` it exercises a
second pipeline without a second physical tablet. It still drives the real
host capture/encoder pipeline and can attach EVDI; it is not an isolated unit
test or a measure of tablet decoding. Token discovery uses the daemon's base
selection: an existing XDG_RUNTIME_DIR, then an existing /run/user/<uid>, then
HOME/.cache (/tmp/.cache when HOME is absent). Both resolve base aliases before
using the uscreen/token path. Run with the daemon's environment; the script
reads the token and does not create runtime directories. Rust and Python test
this order against the shared runtime-bases.json fixture. The client drains video
through bounded reusable storage; its ACKs mean complete receipt with synthetic
decode time, never real rendering. The [T408 replay](benchmarks/2026-09-17-fake-tablet.md)
records one/two/four-client copying and allocation measurements.

## Project layout

```
host/              Rust daemon
  src/main.rs        CLI, orchestration, adb monitor, per-tablet sessions
  src/media.rs       Shared codec, frame-generation and live-settings contracts
  src/annex_b.rs     Incremental H.264/HEVC access-unit assembly
  src/ivf.rs         Bounded VP9 packet framing
  src/capture.rs     Capture supervision: settings, cancellation, retries
  src/capture/       Helper/FIFO, encoder, process and desktop adapters
  src/encoder.rs     optional in-process libavcodec encoder
  src/stream.rs      TCP video server, session token, IDR-aware backlog skipping
  src/input.rs       WebSocket input server, uinput devices, KWin mapping
  src/config.rs      re-exports shared settings
  src/runtime.rs     re-exports shared runtime state
  src/latency.rs     encoded-packet-to-render-acknowledgement timing
  src/tray.rs        StatusNotifierItem tray icon
  src/update.rs      release check (report only)
  src/doctor.rs      `uscreen doctor`
  src/osk.rs         KDE on-screen keyboard suppression over D-Bus
  src/kwin.rs        KWin D-Bus calls, through busctl or qdbus
  src/vdisplay.rs    EVDI discovery via sysfs
  src/edid.rs        EDID generation for the virtual display
  evdi/              C helper: EVDI framebuffer capture → NV12 → FIFO
    capture.c        EVDI callbacks, mode and framebuffer lifetime
    conversion.c     per-context workers and native/scaled BT.709 kernels
    frame_exchange.c triple buffers, dirty histories and generation-tagged leases
    fifo_writer.c    nonblocking writes and damaged-inode quarantine
    writer.c         lease ownership, pacing and idle keepalives
    evdi_helper.c    options, device acquisition, signals and teardown
common/            settings, commands, version parsing, runtime session ledger
gui/               egui desktop app: status, settings, start/stop
android/           Kotlin/Compose app: MediaCodec decoder, touch/pen capture
packaging/         deb control/postinst, rpm spec, PKGBUILD, udev/modprobe files
scripts/           install.sh, release build, fake tablet
docs/              documentation and proposed static-site source
```

The proposed [Windows integration plan](windows-port.md) covers pending
decisions, platform changes, milestones and validation. Windows host support
is not implemented yet.

## Command line

```
uscreen [OPTIONS] [COMMAND]

COMMANDS
  start           start the daemon
  stop            stop the daemon
  status          show daemon status
  list-displays   show compositor displays and PipeWire status
  wifi            enable ADB TCP; --off forgets/disconnects the saved address
  doctor          diagnose the setup and print fixes

OPTIONS (override ~/.config/uscreen/config.toml for this run only)
  --encoder <NAME>      h264_nvenc, hevc_nvenc, h264_vaapi, h264_vaapi_baseline, hevc_vaapi, libx264
  --fps <N>             frame rate (10–90)
  --bitrate <KBPS>      rate-control limit (1000–60000; not enforced by VAAPI CQP)
  --width/--height <N>  capture size (auto_resolution off)
  --quality <Q>         constant-quality target, 12–32, lower is sharper
  --stream-scale <N>    stream-only integer downscale, 1–4; 2 halves both axes
  --pen-only            graphics-tablet mode for this run
  --video-port/--input-port <PORT>
  --helper <PATH>, --edid <PATH>
  -h, --help; -V, --version
```

No subcommand defaults to `start`. `--width`/`--height` do not turn off
auto-resolution: set `auto_resolution = false` in config for a manual mode.
Oversized tablet metadata can still block an otherwise valid manual mode
(T275). An explicit `--edid` pins the supplied EDID instead of generating one.
`list-displays` invokes `kscreen-doctor -o` and, when present, `wpctl status`;
it is not a universal compositor enumeration API.

## Settings defaults and scope

These defaults come from `common/src/model.rs`, unless a saved config overrides
them. See the README for config paths and the app gear-menu controls.

| Host setting | Default | Scope or limit |
| --- | --- | --- |
| `encoder` | `h264_nvenc` | Explicit selection; no automatic GPU fallback |
| `vaapi_device` | `/dev/dri/renderD128` | Used by VAAPI |
| `fps` / `bitrate` / `quality` | 60 / 20000 kbps / 18 | Accepted ranges 10–90 / 1000–60000 / 12–32; encoder-specific rate control |
| `width` / `height` | 2960 / 1848 | Fallback dimensions; `auto_resolution = true` follows tablet geometry |
| `stream_scale` | 1 | Integer 1–4; divides stream dimensions, not capture dimensions |
| `position` / `pen_only` | `right` / false | Placement / graphics-tablet mode |
| `ten_bit` | false | HEVC FFmpeg path only; not HDR |
| `max_tablets` | 1 | Up to four slots; EVDI capacity required |
| `input_touch` / `input_pen` / `input_pointer` | true / true / true | Pointer creation also requires pen |
| `video_port` / `input_port` | 8890 / 8891 | Incremented by two per additional slot |
| `require_token` / `check_updates` / `auto_launch_app` | true / true / true | Keep authentication enabled; update checks are optional |
| `wifi_address` | empty | Set by `uscreen wifi`; reread for reconnect attempts |

Independent width/height/FPS limits do not guarantee a valid EDID combination
(T332). The GUI offers all registered encoder/profile choices, including HEVC VAAPI;
10-bit controls are enabled for HEVC. The render-node path remains a config setting.
App brightness starts at 50%, refresh preference at 60 Hz; these persist only
in the app and do not set the host stream rate or other apps' display settings.

ADB transport preference recognizes socket serials (`host:port`, including IPv6)
and mDNS service serials (`_adb` / `_adb-tls-connect`, with optional `.local`
and trailing dot). Plain device serials retain USB classification. The shared
classifier drives selection, Wi-Fi setup eligibility and diagnostics; network
ADB alone is not proof of physical Wi-Fi/radio use. Physical deduplication still
requires a nonempty, non-`unknown` `ro.serialno`; service-name prefixes are never
used to merge tablets. This follows AOSP's [mDNS connection naming](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/client/transport_mdns.cpp)
and [instance-name parsing](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/client/mdns_utils.cpp).

## Encoder tuning

- NVIDIA: `h264_nvenc` (default) or `hevc_nvenc` — see the codec section of
  the README for when HEVC and `ten_bit` are worth it.
- AMD/Intel: `h264_vaapi`, optional `h264_vaapi_baseline`, or `hevc_vaapi`, constant-quality via `quality`.
  The explicit baseline profile maps to stock H.264 VAAPI with Constrained Baseline/CAVLC;
  it reduced decoder delay on the tested tablet at higher bandwidth. Existing selections
  stay unchanged. See [codec measurements](benchmarks/2026-09-18-codecs.md).
  Set `vaapi_device = "/dev/dri/renderD129"` in config.toml to select another GPU
  (default: `/dev/dri/renderD128`). HEVC supports `ten_bit = true`.
- VP9: `libvpx-vp9` or `vp9_vaapi` on a GPU with encoding support. The CLI
  uses IVF framing and requires a current tablet capability report; see
  [codec compatibility and protocol](video-codecs.md).
- CPU H.264: `libx264`, `ultrafast`/`zerolatency`; throughput depends on CPU,
  resolution and content. No general laptop FPS limit has been measured here.

`quality` selects NVENC CQ, VAAPI QP or x264 CRF. NVENC VBR and x264 VBV
use the bitrate limit; **VAAPI CQP does not enforce it**. The desired bounded
VAAPI policy/UI remains unresolved (T259). Do not interpret the configured
bitrate as measured throughput. Static scenes may use much less bandwidth.

## Optional in-process encoder

```bash
cargo build --release --manifest-path host/Cargo.toml --features inproc-encoder
```

Encoded output retains known large packet allocations through stock public
libavcodec APIs, with copying for small or unknown storage. This requires no
FFmpeg patches. See [packet ownership and measurements](benchmarks/2026-09-17-packet-storage.md)
for retained-memory accounting, regression coverage and measurement limits.

Encodes through libavcodec in-process instead of an `ffmpeg` child. Measured
encoded-packet-to-acknowledgement latency was similar, with about one CPU
core less and keyframes on demand; that metric excludes encoding time. Needs
the development libraries enabled by `ffmpeg-next` default features, plus
`pkg-config` and libclang for bindgen. On Debian/Ubuntu, in addition to the
normal build prerequisites:

```bash
sudo apt-get install -y libclang-dev libavcodec-dev libavformat-dev libavutil-dev \
  libavfilter-dev libavdevice-dev libswresample-dev libswscale-dev
```

On Fedora, install the equivalent FFmpeg development libraries (for example
RPM Fusion's `ffmpeg-devel`) and clang development package. Check that
`pkg-config` resolves all seven libraries above. On atomic distributions a
container can supply the build environment; the installed binary still needs
ABI-compatible FFmpeg shared libraries at runtime. `ten_bit` is not available
on this path. The default FFmpeg subprocess build needs no FFmpeg headers.
The optional build rejects VAAPI selections (including the legacy
`vaapih264enc` alias) and VP9 before daemon resources or capture helpers are
created. Tablet requests cannot switch a running optional build to these encoders. This adapter
has no hardware-frames context/render-node integration: use the default build
for VAAPI, or select libx264/NVENC with the optional build. Compiling the feature
does not establish hardware availability or validate every encoder on a device.

Keep FFmpeg unmodified. Use distribution packages and their matching development
libraries; implement compatibility and encoder integration in UScreen's adapters
without maintaining or requiring FFmpeg patches.

### Encoder policy ownership

`common/src/encoding.rs` owns the encoder registry (also used by the GUI), preset/tune/quality options,
nominal GOP, maximum-rate value and buffer sizing. NVENC uses a one-frame buffer;
libx264 uses two frames; both keep the existing 200-kbit minimum and integer
kilobit rounding. VAAPI retains CQP and its existing maxrate argument, which does
not establish a rate ceiling (T259).

The CLI adapter adds command syntax, periodic wall-clock IDRs, explicit
`scenecut=0` for x264, VAAPI upload filters and optional HEVC depth conversion.
The in-process adapter sets bitrate/GOP/B-frame/color fields through the typed
libavcodec context, passes buffer sizes in bits, and requests IDRs on frames.
Its input remains 8-bit NV12; VAAPI and VP9 are rejected. Both adapters use BT.709
limited-range input; CLI color tags stay before `-i` to avoid conversion.
T373 boundary tests preserve these adapter differences, while the existing
software encode/decode regressions verify color and rate behavior.

### Packetizer ownership and profiling

`host/src/annex_b.rs` owns access-unit assembly; `encoder_io.rs` supplies the
shared start-code scanner. The packetizer's scan cursor and retained buffer are
separate from pending picture data. Do not discard the last possible prefix
bytes or mutate codec headers already held by a queued packet. Both three- and
four-byte prefixes, fragmented headers and multi-slice pictures are covered by
normal tests. Test-only allocation/copy/scan probes and the reproducible baseline
are documented in [the packetizer replay](benchmarks.md#annex-b-packetizer-replay).

## Release APK

```bash
cd android
keytool -genkeypair -keystore uscreen-release.keystore -alias uscreen \
        -keyalg RSA -keysize 2048 -validity 10000
# keystore.properties: storeFile / storePassword / keyAlias / keyPassword
./gradlew assembleRelease     # app/build/outputs/apk/release/app-release.apk
```

The keystore and `keystore.properties` are gitignored. Create a new key only
for a new signing identity; do not regenerate an established release key.
The fork's official identity and migration policy remain pending (T250);
a developer's new key is not automatically the official fork key. Android
requires compatible signing credentials for in-place updates. Debug and
release APKs normally use different keys. See [release integrity](../SECURITY.md#release-integrity).

## Releasing (maintainers)

The portable workflow builds in a Debian 12 container and checks a glibc
2.36 ceiling. This addresses the Linux x86-64 ABI baseline, not GPU/kernel
compatibility. Packaging needs both `rpm` and `rpmbuild`; the host also needs
Python 3.11+, binutils (including `readelf`), distrobox, Git and the Android
build/signing environment:


```bash
distrobox create --image debian:12 --name uscreen-build
# Inside: install the host compiler/GUI prerequisites listed above,
# plus dpkg-dev fakeroot rpm, and stable Rust through rustup.
# build-release.sh builds and bundles pinned libevdi v1.15.0 itself.

GH_TOKEN=... make publish NOTES=release-notes.md
```

Set `USCREEN_BUILD_CONTAINER=name` to use a differently named build container
with `make dist` or `make publish`; an empty or unset value uses `uscreen-build`.

`make publish` runs `scripts/build-release.sh` and `packaging/build-packages.sh`,
checks that HEAD and both local/origin tag objects match, and refuses to
continue unless all five release files exist. It creates a **draft**, uploads
those files plus `SHA256SUMS`, verifies the complete server asset inventory,
sizes and SHA-256 digests, then publishes. Any earlier failure leaves the draft
unpublished. Publishing needs Python 3.11+ on the host and `GH_TOKEN` with
release write access; credentials are read from the environment inside Python.
The Android release build runs on the host and needs the SDK/JDK and signing
key described above. No publishing command belongs in routine validation.
Version/date entries are checked literally. Before any release API request,
the publisher rechecks the original HEAD/tag object and rejects staged,
unstaged or untracked source changes made during the build. Use a dedicated
release checkout and leave it untouched until publication finishes: these
checks do not make a mutable filesystem an immutable build snapshot or detect
temporary edits that were reverted before the final check.

Ignored build outputs may change. SDK/compiler/container inputs and ignored
signing configuration/keys are supplied by the trusted release environment and
must stay fixed throughout the run; the Git checks do not attest those external
inputs. The fork signing/package-maintainer policies remain pending
(T250/T308), so a successful source check does not establish an official key.

`make dist-local` uses the local toolchain and requires a successful signed APK
build. It bundles libevdi v1.15.0 beside the helper; if the compiler cannot find
that exact library, pass `LIBEVDI=/absolute/path/libevdi.so.1.15.0`. Its tarball is
only compatible with systems at least as new as the build host. All tar/native
packages carry the notices and documentation listed in
`packaging/distribution-docs.txt`.

Local, portable and CI bundles share `scripts/stage-linux-bundle.sh`. It takes
the Rust binary directory, helper executable, libevdi file and destination;
it copies a replaceable library with its SONAME link and the common support
files. Callers retain build/ABI validation and APK creation. CI package fixtures
omit the APK; local and complete portable release bundles require it.

Before that, for a new version:

1. Bump `VERSION` in the Makefile, `version` in `host/Cargo.toml`,
   `gui/Cargo.toml`, and `common/Cargo.toml` (refresh `Cargo.lock`),
   `versionCode`/`versionName` in `android/app/build.gradle.kts` and
   `pkgver` in `packaging/arch/PKGBUILD`.
2. Add a `## X.Y.Z — YYYY-MM-DD` entry to `CHANGELOG.md`.
3. `make release-metadata DATE=YYYY-MM-DD` (or
   `scripts/update-release-metadata.sh X.Y.Z YYYY-MM-DD`): rewrites the
   source version, preparation date, candidate status and package names in
   `docs/index.html`, `docs/llms.txt`, `docs/sitemap.xml` and `CITATION.cff`.
   This does not verify publication: JSON-LD publication/download fields stay
   null and the citation has no release date. The candidate link names the
   proposed tag and may not resolve until publication.
4. Commit, tag `vX.Y.Z`, push both.

`make publish` re-checks all of it (`update-release-metadata.sh --check`,
the Cargo/Gradle versions, the changelog entry) and stops with a message
naming the stale file rather than publishing a page that still says the
previous version. The date defaults to today; `RELEASE_DATE=YYYY-MM-DD` overrides it.
After successful publication, review the release links and dated availability
notes in README/installation docs. Enable/deploy Pages separately if wanted;
its sitemap and robots file are deployment templates, not proof of hosting.
Update documentation branch links when the development work is merged.

## Portable build and package checks

`packaging/ci/Dockerfile` supplies Rust 1.90 on Debian 12, the pinned EVDI
userspace library, GUI libraries, sanitizers and the default workspace test
tools. Optional-encoder FFmpeg/libclang development packages are installed
by the separate build workflow, not this container.
The Docker base image is pinned by digest; Cargo uses `Cargo.lock`. Debian
security package updates remain enabled. Build/test tools include **both
`rpm` and the `rpmbuild` executable**, plus `dpkg-deb` and `fakeroot`.
Installing only `librpmbuild9t64` does not supply those commands. These tools
are not dependencies for running an installed UScreen package.

From the checkout, reproduce the CI environment and suite with:

```bash
docker build -t uscreen-ci packaging/ci
docker run --rm --security-opt seccomp=unconfined \
  -v "$PWD:/work" -v uscreen-cargo:/usr/local/cargo/registry \
  -v uscreen-target:/build -e CARGO_TARGET_DIR=/build \
  uscreen-ci cargo test --locked --release --workspace
```

The seccomp exception allows the existing ThreadSanitizer test's `setarch`
fallback to disable ASLR for its own child process. It needs no host sysctl
change, display socket, device mount or privileged container.

`.github/workflows/portability.yml` then builds real Linux packages and
installs them in clean Debian 12, Fedora 44, Arch and openSUSE Leap 16
containers. Arch runs the production `package()` recipe over the portable
binaries before installing its package in a second clean container. Only
its external `evdi-dkms` kernel prerequisite is assumed installed; ordinary
userspace dependencies are resolved by the package manager. Fedora uses
RPM Fusion for the declared FFmpeg dependency.

These jobs check the glibc 2.36 ceiling for the daemon, GUI, helper **and
bundled libevdi**, verify the helper's `$ORIGIN` lookup and ELF dependency resolution,
check notices, and exercise an idle daemon's direct start/status/stop with
no systemd user manager or display devices. T314 also launches the installed
GUI under an isolated Xvfb and requires its window to open without a panic;
this catches runtime-loaded libraries that `ldd` cannot inspect. The test
installs only the display server and inspection tools in addition to the
package's own dependencies. These checks do not validate EVDI kernel
attachment or desktop/compositor compatibility. Separate autostart fixtures
exercise systemd availability/failure, XDG fallback and actual desktop-entry
launch using disposable programs; they do not exercise a real desktop login.

To generate the same Linux package fixtures locally:

```bash
docker run --rm -v "$PWD:/work" \
  -v uscreen-cargo:/usr/local/cargo/registry -v uscreen-target:/build \
  -e CARGO_TARGET_DIR=/build uscreen-ci scripts/ci/build-artifacts.sh
```

This writes package test artifacts under `dist/`, without an Android APK;
use `scripts/build-release.sh` for the complete release bundle. Both paths
share the Linux layout and ABI validator. Native install smoke scripts under
`scripts/ci/` are intended only for disposable Docker containers.

### Lifecycle command deadlines

The CLI allows 10 seconds for daemon cleanup. GUI direct stop/restart commands
allow 15 seconds; systemd actions allow 35 seconds. Both shipped user units set
`TimeoutStopSec=15`: the GUI budget covers ExecStop, subsequent service retirement
and dispatch/restart overhead. These lifecycle limits are defined in
`common/src/commands.rs`; other external commands retain their five-second
limit. The GUI runs lifecycle actions on its worker and reports an error without
starting a replacement if a direct stop times out. On Linux each bounded command
starts a private process group. Deadline expiration signals that group and the
direct child; asynchronous cancellation also signals the group before dropping
the child. Normal direct children are reaped, and successful commands preserve
their status/output without running cancellation cleanup. A synchronous child
that cannot be signalled is left with a background reaper, so waiting for that
child does not extend the command deadline.

This is a command-response deadline, not transactional cancellation of delegated
work: processes that detach, change credentials or ask another service to act
can continue. Linux checks signal permission separately for group members; a
successful group signal does not prove every member was terminated. See the
[Linux signal contract](https://man7.org/linux/man-pages/man2/kill.2.html) and
[Rust process-group API](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.process_group).
Timeout errors explicitly report this limit. The GUI's two-minute privileged
setup deadline says setup may still be running and asks the user to check its
status before retrying. It does not claim to undo configuration changes or kill
root-owned work. Non-Linux adapters currently terminate only the direct child;
the Windows plan must supply its own job/process-tree boundary.

Permanent T328 tests use disposable process trees and an isolated Linux
subreaper, cover sync/async timeout, task cancellation, detached work and normal
completion, and verify no delayed writes from ordinary cancelled descendants.
An injected permission denial covers eventual reaping without blocking the
deadline; it does not execute privileged system setup. T094 continues to check
direct-child reaping.
