Name:           blent
Version:        1.1.0
Release:        1%{?dist}
Summary:        Android tablet as a USB second display and graphics tablet
License:        MIT AND LGPL-2.1-or-later
URL:            https://github.com/geraldo-netto/UScreen
Source0:        blent-%{version}-linux-x86_64.tar.gz
BuildArch:      x86_64
Requires:       ffmpeg android-tools
# Loaded by the GUI at runtime; automatic ELF dependency scans cannot see it.
# SONAME works with both Fedora and openSUSE package names.
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libX11.so.6()(64bit) libX11-xcb.so.1()(64bit)
Requires:       libXcursor.so.1()(64bit) libXi.so.6()(64bit)
# The helper ships with its own libevdi next to it (LGPL, $ORIGIN rpath), so
# only the kernel module is needed from the system. That is packaged on
# openSUSE (evdi) and not on Fedora at all, hence Recommends rather than
# Requires: the package must still install on Fedora, where evdi is built from
# source.
Recommends:     evdi

%description
Turns an Android tablet into a low-latency second monitor for a KDE Wayland
desktop over USB, with touch and stylus forwarded back. Can also act as a
plain graphics tablet for the host's own screen.

%prep
%setup -q -n blent-%{version}

%install
install -Dm755 scripts/setup-evdi.sh %{buildroot}%{_datadir}/blent/setup-evdi.sh
install -Dm755 bin/blent          %{buildroot}%{_bindir}/blent
install -Dm755 bin/blent-gui      %{buildroot}%{_bindir}/blent-gui
install -Dm755 bin/evdi_helper      %{buildroot}%{_libdir}/blent/evdi_helper
install -Dm755 bin/libevdi.so.1.15.0 %{buildroot}%{_libdir}/blent/libevdi.so.1.15.0
ln -sf libevdi.so.1.15.0            %{buildroot}%{_libdir}/blent/libevdi.so.1
install -Dm644 scripts/blent.desktop %{buildroot}%{_datadir}/applications/blent.desktop
install -Dm644 packaging/icons/blent.svg     %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/blent.svg
install -Dm644 packaging/icons/blent-pen.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/blent-pen.svg
install -Dm644 packaging/blent.service %{buildroot}%{_userunitdir}/blent.service
install -Dm644 packaging/blent-evdi.conf    %{buildroot}%{_modprobedir}/blent-evdi.conf
install -Dm644 packaging/blent-modules.conf %{buildroot}%{_modulesloaddir}/blent.conf
install -Dm644 packaging/60-blent-uinput.rules %{buildroot}%{_udevrulesdir}/60-blent-uinput.rules

./scripts/copy-distribution-docs.sh %{buildroot}%{_docdir}/blent

%post
sh %{_datadir}/blent/setup-evdi.sh 2 || true
# Icon caches go by directory mtime; touch the theme so menus pick the
# icon up without a logout.
touch /usr/share/icons/hicolor 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || true
modprobe uinput 2>/dev/null || true
udevadm control --reload 2>/dev/null || true
udevadm trigger --name-match=uinput 2>/dev/null || true

%files
%{_datadir}/blent/setup-evdi.sh
%{_bindir}/blent
%{_bindir}/blent-gui
%{_libdir}/blent/evdi_helper
%{_libdir}/blent/libevdi.so.1
%{_libdir}/blent/libevdi.so.1.15.0
%{_datadir}/applications/blent.desktop
%{_datadir}/icons/hicolor/scalable/apps/blent.svg
%{_datadir}/icons/hicolor/scalable/apps/blent-pen.svg
%{_userunitdir}/blent.service
%{_modprobedir}/blent-evdi.conf
%{_modulesloaddir}/blent.conf
%{_udevrulesdir}/60-blent-uinput.rules
%license %{_docdir}/blent/LICENSE
%license %{_docdir}/blent/THIRD_PARTY_LICENSES.md
%license %{_docdir}/blent/licenses
%doc %{_docdir}/blent/README.md
%doc %{_docdir}/blent/SECURITY.md
%doc %{_docdir}/blent/CHANGELOG.md
%doc %{_docdir}/blent/CONTRIBUTING.md
%doc %{_docdir}/blent/docs
