#!/bin/sh
# Copyright (c) 2026 Geraldo Netto
# Disposable guest PID1 only; never run on the host.
export PATH=/usr/bin:/bin:/usr/sbin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts /run/udev /tmp/.X11-unix
mount -t devpts devpts /dev/pts
/usr/lib/systemd/systemd-udevd --daemon
/bin/modprobe virtio_pci
# Deliberately leave virtio_gpu unloaded until after the software Xorg screen.
/bin/modprobe evdi
sleep 1
ls -l /dev/dri
/usr/lib/xorg/Xorg :0 -noreset -nolisten tcp -ac -novtswitch -logfile /var/log/Xorg.log &
xorg_pid=$!
sleep 4
DISPLAY=:0 xdpyinfo >/tmp/ready 2>&1
printf 'T700 before-hotplug Xorg=%s\n' "$?"
/bin/modprobe virtio_gpu
sleep 5
if kill -0 "$xorg_pid" 2>/dev/null; then echo 'T700 Xorg survived'; else echo 'T700 Xorg exited'; fi
DISPLAY=:0 /usr/bin/xdpyinfo >/tmp/after 2>&1
printf 'T700 after-hotplug query=%s\n' "$?"
/usr/bin/bash /gpu-boot-order.sh /sys/class/drm /tmp/gpu-plan
printf 'T700 discovered-plan: '
cat /tmp/gpu-plan/blent-gpu-order.conf
cat /var/log/Xorg.log
cat /tmp/ready | head -12
poweroff -f
