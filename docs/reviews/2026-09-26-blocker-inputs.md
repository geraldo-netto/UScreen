# Inputs needed for the reviewed TODO items

Implementation/dependency map refreshed 2026-09-27 after T525, T531, T652 and
the maintainer’s decision to retain touch/mouse while declining stylus work.
Physical observations below retain their original dates; devices were not
reprobed for this documentation review. [TODO.md](../../TODO.md) is the current
work ledger; [the row review](2026-09-27-todo-review.md) records the decomposition.
This map is not authorization to resume deferred experiments.

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
[Run 36264925943](https://github.com/geraldo-netto/blent/actions/runs/36264925943)
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
| T492, T679 | A functional adaptive-idle sharing window; separately, the requested roughly 25-minute battery window or an explicit deferral decision. | T492 checks complete-pipeline/watchdog/join/reconnect behavior. T679 compares sustained power only after functional acceptance; isolated raw-write savings are insufficient. |
| T549 | Timestamp and preceding action if the lock recurs, or a coordinated reproduction window when you can manually unlock. | Correlate power/activity/keyguard records; the historical caller is unknown. Do not force-lock the live session to create evidence. |
| T564 | Access to compatible NVIDIA NVENC and/or VP9/AV1 VAAPI encoding hardware/drivers. | Bounded tests using the bundled FFmpeg. The current GPU/software VM does not supply these missing encoder capabilities. |
| T540 | Be present to unplug/replug the physical USB cable during an explicitly authorized camera test. | Release, inactive black output and explicit selection after reconnect. Coordinate with T621 and recheck the provisioned devices. |
| T538 | Exact hub and charger models, and which device/cable connects to each port; then a stable test below full battery. | Establish a working data/power arrangement and sustained net charging. Advertised current and brief near-full samples do not prove it. |
| T493, T497 | Resolved; nothing needed. | Native foundation/lifecycle and the archived combined per-function gate passed. T645 subsequently fixed empty-sysfs native coverage reproducibility; [its evidence](artifacts/2026-09-27-sysfs-coverage/README.md) is separate from the still-unattributed T644 Xvfb stall. |
| T520 | **Answered: Windows 11 x64 first.** | Resolved and removed from TODO; target encoded in the support/build plan. |
| T522, T674 | One Windows 11 x64 PC/tester with the existing tablet, GPU/driver/USB details and a physical window; no stylus prerequisite. | T522 covers USB connection and, after T672/T689/T673, touch/mouse input. T674 adds extended display after T527/T675/T528/T676/T529. Administrator access is only for the selected driver installation. VM/CI evidence is not physical acceptance. |
| T524 | Resolved; nothing needed. | Native start/stop/restart/status, single-instance ownership and cleanup acceptance pass. Capture/input remain unsupported. |
| T672, T689, T673 | No virtual-display choice or stylus is needed for touch/mouse implementation; settle any missing non-stylus mouse source/gesture decision before dependent work. | T672 mapping and T689 native injection remain planned. T689 must supply an independent mouse contract because the existing virtual pointer is pen-derived; T673 integrates settings/session retirement. Physical acceptance remains T522. |
| T526 | None; Windows stylus work was declined on 2026-09-27. | No stylus injection or pressure/tilt/hover/eraser acceptance is planned. Touch/mouse requirements remain in T672/T689/T673/T522; existing pen code/tests are preserved. |
| T525 | No implementation choice remains; physical USB availability belongs to T522. | Authenticated USB preview, owned reverse routes, credentials, native fixtures and cleanup are implemented. Capture/input remain unavailable until their adapters and integration are accepted. See [USB results](2026-09-27-windows-usb.md). |
| T527 | Driver strategy: validated upstream ownership extension/exclusive instance, or a Blent-owned IDD. An owned driver also needs an agreed signing/maintenance/distribution owner. | Current upstream VDD is a lab candidate only: its global count/reload interface cannot retire only Blent-owned outputs. Do not buy a certificate before deciding this route. |
| T528, T676 | No independent preference after T527’s driver/control choice; native output compatibility must be demonstrated. | T528 acquires/converts frames; T676 adds bounded access-loss/lock/mode recovery, using T675 mode control. Native resource and cancellation tests accompany both. |
| T529 | No new choice for the software pipe adapter; T527 still needs the driver decision. | The libx264 pipe adapter and T525 USB ownership are available. T529 waits for capture/recovery and output modes, not another ADB implementation; see T528/T676/T675. |
| T685, T530, T677 | GPU/tablet access is needed for T677, not for starting recipe/discovery/probe tooling. | AMF/QSV recipes are absent and the pipe adapter admits libx264 only. T685 adds tested recipes; T530 isolates inventory/probe/selection orchestration; T677 requires real GPU and integrated session evidence before hardware enablement. |
| T531 | Resolved; nothing needed. | Native Windows tray now consumes USB status, opens Settings, requests normal Quit cleanup and restores the icon after Explorer recreation. [Native tests and changed-function coverage](2026-09-27-windows-tray.md) pass. Linux ksni teardown is separately resolved by T537. |
| T532 | Nothing now. | Resolved: per-user registration, repeated-login ownership, native failure regressions and ordinary-user coverage pass; see [autostart results](2026-09-27-windows-autostart.md). |
| T534, T678 | Installer format, ADB/FFmpeg bundling versus prerequisites, and application signing route; driver responsibilities remain under T527. | A per-user EXE with pinned dependencies was only a suggestion. T534 covers reproducible package contents; T678 covers install/upgrade/uninstall ownership. Shipping needs accepted Windows milestones and T522/T674 evidence. |
| T535 | A controlled benchmark window on the same T522/T674 machine/tablet. | Requires working T529 streaming; compare accelerated candidates only where T677 validates them. No additional machine/tablet campaign. |
| T388 | An uninterrupted **60–90 minute** host/tablet window, keeping desktop focus and workload stable. | Balanced USB normal/saver comparisons plus streaming-off control; prior incomplete samples do not establish a power saving. |
| T419, T680, T681, T686 | Choose RGB fidelity/CPU independently of battery, or retain the repeatable battery-advantage gate; later supply a comparative device window. | T419 bounded wire/replay; T680 renderer retirement/fallback; T681 live source/freshness integration; T686 complete-path comparison. Prior CPU savings did not establish repeatable battery savings. |
| T597 | Resolved; nothing needed. | Exact hybrid order-statistic summaries retain bounded-window semantics; [Linux/Windows tests and microbenchmarks](2026-09-27-latency-summary.md) pass. |
| T414 | Blocked on a timestamped recurring audio gap and correlated playback/audio/scheduling evidence. | [Fresh Bluetooth errors](2026-09-27-audio-followup.md) are not yet matched to a reported gap. Latest symptom details remain requested; no causal fix is established. |
| T537 | Resolved; nothing needed. | Explicit owned-service shutdown fixes the reproduced ksni panic and leaked D-Bus registration; [permanent private-bus regressions](2026-09-27-tray-shutdown.md) pass. |
| T222 | Resumed 2026-09-27; a current occurrence/trigger or readable causal crash evidence is still missing. | Isolated ownership/lifecycle review can proceed using T558 evidence; a fix needs a reproduced cause. Ordinary authorized reloads remain independent. |
| T558 | Blocked on the requested stock-library versus narrow-patch decision. | [Current upstream review](2026-09-27-evdi-acquisition.md) found no stock mitigation/API; a patch still needs permanent deadline/permission/ownership regressions and native validation. |
| T544 | Accept the camera milestone after T621/T540, then prioritize microphone sharing and define desktop backends and latency/echo needs. | T543 camera controls already exist. Microphone consent/foreground lifecycle and implementation remain deferred; further splitting needs the audio requirements. |
| T545 | After camera acceptance T621/T540, prioritize and specify audio direction and desired OS device behavior. | Routing/buffering/backend design depends on that decision. T543 is implemented; this work remains deferred. |
| T546 | After camera acceptance T621/T540, prioritize and choose NFC operations/use cases such as tag reading/writing versus card emulation. | Research tablet/Android/desktop limits before defining implementation or promising an OS NFC device. Remains deferred. |
| T487, T683 | Explicitly prioritize each after T388, with its own uninterrupted window and network/power prerequisites. | Network and alternative supply remain deferred controlled experiments; T538 identifies the current hub arrangement, not a comparative power result. Pen experiment T682 was declined on 2026-09-27. |
| T417 | Resumed 2026-09-27; read-only diagnostics versus profile controls question pending. | Expose actual/advertised/requested decoder state accurately; controls would additionally require persistence and safe transitions. |

T592 stylus investigation/acceptance and T682 pen power testing were declined by
the maintainer on 2026-09-27. Existing code, regressions and historical evidence
are retained; these dispositions do not establish physical pen validation.
No additional tablet or broad large-machine campaign is required.

## One-time Linux camera prerequisite

The retained provisioning check recorded `/dev/video20` as **Blent Front** and `/dev/video21`
as **Blent Rear**, both with exclusive capabilities enabled. Recheck availability
at the next accepted test window; this review did not reload or probe them.
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
