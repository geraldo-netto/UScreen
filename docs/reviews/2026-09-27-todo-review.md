# TODO dependency and scope review — T684

Historical planning snapshot before the maintainer’s subsequent 2026-09-27 pen
scope decision. See [TODO.md](../../TODO.md) and the [Windows plan](../windows-port.md)
for current dispositions and prerequisites.

Reviewed the 31 unresolved rows remaining after T531 (`3c5ef51`) and T652
(`7ef089f`) against implementation, retained native evidence and maintainer
choices. No unresolved original ID was removed. Nine broad items were split
into 23 deliverables (14 additional IDs); other items remain intact where a
split would invent requirements or separate a fix from its necessary tests.

This review updates planning and documentation. It runs no camera, stylus,
physical Windows, power, display-driver or GPU acceptance campaign. Historical
measurements retain their original configurations/dates; they are not fresh
observations of the current installation. The existing coverage, regression,
platform and maximum-complexity requirements remain unchanged.

## Corrected dependencies

T526 cited T525 as future work even though USB negotiation, route ownership and
native daemon fixtures are implemented. `host/src/usb/preview.rs` still installs
unavailable input/capture adapters, so connection readiness does not establish
input delivery. Mapping, injection and session integration are separate tasks;
physical input remains unvalidated.

T530 combined tooling with hardware enablement under one blocker. The software
pipe adapter is available for isolated native probes, so discovery and injected
failure tests do not require an extended display. Conversely,
`host/src/pipe_encoder.rs::validate` accepts **libx264 only**, and
`common/src/encoding.rs` has **no AMF/QSV recipes**. The revised work explicitly
includes those missing recipes and platform probe/resource adapters. It does
not infer working hardware encoding from the existing pipe process wrapper.

T544–T546 referred only to T543 even though host camera controls are implemented.
The remaining camera acceptance is T621/T540. Their explicit deferral and
unanswered audio/NFC requirements remain in force; completing camera tests will
not automatically authorize those features.

The [prerequisite map](2026-09-26-blocker-inputs.md) still called T525/T531 pending
and T645 unresolved. It now reflects the implemented USB/tray and isolated
sysfs-coverage fix, while retaining the unrelated T644 readiness-stall blocker.
The [Windows plan](../windows-port.md) names the new task boundaries and treats
the tray as completed. No source change was needed to reconcile these statements.

## Natural decomposition

| Original item | Retained scope and extracted deliverables | Why the boundaries are useful |
| --- | --- | --- |
| T526 | T672 monitor mapping; T526 native injection/contact ownership; T673 session integration | Geometry, native device operations and authenticated daemon wiring have separate interfaces and acceptance. Mapping/injection can start without a virtual-display decision. |
| T522 | T522 physical connection/pen-only; T674 physical extended display | Pen-only acceptance need not wait for a virtual-display driver. Both use the same single PC/tablet. |
| T527 | T527 driver ownership/acquisition; T675 mode/placement control | Owned create/remove must be established before modifying display modes; unrelated displays remain protected. |
| T528 | T528 frame acquisition/conversion; T676 access-loss/mode recovery | Frame validity and lifecycle recovery have distinct contracts. Neither alone enables Windows display support. |
| T530 | T685 missing encoder recipes; T530 discovery/probe orchestration; T677 native hardware/session enablement | Tooling and policy can advance without GPU/tablet availability; hardware support cannot. |
| T534 | T534 reproducible package contents/dependencies; T678 installation/upgrade/uninstall | Artifact composition and installed-state ownership have different validation. Format/bundling/signing choices remain pending. |
| T492 | T492 functional adaptive-idle acceptance; T679 sustained battery comparison | Correctness does not depend on demonstrating a power benefit; power claims still need controlled measurements. |
| T419 | T419 bounded wire/isolated replay; T680 renderer ownership; T681 live source/freshness integration; T686 complete-path comparison | These follow the existing conditional research plan. All inherit the battery-evidence or fidelity/CPU-priority gate. Experimental integration is not production enablement. |

T487 is also separated into network (T487), pen interaction (T682) and alternative
power supply (T683), preserving its explicitly requested *separate experiments*.
All remain deferred after T388 and require their own prioritization and setup.
The eight implementation/acceptance chains above and this experiment split
account for the 14 additional IDs.

## Remaining row review

| Items | Code/evidence checked | Disposition and reason not to split further |
| --- | --- | --- |
| T644 | `scripts/tests/test_benchmark_visibility.py`, [timeout evidence](artifacts/2026-09-27-xvfb-diagnostics/README.md) | Diagnostics exist; the causal child/syscall evidence does not. One bounded diagnosis, followed by its regression/fix if reproduced. |
| T592 | `host/src/input/mapping.rs`, [native X11 mapping](2026-09-26-xrandr-mapping.md) | Physical stylus unavailable and maintainer skipped the item. Initial Xorg base-node classification alone is not a defect. |
| T621, T540 | `host/src/camera/`, `common/src/camera/`, [camera controls](../cameras.md), [dated host acceptance](2026-09-20-camera-host.md) | Separate consumer-freshness and physical-cable checks already exist. Correct provisioning is retained evidence, not fresh capture authorization. |
| T578 | `host/gpu/identity.c`, `host/gpu/capture.c`, [explicit-layout probe](2026-09-26-gpu-layout.md) | Production cross-GPU admission still lacks a validated export route. Further adapter design depends on an upgraded test stack or selection of the alternate owned-surface route; keep guard and FIFO fallback. |
| T561 | `host/src/latency.rs`, `host/src/capture/cli_encoder.rs`, [queue/latch correlation](../benchmarks/2026-09-21-frame-omissions/README.md) | One unresolved causal investigation. Queue gaps are measured from earlier queues, not time-to-latch; codec correlation was temporal, not an explicit SurfaceFlinger identity join. Additional instrumentation depends on the next trace. |
| T549 | [retained power/activity evidence](../benchmarks/2026-09-21-android-lock/README.md), Android activity/service lifecycle | Caller and event identity remain unknown. Do not turn a possible cause into a speculative fix. |
| T564 | `packaging/appimage/ffmpeg.json`, [bundled codec tests](../benchmarks/2026-09-21-ffmpeg6/README.md) | Bounded native coverage awaits compatible hardware. Old unavailable capabilities are dated observations; no broader T382 campaign. |
| T538 | [hub/cable observations](../benchmarks/2026-09-20-usb-hub-power/README.md) | Exact models/wiring are missing. Short near-full measurements prove neither charging capacity nor a software fault. Comparative supplies are separately deferred in T683. |
| T529 | `host/src/pipe_encoder.rs`, `host/src/usb/preview.rs`, [native pipe evidence](2026-09-26-pipe-encoder.md) | Previously implemented transport/process work is removed from remaining scope. Capture-to-session integration, deadlines and retirement form one cohesive deliverable. |
| T535 | [Windows plan](../windows-port.md), existing benchmark boundaries | One controlled complete-stream benchmark on the same machine/tablet; isolated GPU probes belong to T677. Splitting metric collection would duplicate the controlled workload. |
| T388 | [partial USB comparison](../benchmarks/2026-09-18-usb-power-comparison.md), [power-policy coverage](../benchmarks/2026-09-18-power-policy.md) | Balanced pairs and streaming-off control constitute one experiment. Keep host-focus guard, deployed-build identity, settings and counter-resolution limits. |
| T597 | `host/src/latency.rs`, [measured sorting cost](2026-09-26-algorithm-costs.md) | Explicitly deferred until measured cost or report scale changes; no asymptotic-only optimization. |
| T414 | [passive audio sample](../benchmarks/2026-09-20-youtube-audio/README.md) | Explicitly deferred causal investigation. Conditional offset/drift measurements remain requirements if that symptom returns; no invented audio implementation task. Effort describes diagnosis, not an unscoped future transport. |
| T537 | `Cargo.lock`, `host/src/linux_main.rs`, `host/src/tray.rs` | Deferred Linux ksni shutdown reproduction remains independent of the completed Windows tray. Existing D-Bus tests do not establish the historical panic's cause. |
| T222 | [crash evidence](2026-09-17-cinnamon-restart.md) and maintainer deferral | No current reproduction or accessible old core. Keep the large conditional diagnosis intact; do not invent a cause, reopen it or block ordinary reloads. |
| T558 | `host/evdi/evdi_helper.c`, [stock-library decision](../benchmarks/2026-09-20-evdi-startup/README.md) | Dependency choice is explicit. A specific upstream mitigation or changed decision is required before meaningful implementation subdivision. |
| T544–T546 | [implemented camera controls](../cameras.md), [prior decision map](2026-09-26-blocker-inputs.md) | Remain large/deferred because audio direction/backend/latency and NFC operations are unresolved. Each now names the first concrete decision; speculative backend tasks would be artificial. |
| T417 | `DecoderSelection.kt`, `DecoderConfiguration.kt`, current Android UI | Diagnostics versus controls is unanswered. Preserve requested/advertised/effective distinctions, API27/fallback coverage and conditional persistence/transition requirements. |
| T584 | `common/src/platform.rs` and explicit macOS decision | Remains declined. No native macOS work or new exclusion introduced. |

## Validation

The ledger retains exactly the two project tables and their five columns. Original
unresolved IDs and deferred/declined dispositions are preserved; new IDs do not
collide with tracked history. Open entries identify work that can start now;
Other entries name actual dependencies, evidence, decisions or explicit deferrals.
Relative links, dependency ordering and requirement transfer were reviewed.
Review also found a stale T561 logging recipe, recorded separately as T687
for its own documentation fix. Documentation-only changes need no artificial
production regression tests.
