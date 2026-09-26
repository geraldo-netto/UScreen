# Third-party components
- libevdi (host/evdi/evdi_lib.h, and libevdi.so.1 shipped next to the helper
  in release packages) is copyright DisplayLink (UK) Ltd. and licensed under
  the GNU Lesser General Public License v2.1 or later. It is used unmodified as
  a separate shared library. Source for the bundled v1.15.0 library:
  https://github.com/DisplayLink/evdi/tree/v1.15.0/library
  License text: [LGPL-2.1](licenses/libevdi-LGPL-2.1.txt).
- The evdi kernel module is not part of this project; it is installed from
  your distribution or built from the same upstream repository.
- The AppImage builds unmodified FFmpeg 6.1.6 from upstream source with GPL
  components enabled, including libx264. It also bundles libvpx/libaom and uses
  MIT-licensed NV codec headers. The installed AppImage's
  `usr/share/doc/blent/bundled` directory contains the exact dependency
  notices and manifests. Its required matching `AppImage-sources.tar.gz`
  asset contains the FFmpeg/header archives, build configuration and exact
  Debian sources for bundled external codec libraries. See
  [AppImage dependencies and source distribution](docs/appimage-plan.md).
