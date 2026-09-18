# FAQ

**Is UScreen an open-source SuperDisplay alternative for Linux?**
Yes. It provides a real extended display on an Android
tablet over a USB cable, with S Pen pressure and tilt, on a Linux host.
SuperDisplay supports Windows hosts and Android clients; its
[official FAQ](https://superdisplay.app/help/) says no Linux or macOS port is planned
(checked 2026-09-17).

**Can an Android tablet be used as a real second monitor on Linux?**
Yes. UScreen creates a virtual display through the EVDI kernel module, so the
tablet shows up in your display settings like any other monitor — windows can
be moved onto it, it has its own resolution and position, and it is not a
mirror.

**Does UScreen work over a normal USB cable?**
Yes, over a normal data-capable USB cable (charge-only cables carry no data).
It uses the adb connection that USB debugging provides; no special cable and
no USB tethering.

**Does UScreen require USB tethering or Wi-Fi?**
No. USB tethering is not needed; USB debugging is. Wi-Fi works as a fallback,
with longer tail delays in the historical test — see the benchmarks.

**Can I adjust the tablet's brightness and refresh rate?**
Open UScreen's gear menu. Brightness starts at 50% and the display refresh-rate preference at
60 Hz. Both controls apply immediately and remember your choices, including in
graphics-tablet mode. They affect only UScreen; other apps keep the tablet's
normal settings. Refresh-rate choices use the current display resolution,
with the closest supported rate as a fallback. Choose **System default** to
let Android select the refresh rate. Android may override a requested mode.
These controls are separate from the stream's frame rate and bitrate.

**Can I use it without the cable?**
Yes, as a fallback. Run `uscreen wifi` once with the cable plugged in: it puts
the tablet's adb on the network, remembers the address, and from then on the
daemon reconnects on its own whenever the cable is out. The tablet goes back
to USB-only when it reboots, so that one command has to be repeated after a
tablet restart. `uscreen wifi --off` forgets the address and disconnects; it
does not close the tablet's adb TCP listener. Host video/input ports stay on
loopback, but `uscreen wifi` opens tablet port 5555. Use a trusted network;
see [SECURITY.md](../SECURITY.md#connections-and-trust-boundaries) for returning
adbd to USB mode.

**Does UScreen support Samsung S Pen pressure and tilt?**
Yes: pressure, tilt, the eraser end and the stylus button are all forwarded
to Linux as a real graphics-tablet device, so Krita, GIMP, Blender and the
rest see it as a tablet. Angular tilt units and hover/button state across tip
lift have automated regression coverage; physical-device reports remain
important. Consult the [compatibility guide](compatibility.md).

**Does UScreen extend the desktop or only mirror the screen?**
It extends. There is also a "graphics tablet" mode in which nothing is
streamed and the pen drives your existing screen. Its connection overlay waits
for the host's authenticated control greeting; opening a socket alone is insufficient.

**Does UScreen work on Bazzite and KDE Wayland?**
That is the historical upstream reference setup. KDE on Wayland gets automatic output placement,
input mapping and on-screen-keyboard suppression. X11 desktops get automatic
input mapping when `xinput` and `xrandr` are installed; place outputs through
desktop display settings. Other Wayland desktops depend on their compositor
facilities and manual mapping. See [current limitations](compatibility.md#current-fork-limitations).

**Does UScreen require a dummy HDMI plug?**
No. The virtual display is created in software by EVDI.

**What Android versions are supported?**
Android 8.1/API 27 and newer. The selected codec, profile, resolution and
frame rate must be supported by the device decoder; the Android version
alone does not guarantee compatibility.

**Is screen or input data sent to the cloud?**
No. Screen and input data never leave the cable (or your own network if you
chose Wi-Fi). The only automatic internet request is an optional check of the latest
release tag on GitHub, off with `check_updates = false` on the desktop and a
switch in the app's settings sheet. See
[SECURITY.md](../SECURITY.md).

**How is UScreen different from Weylus?**
[Weylus](https://github.com/H-M-H/Weylus#readme) uses the tablet's browser and
captures a host screen or window; extension requires a separately configured
output. Its documented Android USB setup uses `adb reverse`. UScreen creates
an EVDI output and uses a native Android app. Both offer Linux pen input.

**How is it different from Sunshine/Moonlight?**
[Sunshine](https://docs.lizardbyte.dev/projects/sunshine/latest/) streams a host
display to a Moonlight client over a network. How an extra output is provided
depends on the host setup. UScreen provisions its output through EVDI and
uses adb to carry video and input over USB. This comparison does not imply
that Sunshine/Moonlight lacks stylus support.

**What latency should I expect?**
Historical upstream results on the reference hardware: 18–22 ms median with H.264, 15–18 ms with
HEVC, over USB, measured from encoded packet readiness to receipt of the
render acknowledgement. Capture, encoding and packetizer assembly are excluded;
see [measurement boundaries](benchmarks.md#how-latency-is-measured).

**Can I use two tablets?**
The configuration supports 1–4 slots. Historical validation used one real
tablet plus a simulated client. The current allocator leases a free EVDI card
and skips busy cards; automated tests cover independent slots and fallback.
Reports with multiple physical tablets are welcome.

**How do I uninstall UScreen completely?**
Instructions for package and source installs, custom paths and residual
system state are in
[SECURITY.md](../SECURITY.md#how-to-uninstall-completely).
