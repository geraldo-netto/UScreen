# Security

## Connections and trust boundaries

The host video and input servers bind to **127.0.0.1**, on ports 8890/8891
by default, plus two ports per additional tablet slot. The tablet reaches
these ports through `adb reverse`.

With the default `require_token = true`, each daemon run creates a random
256-bit token, encoded as 64 hex characters. The host delivers it over adb
on stdin, rather than in process arguments. The video and input connections
must authenticate before receiving video or injecting input. Keep token
authentication enabled: disabling it removes that protection. In disabled
mode, video accepts tokenless clients or consumes one optional saved 64-byte
hex token without checking its value. This compatibility path is never used
when token authentication is enabled. Incomplete optional prefixes expire
three seconds after their first byte; malformed or extra input closes the viewer.

Processes running as the same Linux user can read the token. Authorized adb
hosts and privileged Android apps are also inside the trust boundary: the
Android token-update activity requires the system DUMP permission, available
to adb shell and privileged apps. The ordinary launcher ignores token extras.
This is not isolation from compromised software running as your own user.

`uscreen wifi` runs `adb tcpip 5555`: it **opens the tablet's adb listener on
the network**, then stores its address for reconnection. The host video/input
ports remain on loopback. Use this only on a trusted local network.
`uscreen wifi --off` forgets the address and disconnects that adb connection;
it does not turn off the tablet's TCP listener. To return adbd to USB mode,
use `adb -s TABLET_SERIAL_OR_IP:5555 usb` with the identifier shown by
`adb devices`, or reboot the tablet and verify its debugging settings.
See [Android's adb instructions](https://developer.android.com/tools/adb#wireless).

Screen and input data are sent between the computer and tablet, over USB or
the selected ADB network connection. UScreen has no account, application
telemetry or cloud relay. This describes the application; the static website
loads fonts from Google and a social preview from GitHub.

## Local state and updates

The runtime base is an existing `$XDG_RUNTIME_DIR`, otherwise an existing
`/run/user/<uid>`, otherwise `$HOME/.cache` (`/tmp/.cache` if HOME is absent).
UScreen uses a `uscreen` subdirectory. Runtime-path selection returns an error
unless the resolved base belongs to the current UID, has owner read/write/search
permissions and cannot be written by group/other users. Base symlinks (such as a
relocated HOME cache) are resolved before validation. The final `uscreen`
directory is opened with `O_DIRECTORY | O_NOFOLLOW` and must belong to the same
UID with permission bits 0700. Existing unsafe paths are refused without changing
their ownership or permissions; a selected unsafe path does not trigger fallback.
Creation/open/validation errors propagate to startup and capture; doctor reports
them, and cleanup skips paths it cannot validate. Token and FIFO creation request
mode 0600 and use only accepted runtime paths. These checks do not protect
against the same user or root replacing their own directories after validation.

Android disables application backup and explicitly excludes the token-bearing
`uscreen.xml` preferences from legacy backup, cloud backup and device transfer.
Both rule files are referenced by the packaged manifest and covered by the
Android regression suite. Device-transfer exclusions matter because some OEMs
ignore `allowBackup=false` for that transport; see
[Android's backup rules](https://developer.android.com/identity/data/autobackup).

Optional HTTPS checks read the fork's latest release metadata from GitHub.
The daemon first checks after 30 seconds and then approximately daily while
running; the GUI checks at startup; Android checks at most once per Activity
instance when started, so Activity recreation can cause another request.
`check_updates = false` disables host checks; the app has its own *Check for
newer releases* switch. No application update is downloaded or installed
automatically. With no published fork release, no newer release is announced.

## What installation changes

Run the daemon as your ordinary desktop user. The supplied services are
systemd **user** services; the executable does not enforce a ban on root.
Privileged setup prepares devices and permissions so root is unnecessary
for normal streaming.

| Installed or changed state | Purpose |
| --- | --- |
| User-local binaries, menu entry, icons, user service and optional `XDG_CONFIG_HOME/autostart/uscreen.desktop`, or their package-managed counterparts | Application launch and optional desktop-session autostart |
| `/etc/modprobe.d/uscreen-evdi.conf` (script) or `/usr/lib/modprobe.d/uscreen-evdi.conf` (package) | Default `options evdi initial_device_count=2` at module load |
| `/etc/modules-load.d/uscreen.conf` (script) or `/usr/lib/modules-load.d/uscreen.conf` (package) | Boot-module list for init systems that read `modules-load.d`; other init systems need their own configuration |
| `/etc/udev/rules.d/60-uscreen-uinput.rules` or `/usr/lib/udev/rules.d/60-uscreen-uinput.rules` | Seat-user access to `/dev/uinput`; this permits synthetic input |
| Distribution packages and, where the full script selects them, RPM Fusion configuration or rpm-ostree layers | Runtime/build dependencies |

Project setup helpers preserve a loaded EVDI module and existing devices,
adding missing capacity or reporting why it cannot be added. This does not
resolve the separate Cinnamon crash observed during virtual-display attachment
(T222). See [installation.md](docs/installation.md) before changing a live
display setup. Package-manager changes and external EVDI installations are not
automatically undone by removing application files.

## How to uninstall completely

Stop the daemon before removing its files. For a systemd installation:

```bash
systemctl --user disable --now uscreen
```

For a terminal or direct GUI launch, use `uscreen stop`. Then remove **the
installation method you used**:

- Package: `sudo apt remove uscreen`, `sudo dnf remove uscreen`,
  `sudo zypper rm uscreen` or `sudo pacman -R uscreen`, as appropriate.
  Inspect the proposed transaction. Dependencies and repository settings may remain.
- Default source/tarball installation: remove these application-owned files:

```bash
rm -f ~/.local/bin/uscreen ~/.local/bin/uscreen-gui ~/.local/bin/evdi_helper
rm -f ~/.local/bin/libevdi.so.1 ~/.local/bin/libevdi.so.1.15.0
rm -f ~/.config/systemd/user/uscreen.service
rm -f ~/.config/autostart/uscreen.desktop
rm -f ~/.local/share/applications/uscreen.desktop
rm -f ~/.local/share/icons/hicolor/scalable/apps/uscreen.svg
rm -f ~/.local/share/icons/hicolor/scalable/apps/uscreen-pen.svg
systemctl --user daemon-reload
```

Run the final reload command only when a systemd user manager is available.
Remove the desktop-autostart entry after either installation method if the GUI
or installer created it; native package removal does not remove user preferences.

Adapt the binary path if you selected a custom `BIN_DIR`. Both user installers
use an absolute `XDG_DATA_HOME` for launchers/icons and an absolute
`XDG_CONFIG_HOME` for the user unit and desktop-autostart entry. Unset, empty or
relative values use the HOME defaults shown above. Adjust those cleanup paths
to your selected bases and remove only the named UScreen files. Native packages
own their files under `/usr`; let the package manager remove them instead of
deleting arbitrary system libraries.

Remove the following only if UScreen created them and you do not need their
settings for another EVDI/uinput application:

```bash
sudo rm -f /etc/modprobe.d/uscreen-evdi.conf
sudo rm -f /etc/modules-load.d/uscreen.conf
sudo rm -f /etc/udev/rules.d/60-uscreen-uinput.rules
sudo udevadm control --reload
```

Settings are in `$XDG_CONFIG_HOME/uscreen` when XDG_CONFIG_HOME is absolute,
otherwise `~/.config/uscreen`. Logs/PID state from direct GUI launches are in
`~/.local/share/uscreen`; service logs are in the user journal. After stopping,
remove unwanted settings/state and the selected runtime `uscreen` directory
listed above. Check it for a pending `osk-restore` backup before removal: on
KDE, restore the desired virtual-keyboard setting first. Journal records,
external dependencies, kernel modules and repository configuration require
separate management; deleting these directories does not remove them.
Reboot if you need boot-module or device-access changes to take full effect;
do not unload an in-use EVDI module as an uninstall step.

On the tablet, uninstall UScreen normally. If Wi-Fi debugging was enabled,
return adbd to USB mode as described above.

## Release integrity

The publishing workflow produces `SHA256SUMS` and verifies uploaded asset
hashes before publication. Checksums detect changes relative to that manifest;
they do not independently authenticate its publisher.

The fork's permanent Android release key is provisioned; its public certificate
and SHA-256 fingerprint are recorded in [release signing](docs/release-signing.md).
The selected application ID is `io.github.geraldo_netto.uscreen`. Current code
still builds `com.uscreen` and accepts a supplied keystore: application-ID
migration and publication-time certificate enforcement remain open under T250.
Provisioning the key does not establish that existing APKs were signed with it.
The planned fork package installs separately from upstream; no upstream
certificate is asserted as the fork identity.

Android updates require compatible signing credentials;
a debug APK, a differently signed release, a version downgrade or platform
requirements can prevent installation over an existing app. Such a failure
alone does not establish who built the APK. Preserve the chosen release key
for future updates; do not delete app data to bypass a mismatch without first
understanding which build you are installing.
