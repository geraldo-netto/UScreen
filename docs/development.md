# Development

## Building from source

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
KDE mapping needs `kscreen-doctor` plus `busctl` or `qdbus`; X11 mapping needs
`xinput` and `xrandr`. See [installation.md](installation.md).

```bash
make build            # EVDI helper (C) + Rust daemon + GUI
make install          # copies to ~/.local/bin, installs the systemd user unit
make setup-system     # modprobe.d / modules-load.d / udev rule (sudo)
```

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

```bash
RUST_LOG=uscreen=debug uscreen start      # in a terminal, with the tablet attached
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

`scripts/fake-tablet.py` pretends to be a tablet on the loopback ports
(authenticates, reports a resolution, acks frames). With
`USCREEN_FAKE_TABLET=fake1,fake2` and `max_tablets = 2` it exercises a
second pipeline without a second device.

## Project layout

```
host/              Rust daemon
  src/main.rs        CLI, orchestration, adb monitor, per-tablet sessions
  src/capture.rs     EVDI helper + encoder management, display placement
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
common/            settings, commands, version parsing, runtime session ledger
gui/               egui desktop app: status, settings, start/stop
android/           Kotlin/Compose app: MediaCodec decoder, touch/pen capture
packaging/         deb control/postinst, rpm spec, PKGBUILD, udev/modprobe files
scripts/           install.sh, release build, fake tablet
docs/              this documentation and the GitHub Pages site
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
  wifi            put the tablet on the network (--off to undo)
  doctor          diagnose the setup and print fixes

OPTIONS (override ~/.config/uscreen/config.toml for this run only)
  --encoder <NAME>      h264_nvenc, hevc_nvenc, h264_vaapi, hevc_vaapi, libx264
  --fps <N>             frame rate (10–90)
  --bitrate <KBPS>      bitrate ceiling
  --width/--height <N>  capture size (auto_resolution off)
  --quality <Q>         constant-quality target, 12–32, lower is sharper
  --stream-scale <N>    downscale the stream only, 1 = native, 2 = half
  --pen-only            graphics-tablet mode for this run
  --video-port/--input-port <PORT>
  --helper <PATH>, --edid <PATH>
```

## Encoder tuning

- NVIDIA: `h264_nvenc` (default) or `hevc_nvenc` — see the codec section of
  the README for when HEVC and `ten_bit` are worth it.
- AMD/Intel: `h264_vaapi` or `hevc_vaapi`, constant-quality via `quality`.
  Set `vaapi_device = "/dev/dri/renderD129"` in config.toml to select another GPU
  (default: `/dev/dri/renderD128`). HEVC supports `ten_bit = true`.
- CPU: `libx264`, `ultrafast`/`zerolatency`; expect 30 fps at most on a laptop.

`quality` is what governs picture quality; `bitrate` is only a ceiling for
bursts. On a static desktop the stream sits far below it.

## Optional in-process encoder

```bash
cargo build --release --manifest-path host/Cargo.toml --features inproc-encoder
```

Encodes through libavcodec in-process instead of an `ffmpeg` child. Measured
encoded-packet-to-acknowledgement latency was similar, with about one CPU
core less and keyframes on demand; that metric excludes encoding time. Needs
the ffmpeg development headers (`ffmpeg-devel` from RPM Fusion on Fedora,
`libavcodec-dev libavformat-dev libavutil-dev libswscale-dev` on Debian). On
atomic distributions build inside a container (`distrobox`); the binary links
against the host's ffmpeg at runtime. `ten_bit` is not available on this path.

## Release APK

```bash
cd android
keytool -genkeypair -keystore uscreen-release.keystore -alias uscreen \
        -keyalg RSA -keysize 2048 -validity 10000
# keystore.properties: storeFile / storePassword / keyAlias / keyPassword
./gradlew assembleRelease     # app/build/outputs/apk/release/app-release.apk
```

The keystore and `keystore.properties` are gitignored. The same key must sign
every future release or users cannot update in place.

## Releasing (maintainers)

Binaries are built in a Debian 12 container for glibc 2.36 or newer (Debian 12+, Ubuntu 24.04+):

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
requires HEAD and both local/origin tag objects to match, and refuses to
continue unless all five release files exist. It creates a **draft**, uploads
those files plus `SHA256SUMS`, verifies the complete server asset inventory,
sizes and SHA-256 digests, then publishes. Any earlier failure leaves the draft
unpublished. Publishing needs Python 3.11+ on the host and `GH_TOKEN` with
release write access; credentials are read from the environment inside Python.
The Android release build runs on the host and needs the SDK/JDK and signing
key described above. No publishing command belongs in routine validation.

`make dist-local` uses the local toolchain and requires a successful signed APK
build. It bundles libevdi v1.15.0 beside the helper; if the compiler cannot find
that exact library, pass `LIBEVDI=/absolute/path/libevdi.so.1.15.0`. Its tarball is
only compatible with systems at least as new as the build host. All tar/native
packages carry the notices and documentation listed in
`packaging/distribution-docs.txt`.

Before that, for a new version:

1. Bump `VERSION` in the Makefile, `version` in `host/Cargo.toml`,
   `gui/Cargo.toml`, and `common/Cargo.toml` (refresh `Cargo.lock`),
   `versionCode`/`versionName` in `android/app/build.gradle.kts` and
   `pkgver` in `packaging/arch/PKGBUILD`.
2. Add a `## X.Y.Z — YYYY-MM-DD` entry to `CHANGELOG.md`.
3. `make release-metadata DATE=YYYY-MM-DD` (or
   `scripts/update-release-metadata.sh X.Y.Z YYYY-MM-DD`): rewrites the
   version, dates and package names in `docs/index.html` (including the
   JSON-LD), `docs/llms.txt`, `docs/sitemap.xml` and `CITATION.cff`.
4. Commit, tag `vX.Y.Z`, push both.

`make publish` re-checks all of it (`update-release-metadata.sh --check`,
the Cargo/Gradle versions, the changelog entry) and stops with a message
naming the stale file rather than publishing a page that still says the
previous version. The date defaults to today; `RELEASE_DATE=YYYY-MM-DD` overrides it.

## Portable build and package checks

`packaging/ci/Dockerfile` supplies Rust 1.90 on Debian 12, the pinned EVDI
userspace library, GUI libraries, sanitizers and every workspace test tool.
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
attachment or desktop/compositor compatibility. Service-manager/autostart
limitations remain tracked separately in `TODO.md`.

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
