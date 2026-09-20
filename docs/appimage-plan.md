# AppImage distribution — T308

AppImage replaces the Debian `.deb` release asset in the packaging and publishing
code. RPM, Arch and the portable tarball remain available build formats. This
change does not announce a published fork release.

## Launch and install for your user

On Linux x86-64 with glibc 2.36 or newer:

```bash
chmod +x uscreen-1.2.3-x86_64.AppImage
./uscreen-1.2.3-x86_64.AppImage               # GUI
./uscreen-1.2.3-x86_64.AppImage status        # daemon CLI
./uscreen-1.2.3-x86_64.AppImage --daemon --help
```

Stop any running UScreen daemon before explicitly registering this distribution:

```bash
./uscreen-1.2.3-x86_64.AppImage --install-user
```

Registration copies the image to `$XDG_DATA_HOME/uscreen/appimage/UScreen.AppImage`
(default `~/.local/share`), writes a user service and desktop launcher, and creates
`~/.local/bin/uscreen`. The installed image wrapper enables extract-and-run,
so it also works on hosts without FUSE. It preserves application preferences and existing
autostart intent; enable or disable autostart in the GUI. Existing desktop
autostart paths are redirected to the installed image. When the user service is
already enabled, registration creates or repairs its desktop login entry, which
starts that same service even on desktops without `graphical-session.target`.
Registration does not start streaming,
activate EVDI, enable autostart, uninstall packages or alter system configuration.

Update by stopping UScreen and registering the new image the same way. Close the
old GUI and reopen the installed launcher. Keep the installed image in place:
a running portable GUI reports a missing launcher if its outer image was moved.
The GUI will use a systemd service only when its installation marker matches the
current AppImage path; otherwise it starts its own foreground daemon process.

## Without FUSE; replacing libevdi

The [runtime supports extraction](https://docs.appimage.org/user-guide/troubleshooting/fuse.html):

```bash
./uscreen-1.2.3-x86_64.AppImage --appimage-extract-and-run
# Or retain an editable directory:
./uscreen-1.2.3-x86_64.AppImage --appimage-extract
./squashfs-root/AppRun --install-user
```

Registration from a manually extracted directory copies it to
`$XDG_DATA_HOME/uscreen/appimage/UScreen.AppDir`. Registration from automatic
extract-and-run copies the original image. The launcher propagates extraction
mode to children. Every independently launched daemon invokes the outer image
again and owns its own runtime lifetime; closing the GUI cannot unmount the
daemon's files. Manually extracted execution uses the persistent directory.

`usr/bin/libevdi.so.1.15.0` remains unmodified and replaceable, with the
`libevdi.so.1` link beside the helper. Use an extracted directory to replace it
with an ABI-compatible library and retain the applicable license notices.

## Bundled dependencies and host requirements

The bundle contains the GUI, daemon, helper, Bash, pinned upstream **FFmpeg
6.1.6/ffprobe**, Debian ADB and discovered dependencies. FFmpeg is built from
unmodified, checksum-pinned source on Debian 12, with its libav libraries linked
into the executables. Its external libx264, libx265, libvpx and libaom codec libraries
have a private search directory that takes precedence over matching host
libraries. AppImage therefore uses its selected codec build independently of
the operating system's FFmpeg package.

The build includes H.264/HEVC/VP9/AV1 VAAPI and H.264/HEVC/AV1 NVENC support,
plus software H.264, VP9 and AV1. A compiled backend still needs compatible
host hardware/drivers and a usable tablet decoder. Automatic selection retains
the existing compatibility, quality, measured timing and render-progress checks;
installing a newer codec library does not prove that AV1 or HEVC is fastest.
The generic x86-64 build retains runtime CPU dispatch rather than requiring
the build machine's instruction set. Stock source receives no codec patches;
UScreen supplies its existing low-latency runtime options.
The bundled libx265 software encoder also preserves the existing HEVC framing
regressions; it is not an additional automatic-selection candidate.

Other matching host library SONAMEs are preferred, with bundled fallbacks, for
GPU and desktop integration. Dynamically loaded X11/Wayland libraries are also
included; the helper prefers its sibling libevdi. Private search paths apply
only to bundled executables. AppRun does not set a global `LD_LIBRARY_PATH`
that could affect host utilities. This is an ABI baseline, not a promise that
every distribution or graphics stack works.

The host still supplies its matching glibc/loader, GPU implementations and kernel
interfaces. EVDI/DKMS for the running kernel, graphics drivers, `/dev/uinput`, USB
permissions and any Xorg/compositor integration remain host setup. Follow
[installation prerequisites](installation.md); the bundle includes the explicit
`usr/share/uscreen/setup-evdi.sh` tool. Core host utilities and a Linux desktop
session are required. No package smoke check deliberately attaches EVDI.

When migrating from `.deb`, stop the existing daemon, close its GUI and register
the AppImage. Configuration stays in the same user location. Removing the old
system package is optional and explicit; inspect the package transaction. The
installer does not remove existing kernel packages, rules or preferences.

## Build inputs, licenses and sources

`packaging/appimage/build.py` consumes the portable Debian 12 Linux bundle.
`packaging/ci/Dockerfile` supplies the dependencies and matching source indexes.
Packaging tools/runtime are pinned with SHA-256 in `tools.json`; a changed or
missing digest fails before execution. Every bundled ELF is checked against the
glibc 2.36 ceiling. Missing libraries or corresponding sources fail packaging.
`ffmpeg.json` pins the upstream FFmpeg source and NV codec headers;
`ffmpeg_bundle.py` verifies them before extraction/build. Packaging builds this
version with two compiler jobs by default (`--ffmpeg-jobs 1..128`); a previously
built prefix may be supplied with `--ffmpeg-prefix`, but its recipe, executable
hashes, versions and encoder inventory must pass verification. Downloaded source
archives are cached under `--ffmpeg-cache`; each build gets its own working tree.

The required `uscreen-<version>-AppImage-sources.tar.gz` asset contains exact
Debian corresponding-source archives and descriptors, Rust dependency sources,
the pinned libevdi source, upstream FFmpeg/NV header archives with their build
manifest, and AppImage runtime source. Debian archive checksums
are verified against their source descriptors. `usr/share/doc/uscreen/bundled`
contains dependency copyright files and a version/license/source manifest.
Project source is supplied by the matching release tag. Keep the source asset,
notices and matching project source alongside any redistribution.

The publisher verifies the AppImage, source archive, RPM, Arch recipe, Linux tar,
APK and `SHA256SUMS` as one complete asset set before publishing. Legacy Debian
metadata remains only for historical regression coverage and is not a release
format. Maintainer: [Geraldo Netto](https://github.com/geraldo-netto).
