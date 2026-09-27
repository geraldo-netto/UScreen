# T222 follow-up: distinguish Xorg failure signatures

Historical evidence below was collected on September 27. The
[September 28 T700 follow-up](2026-09-28-xorg-hotplug.md) now analyzes the
maintainer-exported PID1918 core with matching symbols: late AMD hotplug after
a simpledrm primary screen triggered glamor pixmap-key registration. The other
cores, including T222’s original gamma fault, were not exported in that follow-up.

Fresh read-only journal/coredump metadata contains seven Xorg aborts. They do not
all match the original T222 RandR gamma crash:

| Incident (CEST) | PID | Available stack signature |
| --- | ---: | --- |
| September 17, 01:52:40 | 2392 | Near-null fault through `ProcRRSetCrtcGamma` |
| September 20, 16:46:19 | 1986 | Assertion through `dixRegisterPrivateKey`, `glamor_init`, `AddGPUScreen` |
| September 21, 21:51:37 | 1948 | Startup `InitOutput`; insufficient metadata for the same assertion claim |
| September 21, 21:57:36 | 1954 | Glamor/private-key assertion |
| September 22, 00:20:56 | 2024 | Glamor/private-key assertion |
| September 26, 03:10:31 | 2000 | Glamor/private-key assertion |
| September 26, 16:31:04 | 1918 | Glamor/private-key assertion |

[Retained stack excerpts](artifacts/2026-09-27-xorg-signatures/) preserve the
available offsets and timestamps. All seven core files are inaccessible to the
ordinary user. The original gamma failure still lacks a symbolized causal core
or isolated reproduction; a later, different assertion cannot close it.

The latest assertion occurred during boot. At 16:31:02 Xorg PID 1918 opened EVDI
card1/card2. At 16:31:03 both AMD GPUs registered, followed by the Xorg abort at
16:31:04. The kernel then records Xorg closing both EVDI nodes. There is no Blent
helper connection in this retained kernel interval. This supports investigating
GPU initialization order, but does not identify the exact failing private key,
prove which GPU triggered the assertion, or prove that a setup change fixes it.

This distinct investigation is open as T700. The upstream
[EVDI module guidance](https://github.com/DisplayLink/evdi/blob/33b7b3c9f7c77157c2b1b0c5a40cebe19b8ba110/module/README.md)
recommends creating initial devices before Xorg and loading EVDI after the primary
GPU driver. Blent's installer currently writes initial capacity without that
primary-GPU ordering. A targeted ordering/acceleration proposal needs an isolated
boot reproduction, permanent regression and mixed-driver validation. No live
module, boot configuration, primary GPU or desktop session was changed.

T222 remains unresolved pending original-signature evidence and a permanent
isolated attach/gamma/detach regression. Normal authorized application reloads
remain independent. The earlier successful T418 reattachment is still evidence
of that particular run, not a root-cause fix for either failure family.
