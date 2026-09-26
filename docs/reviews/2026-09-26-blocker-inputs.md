# Inputs needed for the reviewed TODO items

Reviewed 2026-09-26 against the current implementation and TODO ledger. This is
an input/dependency map, not authorization to resume every deferred experiment.

## Windows development environment

The maintainer selected **Windows 11 x64** for the development VM and first
release, using Microsoft's official **90-day Enterprise evaluation**. T520 is
resolved; Windows 10 and ARM64 are outside the first release scope.

The Linux host has 32 AMD-V CPU threads, 90 GiB RAM, accessible `/dev/kvm`, QEMU
8.2.2 and Microsoft-enrolled UEFI firmware. The prepared VM has 8 virtual CPUs,
16 GiB RAM, a 128 GiB expandable disk and a separate software TPM 2.0. VM files
live outside the repository in `~/.local/share/blent-dev/windows-vm/`.

Install only Windows and necessary development tools: MSVC compiler/Windows SDK,
minimal Rust/Cargo, Git, the stock FFmpeg CLI needed by encoder tests and guest integration needed to control the VM. No Visual
Studio IDE. Remove gaming/Xbox/Copilot packages; disable Windows Search UI and
indexing, web-search suggestions, widgets and consumer promotions. Record and
verify the resulting installed state; a prepared policy is not proof it applied.

A Windows VM can supply native kernel/API, filesystem ACL, process lifecycle,
priority and test coverage evidence. Ordinary Linux containers cannot supply the
Windows desktop/kernel. GPU/display-driver/physical USB acceptance remains a
separate hardware exercise. GPU or USB passthrough would require explicit device
allocation and validation; do not detach hardware used by the Linux session.

T631 and the native pinning/ownership failures (T634/T637) are fixed.
[Run 36264925943](https://github.com/geraldo-netto/UScreen/actions/runs/36264925943)
at `44b939e` passed the full native workspace suite and MSVC/GNU builds. T493's
46 Windows foundation functions each meet the 80% executable-line threshold,
including ACLs, leases, token privacy, junction rejection and child retirement.
T583 scheduling and owned-child propagation/denial acceptance also passed.
This CI uses Windows Server 2022. The retained Windows 11 VM now also passes
native portable/workspace tests, the all-features build and ordinary-user
lifecycle acceptance. [T633's final environment and evidence](2026-09-26-windows-development-vm.md)
cover TPM/Secure Boot, consumer-app removal, actual Xbox activation denial,
Search/indexing, the corrected clock and automatic login enabled at the
maintainer's request.

The VM, disk, firmware/TPM state and private setup remain retained. The user
service `blent-windows-vm.service` starts the existing disk; its installation
marker refuses a repeated installation. Open **Blent Windows 11 Dev** from the
Linux application menu, or inspect
`~/.local/share/blent-dev/windows-vm/provision-status.json`. This development
service does not enable Blent display/input/camera sharing.

## Input map

| ID(s) | Input needed from the maintainer | Work or evidence still required |
| --- | --- | --- |
| T628 | Resolved; nothing needed. | Primary/backup metadata corrected and matching certificate verified; key material preserved. |
| T621 | Devices are now provisioned with the correct names and exclusive capabilities. Permit a fresh short camera test; the earlier 30-second authorization was used. | Actual webcam-consumer frames, blank retirement and reconnect; decoder timing alone is insufficient. |
| T578 | Choose a separate graphics-stack test window or prioritize an explicit linear/exportable-surface implementation. | Current X server exposes DRI3 1.0; cross-GPU implicit import is corrupt. A validated explicit-layout path is needed before removing the fallback guard. A Windows VM does not solve this Linux graphics limitation. |
| T583 | Resolved; nothing needed. | Native priority, child propagation and denied-handle checks pass, with 100% affected-function coverage. Windows 11 native acceptance also passed under T633; no macOS work required. |
| T561 | A short normal sharing/trace window with the tablet available. No forced lock or special EVDI detach is needed. | Correlate capture/encode/transport sequence timing to establish the historical omission cause before changing pacing. |
| T549 | Timestamp and preceding action if the lock recurs, or a coordinated reproduction window when you can manually unlock. | Correlate power/activity/keyguard records; the historical caller is unknown. Do not force-lock the live session to create evidence. |
| T564 | Access to compatible NVIDIA NVENC and/or VP9/AV1 VAAPI encoding hardware/drivers. | Bounded tests using the bundled FFmpeg. The current GPU/software VM does not supply these missing encoder capabilities. |
| T540 | Be present to unplug/replug the physical USB cable during an explicitly authorized camera test. | Release, inactive black output and explicit selection after reconnect. Combine with T621 once devices exist. |
| T538 | Exact hub and charger models, and which device/cable connects to each port; then a stable test below full battery. | Establish a working data/power arrangement and sustained net charging. Advertised current and brief near-full samples do not prove it. |
| T493, T497 | Resolved; nothing needed. | Native foundation and host/GUI lifecycle acceptance pass. The archived local combined gate passed all 2,230 maintained functions/methods. Remote Linux reproducibility remains open under T645; see the coverage report. |
| T520 | **Answered: Windows 11 x64 first.** | Resolved and removed from TODO; target encoded in the support/build plan. |
| T522 | A Windows PC/tester with the existing tablet, Windows build/GPU details and a USB test window; administrator access only for the chosen driver installation. | Physical GPU/display/input, lock/sleep/reconnect and cleanup acceptance. A basic VM covers isolated contracts, not this complete result. |
| T524 | Resolved; nothing needed. | Native start/stop/restart/status, single-instance ownership and cleanup acceptance pass. Capture/input remain unsupported. |
| T525 | Tablet/USB availability when connection acceptance is ready. | T524 is validated; implement ADB transport next. Physical acceptance remains under T522. |
| T527 | Driver strategy: validated upstream ownership extension/exclusive instance, or a Blent-owned IDD. An owned driver also needs an agreed signing/maintenance/distribution owner. | Current upstream VDD is a lab candidate only: its global count/reload interface cannot retire only Blent-owned outputs. Do not buy a certificate before deciding this route. |
| T528 | No independent preference after T527's driver decision. | Implement and validate capture on the selected output; lifecycle, mode changes and device loss remain mandatory. |
| T529 | No new choice for the encoder adapter; T527 still needs the driver decision. | Native pipe/software-encoder implementation and coverage passed after T583/T493. T529 now waits for T525 connection ownership and T528 capture integration; it remains in TODO. |
| T530 | Compatible GPU access under T522. | T529 first, then actual hardware initialization/measurements and fallback. Advertised encoder names are insufficient. |
| T531, T532 | Nothing now. | Tray still waits for T525 connection status. Autostart is actionable now that T524 single-instance handling is validated. |
| T534 | Installer format and dependency distribution. Suggested starting point: per-user EXE installer, pinned ADB/FFmpeg bundled, separately installed driver. Confirm publisher/signing route before distribution. | Functioning milestones, redistribution notices, reproducible package checks, upgrade/uninstall ownership and T522 evidence. This suggestion is not an accepted decision. |
| T535 | A controlled benchmark window on the T522 machine/tablet. | Working T525/T528/T529 stream first; test accelerated candidates only where T530 validates them. |
| T388 | An uninterrupted **60–90 minute** host/tablet window, keeping desktop focus and workload stable. | Balanced USB normal/saver comparisons plus streaming-off control; prior incomplete samples do not establish a power saving. |
| T419 | Decide whether RGB fidelity/CPU benefits justify implementation independently of battery. Otherwise keep the repeatable battery-advantage gate. | The earlier battery advantage did not repeat; wire integrity, bounded decoding and lifecycle work remains before production use. |
| T597 | Nothing; keep deferred unless report sizes/frequency grow or profiling identifies meaningful cost. | Current bounded sorting cost is too small to justify the change. |
| T414 | Explicitly resume it and identify a recurring audio-gap window, ideally with timestamps. | Correlate Chrome/PipeWire/Bluetooth and scheduling; do not introduce audio transport or delay without a separate decision. |
| T537 | Explicitly resume the deferred tray-shutdown investigation. | Reproduce under an isolated D-Bus watcher; no EVDI attachment is needed. No additional hardware decision. |
| T222 | Only a new occurrence, with its timestamp and readable crash evidence. | Remains deferred; it does not block ordinary authorized reloads or other work. |
| T558 | A suitable stock upstream fix, or an explicit revised decision allowing dependency changes. | Stock libevdi is intentionally preserved; do not bypass display ownership or grant root. |
| T544 | Accept the camera milestone after remaining acceptance; then prioritize microphone sharing and define latency/echo needs. | Explicit microphone consent/lifecycle and native audio-backend design. Still deferred. |
| T545 | After camera acceptance, specify audio direction: computer audio to tablet speakers, tablet audio to computer, or both; identify desired OS device behavior. | Routing/buffering/lifecycle design depends on that answer. Still deferred. |
| T546 | After camera acceptance, choose NFC operations such as tag reading/writing versus card emulation, with the intended use case. | Research actual tablet/Android/desktop limits before promising a generic OS NFC device. Still deferred. |
| T487 | Explicitly prioritize it after T388; provide the relevant network/power arrangement and uninterrupted window. Pen measurements also need a stylus. | Keep transport, pen and power-source comparisons separate; remains deferred. |
| T417 | Choose read-only decoder diagnostics or profile-selection controls. Read-only is the smaller first step. | Preserve current defaults; controls additionally need persistence and measured safe transitions. Still deferred. |

T592 remains skipped/blocked for lack of a physical stylus, as already directed.
No additional tablet or broad large-machine campaign is required.

## One-time Linux camera prerequisite

The maintainer has now loaded `/dev/video20` as **Blent Front** and `/dev/video21`
as **Blent Rear**, both with exclusive capabilities enabled. No reload is needed.
For reference, the one-time load command is:

```sh
sudo modprobe v4l2loopback devices=2 video_nr=20,21 \
  card_label="Blent Front,Blent Rear" exclusive_caps=1,1
```

To preserve these defaults, put the following in `/etc/modprobe.d/blent-camera.conf`:

```conf
options v4l2loopback devices=2 video_nr=20,21 card_label="Blent Front,Blent Rear" exclusive_caps=1,1
```

For automatic loading, `/etc/modules-load.d/blent-camera.conf` contains only
`v4l2loopback`. These system files are supplied for the maintainer to create; the
agent has not written them. Loading the module does not authorize camera capture.
The coordinated camera/USB tests still need the test-window and capture choices
above. Do not unload a loopback module already used by another application.

References: [Windows evaluation](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise),
[Windows container prerequisites](https://learn.microsoft.com/en-us/virtualization/windowscontainers/about/faq),
[Windows Search policy](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-search#disablesearch),
[Copilot removal and prevention](https://learn.microsoft.com/en-us/windows/client-management/manage-windows-copilot),
[T521 driver decision brief](2026-09-26-windows-display-driver.md).
