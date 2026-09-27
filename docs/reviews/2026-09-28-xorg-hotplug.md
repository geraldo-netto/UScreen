<!-- Copyright (c) 2026 Geraldo Netto -->

# T700: late AMD GPU initialization after simpledrm

The September 26, 16:31:04 Xorg crash is a late glamor pixmap-key registration
failure. The triggering device is the AMD GPU at PCI `0000:08:00.0`, arriving as
`/dev/dri/card4` after Xorg had initialized its main screen on `simpledrm`.
EVDI devices were present as secondary screens, not the primary screen.
This diagnoses that crash; it does not establish a boot mitigation or resolve
T222's separate `ProcRRSetCrtcGamma` crash.

## Matched crash evidence

The maintainer exported PID 1918's existing core with mode `0600`. The core stays
outside the repository. The retained [stack](artifacts/2026-09-28-xorg-hotplug/matched-stack.txt)
and [selected state](artifacts/2026-09-28-xorg-hotplug/verified-state.txt) contain
only the observations needed for this investigation.

The crashed package was Ubuntu `xserver-xorg-core 2:21.1.12-1ubuntu1.6`.
Its Xorg build ID is `2dc631fda11e081777c5d43ba3ad1d6ce8a5a6fd`; glamor's is
`8ea749d0abf3e1030a6f2d63352ec16067df6aa2`. Both IDs were read from the core's
mapped ELF notes and matched against the archived binaries, rather than assuming
the currently installed binaries matched. Matching debug symbols identify:

- `dixRegisterPrivateKey(key=&glamor_pixmap_private_key,
  type=PRIVATE_PIXMAP, size=120)` at `dix/privates.c:382`.
- The key is uninitialized, while `global_keys[PRIVATE_PIXMAP].created == 7`.
  The assertion is `!global_keys[type].created`.
- `glamor_init` was called through modesetting's `drmmode_init` / `ScreenInit`,
  `AddGPUScreen`, and `xf86platformAddDevice(index=3)`, from the udev event path
  inside `WaitForSomething`. This was a GPU addition after initial server startup.
- Platform device 3 is `/dev/dri/card4`, driver `amdgpu`, PCI `0000:08:00.0`.
  The current PCI inventory identifies that address as AMD Raphael `[1002:164e]`.
- Platform device 0 is `simpledrm` at `/dev/dri/card0`. The main screen has glamor
  disabled. Its framebuffer pointer `0x704b2f82b000` falls in the core's mapping
  of `/dev/dri/card0 (deleted)`, directly connecting that screen to simpledrm.
  [Framebuffer mapping evidence](artifacts/2026-09-28-xorg-hotplug/framebuffer-mapping.txt)
  also shows EVDI mappings for card1/card2 and the new AMD mapping for card4.

The [boot journal](artifacts/2026-09-28-xorg-hotplug/boot-order.txt) places simpledrm
at 0.592 s, EVDI devices at 13.501/13.502 s, Xorg's EVDI opens from 16.715 s,
and AMD device registration at 17.838/18.001 s. The core supplies the missing
causal connection: the new AMD screen is the first attempted glamor pixmap-key
registration after non-glamor pixmaps have already been allocated.

GDB warned about the installed `libpciaccess` differing from the crashed process
and about unavailable unrelated driver symbols. The conclusions above use
matched Xorg/modesetting/glamor symbols and saved memory, not unwinding through
libpciaccess or inferring a device from its current DRM minor number. The old
`primaryBus.id.plat` pointer is not used to identify the primary framebuffer.

## Isolated component reproduction

Two retained GDB scripts exercise the archived server's actual DIX functions.
They stop at `main`, before Xorg server initialization; they do not start a
display, access a GPU, attach to the desktop process, or change module loading.

| Order | Observed result |
| --- | --- |
| Allocate one pixmap with `_dixAllocateScreenObjectWithPrivates`, then register a new `PRIVATE_PIXMAP` key of size 120 | Same `dix/privates.c:382` assertion and SIGABRT |
| Register that key, allocate one pixmap, then register the same key again | Both registrations succeed; one pixmap exists |

Scripts and results:
[late registration](artifacts/2026-09-28-xorg-hotplug/private-order.gdb),
[late output](artifacts/2026-09-28-xorg-hotplug/private-order.txt),
[early registration](artifacts/2026-09-28-xorg-hotplug/private-order-early.gdb),
[early output](artifacts/2026-09-28-xorg-hotplug/private-order-early.txt).
The late case deliberately makes GDB exit unsuccessfully after the assertion;
the early case returns success. Core generation was disabled for these probes.

These are component diagnostics, not a normal-suite regression for a Blent fix.
They do not replay udev events, reproduce a boot, or test whether a module-ordering
change works. No behavioral fix or regression-test completion is claimed.

To repeat, extract the following packages into an `old/` directory, without
installing them, then run the scripts from its parent directory. The scripts
expect the matching debug files at `old/usr/lib/debug`.

- [Archived server binary](https://snapshot.ubuntu.com/ubuntu/20260920T000000Z/pool/main/x/xorg-server/xserver-xorg-core_21.1.12-1ubuntu1.6_amd64.deb)
- [Matching debug symbols](https://launchpad.net/ubuntu/+archive/primary/+files/xserver-xorg-core-dbgsym_21.1.12-1ubuntu1.6_amd64.ddeb)
- [Archived source description](https://snapshot.ubuntu.com/ubuntu/20260920T000000Z/pool/main/x/xorg-server/xorg-server_21.1.12-1ubuntu1.6.dsc)
- [Package and binary hashes](artifacts/2026-09-28-xorg-hotplug/SHA256SUMS)

```sh
(ulimit -c 0; timeout 15s gdb -nx -nh -batch -x private-order.gdb)
(ulimit -c 0; timeout 15s gdb -nx -nh -batch -x private-order-early.gdb)
```

The separate [core-inspection commands](artifacts/2026-09-28-xorg-hotplug/verify.gdb)
expect the exported `Xorg-1918.core` beside `old/`; their ELF addresses are specific
to that incident. Do not commit the core or a broad process-memory dump.

## Version difference and remaining work

The package log records an upgrade from `1ubuntu1.6` to `1ubuntu1.8` at 17:02 on
September 26, after the investigated crash. The latter changes boot-display
selection, with boot-VGA fallback; it does not change `glamor/glamor.c` or
`dix/privates.c`. Both files compare byte-for-byte equal in the two extracted
source packages. Thus the component assertion is still possible, but this
comparison does not establish whether the old boot trigger recurs on `1ubuntu1.8`.
The [current boot ordering](artifacts/2026-09-28-xorg-hotplug/current-boot-order.txt)
still has EVDI before AMD. A successful boot by itself does not isolate the effect
of the package update from timing differences.

`scripts/install.sh::configure_boot_modules` provisions initial EVDI capacity
and autoloading without physical-GPU ordering. [EVDI's module guidance](https://github.com/DisplayLink/evdi/blob/33b7b3c9f7c77157c2b1b0c5a40cebe19b8ba110/module/README.md)
recommends loading the physical GPU driver before EVDI. That is a candidate setup
mitigation, not proof that EVDI caused this incident. Here simpledrm was primary;
the stronger requirement to test is that the intended accelerated GPU becomes
ready before Xorg initializes screens. An EVDI soft dependency alone must not be
assumed to provide that guarantee on every driver/init arrangement.

T700 remains open for this dependency-ordered work:

1. Build an isolated Linux boot reproduction with a software primary screen and
   delayed accelerated GPU discovery, retaining udev timestamps, Xorg logs,
   matched versions and the exact assertion. Include an EVDI-absent control to
   establish whether EVDI contributes beyond timing. No suitable boot fixture
   has been prepared in this investigation; the available Windows VM is not one.
2. Add a permanent failing regression before changing setup behavior. A proposed
   driver-ordering adapter also needs fixture coverage for AMD, Intel, Nouveau,
   proprietary NVIDIA, multiple GPUs, missing/unbound devices, malformed driver
   metadata, command failures and preservation of existing administrator options.
   Do not hard-code this machine's driver or DRM card numbers.
3. Validate the same regression after the mitigation and test isolated boot
   readiness and desktop startup across the maintained driver arrangements.
   Keep unavailable native validation explicit. Do not disable GPU hotplug or
   acceleration globally merely to avoid the assertion.

No boot configuration, installed Xorg package, kernel module, display service or
live desktop was changed. T222's original gamma-fault core remains a separate
evidence requirement.
