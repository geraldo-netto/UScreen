<!-- Copyright (c) 2026 Geraldo Netto -->

# T700 retained evidence and boot recipe

Serial logs retain the guest output, including Xorg's timestamped device
addition and failure/survival. Terminal escape sequences, carriage returns and
trailing whitespace were removed for readable diffs; raw originals remain in
the scratch directory. Red test logs precede each associated fix.
`coverage-manifest.json` fingerprints measured sources;
`coverage-report.json` is the full report for the scoped test selection, while
`coverage-affected-functions.json` selects the nine changed production functions.
`coverage-tests.log` records the final instrumented green runs. The original
coverage traces and disposable guest images remain outside git under
`/backups/disk2/blent-work/t700-fix/`.

## Guest assembly

These are Linux x86-64 reproduction instructions, not an installer. Build in a
new private scratch directory. Do not run `guest-init.sh` on the host: it is
PID1 inside the disposable guest and deliberately starts Xorg and powers off.

1. Extract Ubuntu's matching kernel image package, without installing it:
   `linux-image-unsigned-7.0.0-34-generic_7.0.0-34.34~24.04.1_amd64.deb`.
   Use the matching `/lib/modules/7.0.0-34-generic` tree. Copy the module files
   returned by `modprobe --show-depends` for `virtio_pci`, `virtio_gpu`, `bochs`
   and `evdi`, plus `modules.*`, into the guest at their original paths.
   The run used the distribution's `/lib/modules/7.0.0-34-generic/ubuntu/evdi/evdi.ko.zst`.
2. Create a minimal initramfs root with static BusyBox `/bin/busybox` and applet
   links for sh, mount, mkdir, cat, sleep, poweroff, grep, kill, dmesg, ls, printf,
   readlink, timeout, date and head. Include proc/sys/dev/run/tmp/var/log and
   etc/X11/xorg.conf.d, etc/udev/rules.d and etc/modprobe.d directories.
3. Copy kmod, udevadm, systemd-udevd, xkbcomp, xdpyinfo, xrandr and their dynamic
   libraries at their original paths. `/bin/modprobe` must link to
   `/usr/bin/kmod`; use the absolute path in init to avoid BusyBox's applet.
   Copy `/usr/share/X11/xkb`, `/usr/share/glvnd/egl_vendor.d`, libEGL/libGL,
   libEGL_mesa/libgbm, DRI `virtio_gpu_dri.so`/`swrast_dri.so`, GBM `dri_gbm.so`
   and their dynamic dependencies. Preserve filenames containing spaces when
   parsing `ldd` output. Never follow a guest absolute symlink while copying
   from the host: replace the guest link before writing that destination.
4. Copy Xorg, `modesetting_drv.so` and `libglamoregl.so` plus dependencies from
   the selected package revision. The old package and hashes are linked from
   [the core investigation](../../2026-09-28-xorg-hotplug.md). For the current
   run these came from installed revision `1ubuntu1.8`. Write root passwd/group
   entries and video/render groups in the guest. Write `options evdi
   initial_device_count=2` in its modprobe.d. Xorg's ServerFlags set
   `AutoAddDevices` false and `AutoAddGPU` true.
5. Copy Bash and GNU readlink/sort/mktemp/ln/chmod/rm/mkdir with dependencies
   into guest `/usr/bin`, and the checkout's `scripts/gpu-boot-order.sh` as
   `/gpu-boot-order.sh`. Install the retained `guest-init.sh` as executable
   `/init`. The final init probes real guest sysfs after startup.
6. Red: no `blent-gpu-order.conf`. Green: generate that file with the helper
   using a fixture `card0` symlink to `devices/gpu/drm/card0`, whose provider
   `devices/gpu/driver/module` resolves to a `module/virtio_gpu` directory;
   put the generated file in the guest's `/etc/modprobe.d`. This simulates
   installation with an already bound driver. The final guest run independently
   verifies discovery against real sysfs returns the same rule. For the
   EVDI-absent control remove `/bin/modprobe evdi` from guest init and omit
   the ordering rule; keep the delayed `/bin/modprobe virtio_gpu`.
7. Pack the guest root with `find . -print0 | cpio --null -o --format=newc |
   gzip -1 > ../guest.cpio.gz`. Copy OVMF_VARS_4M.fd to a private
   `guest-vars.fd`. Boot with the command below, selecting your kernel image,
   private initramfs and render node. No host disk or network is attached.

```sh
timeout 35s qemu-system-x86_64 -name blent-t700-isolated \
  -machine q35,accel=kvm -cpu host -m 2048 -smp 2 -nodefaults \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=guest-vars.fd \
  -kernel kernel/boot/vmlinuz-7.0.0-34-generic -initrd guest.cpio.gz \
  -append 'console=ttyS0 rdinit=/init loglevel=4' \
  -device virtio-vga-gl -display egl-headless,rendernode=/dev/dri/renderD128 \
  -serial file:guest.log -no-reboot
```

Both cases must answer the initial `xdpyinfo` query. Red must reproduce the
private-key assertion and Xorg exit; green must show glamor at initial startup,
survive, answer the second query and report `softdep evdi pre: virtio_gpu`.
The early old-package green init checked survival but did not yet include the
second query or real-sysfs helper check; the final current-package run has both.
