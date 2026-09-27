# Copyright (c) 2026 Geraldo Netto
set pagination off
set debuginfod enabled off
set auto-load off
set debug-file-directory old/usr/lib/debug
set sysroot old
set solib-search-path /lib/x86_64-linux-gnu
file old/usr/lib/xorg/Xorg
core-file Xorg-1918.core
python
import gdb
for name, addr, expected in [
 ("Xorg", 0x640587d2a000 + 0x378, "2dc631fda11e081777c5d43ba3ad1d6ce8a5a6fd"),
 ("glamor", 0x704b3ee64000 + 0x2d8, "8ea749d0abf3e1030a6f2d63352ec16067df6aa2")]:
 actual = bytes(gdb.selected_inferior().read_memory(addr, 20)).hex()
 print(name, "core build ID", actual, "matches", actual == expected)
end
p glamor_pixmap_private_key
p global_keys[PRIVATE_PIXMAP]
p *xf86_platform_devices[3].attribs
p *xf86_platform_devices[0].attribs
p ((modesettingPtr)xf86Screens[0]->driverPrivate)->drmmode.glamor
p *((modesettingPtr)xf86Screens[0]->driverPrivate)->drmmode.front_bo.dumb
